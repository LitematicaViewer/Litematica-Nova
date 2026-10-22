import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
import { displayFilePath } from "../../../src/business/facade";
import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent } from "react";

import {
  loadStructuralBlocks,
  loadUserConfigMigratingLocalStorage,
  normalizeMaterialListWindowBehavior,
  normalizeDebugMode,
  openEnumeratorWindow as openEnumeratorDesktopWindow,
} from "../../../src/business/facade";
import { listenEvent } from "../../../src/platform/events";
import {
  buildEnumeratorValueRows,
  createRuntimeProjectionCollection,
  evaluateEnumeratorExpression,
  loadEnumeratorCollections,
  saveEnumeratorCollection,
  type EnumeratorCollection,
  type EnumeratorCollectionCategory,
  type EnumeratorValueType,
} from "../../../src/services/enumeratorService";
import { BlockIcon } from "../../components/BlockIcon";
import { Dropdown } from "../../components/Dropdown";
import { VirtualSpacerCell } from "../../components/VirtualSpacerCell";
import { useVirtualWindow } from "../../components/useVirtualWindow";
import { Checkbox } from "../../components/ui/checkbox";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../components/ui/table";
import {
  applyThemeStylesheet,
  currentThemeId,
  subscribeToThemeChanges,
  themeClassName,
} from "../../shell/themeRuntime";

const enumeratorOpenFileEvent = "enumerator-open-file";

type CategoryFilter = "all" | "base" | "version" | "system_enum" | "creative" | "map" | "custom";
const CATEGORY_FILTER_OPTIONS: { label: string; value: CategoryFilter }[] = [
  { get label() { return t("library.allCategories"); }, value: "all" },
  { get label() { return t("library.baseCatalog"); }, value: "base" },
  { get label() { return t("library.version"); }, value: "version" },
  { get label() { return t("library.creativeMode"); }, value: "creative" },
  { get label() { return t("library.systemEnum"); }, value: "system_enum" },
  { get label() { return t("library.mapColors"); }, value: "map" },
  { get label() { return t("library.custom"); }, value: "custom" },
];
type CopyToast = {
  id: number;
  value: string;
  x: number;
  y: number;
};

const initialFileFromUrl = () => {
  try {
    return new URLSearchParams(window.location.search).get("file") || "";
  } catch {
    return "";
  }
};

function categoryLabel(category: EnumeratorCollectionCategory): string {
  switch (category) {
    case "base":
      return t("library.baseCatalog");
    case "version":
      return t("library.version");
    case "creative":
      return t("library.creativeMode");
    case "system_enum":
      return t("library.systemEnum");
    case "map":
      return t("library.mapColors");
    case "custom":
      return t("library.custom");
    case "runtime":
      return t("library.runtime");
    default:
      return category;
  }
}

function reorderValues(values: string[], selected: string[], direction: -1 | 1): string[] {
  if (!selected.length) return values;
  const next = [...values];
  const indexes = values
    .map((value, index) => (selected.includes(value) ? index : -1))
    .filter((index) => index >= 0);

  if (direction < 0) {
    for (const index of indexes) {
      if (index <= 0 || selected.includes(next[index - 1])) continue;
      [next[index - 1], next[index]] = [next[index], next[index - 1]];
    }
  } else {
    for (let idx = indexes.length - 1; idx >= 0; idx -= 1) {
      const index = indexes[idx];
      if (index >= next.length - 1 || selected.includes(next[index + 1])) continue;
      [next[index], next[index + 1]] = [next[index + 1], next[index]];
    }
  }
  return next;
}

function collectionDisplayName(collection: EnumeratorCollection): string {
  if (collection.readOnly && collection.category === "runtime") return t("library.collection.runtime");
  if (collection.readOnly && ["base:dv-blocks", "base:dv-items", "base:dv-enchantments", "base:dv-entities"].includes(collection.id)) return t(`library.collection.${collection.id}`);
  return collection.name;
}

function collectionSummary(collection: EnumeratorCollection): string {
  return t("library.collectionSummary", { category: categoryLabel(collection.category), count: collection.values.length });
}

async function writeClipboardText(value: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return;
    } catch {
      // Some WebView/browser contexts expose clipboard but reject it; try the
      // legacy selection path before surfacing a failure to the user.
    }
  }

  const textArea = document.createElement("textarea");
  textArea.value = value;
  textArea.setAttribute("readonly", "true");
  textArea.style.position = "fixed";
  textArea.style.left = "-9999px";
  textArea.style.top = "0";
  document.body.appendChild(textArea);
  textArea.select();
  try {
    const copied = document.execCommand("copy");
    if (!copied) {
      throw new Error("copy command rejected");
    }
  } finally {
    document.body.removeChild(textArea);
  }
}

/**
 * Opens the enumerator according to the user's configured child-window behavior.
 */
export async function openEnumeratorWithWindowBehavior(currentFile: string, showOverlay: () => void): Promise<void> {
  const info = await loadUserConfigMigratingLocalStorage();
  if (!normalizeDebugMode(info.config.debug_mode)) return;
  const behavior = normalizeMaterialListWindowBehavior(info?.config.material_list_window_behavior);
  if (behavior === "independent_window") {
    try {
      await openEnumeratorDesktopWindow(currentFile);
      return;
    } catch {
      // Browser preview cannot create a desktop window, so fall back to the in-window dialog.
    }
  }
  showOverlay();
}

export function EnumeratorDialog({
  theme,
  currentFile,
  runtimeCollection,
  onClose,
}: {
  theme: string;
  currentFile: string;
  runtimeCollection: EnumeratorCollection | null;
  onClose: () => void;
}) {
  useI18n();
  return (
    <div className={["dialog-overlay", themeClassName(theme)].filter(Boolean).join(" ")}>
      <EnumeratorContent currentFile={currentFile} runtimeCollection={runtimeCollection} onClose={onClose} />
    </div>
  );
}

export function EnumeratorWindow() {
  useI18n();
  const [themeId, setThemeId] = useState(currentThemeId());
  const [currentFile, setCurrentFile] = useState(initialFileFromUrl());
  const [runtimeCollection, setRuntimeCollection] = useState<EnumeratorCollection | null>(null);
  const [runtimeStatus, setRuntimeStatus] = useState<UiMessage>("");

  useEffect(() => {
    applyThemeStylesheet(themeId);
  }, [themeId]);

  useEffect(() => {
    setThemeId(currentThemeId());
    return subscribeToThemeChanges(setThemeId);
  }, []);

  useEffect(() => {
    const unlistenPromise = listenEvent<string | null>(enumeratorOpenFileEvent, (event) => {
      setCurrentFile(event.payload || "");
    }).catch(() => undefined);
    return () => {
      unlistenPromise.then((unlisten) => unlisten?.());
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    if (!currentFile) {
      setRuntimeCollection(null);
      setRuntimeStatus("");
      return;
    }
    setRuntimeStatus(message("library.runtimeCollectionLoading"));
    loadStructuralBlocks(currentFile)
      .then((blocks) => {
        if (cancelled) return;
        setRuntimeCollection(createRuntimeProjectionCollection(currentFile, blocks));
        setRuntimeStatus(message("library.runtimeCollectionLoaded", { count: blocks.length }));
      })
      .catch((error) => {
        if (cancelled) return;
        setRuntimeCollection(null);
        setRuntimeStatus(message("library.currentSchematicReadFailed", { error: String(error) }));
      });
    return () => {
      cancelled = true;
    };
  }, [currentFile]);

  return (
    <main className={["subwindow-standalone-page", "enumerator-window", themeClassName(themeId)].filter(Boolean).join(" ")}>
      <EnumeratorContent currentFile={currentFile} runtimeCollection={runtimeCollection} runtimeStatus={localizeMessage(runtimeStatus)} standalone />
    </main>
  );
}

function EnumeratorContent({
  currentFile,
  runtimeCollection,
  runtimeStatus = "",
  onClose,
  standalone = false,
}: {
  currentFile: string;
  runtimeCollection: EnumeratorCollection | null;
  runtimeStatus?: UiMessage;
  onClose?: () => void;
  standalone?: boolean;
}) {
  const { locale, revision } = useI18n();
  const [collections, setCollections] = useState<EnumeratorCollection[]>([]);
  const [isLoadingCollections, setIsLoadingCollections] = useState(false);
  const [status, setStatus] = useState<UiMessage>("");
  const [error, setError] = useState<UiMessage>("");
  const [searchKeyword, setSearchKeyword] = useState("");
  const [categoryFilter, setCategoryFilter] = useState<CategoryFilter>("all");
  const [sourceCollectionId, setSourceCollectionId] = useState("");
  const [sourceExpression, setSourceExpression] = useState("");
  const [subtractRight, setSubtractRight] = useState(false);
  const [editCollectionId, setEditCollectionId] = useState("");
  const [editName, setEditName] = useState("");
  const [editValueType, setEditValueType] = useState<EnumeratorValueType>("mixed");
  const [editVersion, setEditVersion] = useState("custom");
  const [editDescription, setEditDescription] = useState("");
  const [leftSearchKeyword, setLeftSearchKeyword] = useState("");
  const [rightSearchKeyword, setRightSearchKeyword] = useState("");
  const [rightValues, setRightValues] = useState<string[]>([]);
  const [rightSelected, setRightSelected] = useState<string[]>([]);
  const [isSaving, setIsSaving] = useState(false);
  const [copyToast, setCopyToast] = useState<CopyToast | null>(null);
  const copyToastTimerRef = useRef<number | null>(null);

  const hydrateRightEditor = useCallback((collection: EnumeratorCollection | null) => {
    if (!collection) {
      setEditCollectionId("");
      setEditName("");
      setEditValueType("mixed");
      setEditVersion("custom");
      setEditDescription("");
      setRightValues([]);
      setRightSelected([]);
      return;
    }
    setEditCollectionId(collection.category === "custom" ? collection.id : "");
    setEditName(collection.name);
    setEditValueType(collection.valueType);
    setEditVersion(collection.version);
    setEditDescription(collection.description);
    setRightValues(collection.values);
    setRightSelected([]);
  }, []);

  const reloadCollections = useCallback(async (preserveEditId = "") => {
    setIsLoadingCollections(true);
    setError("");
    try {
      const next = await loadEnumeratorCollections(runtimeCollection ? [runtimeCollection] : []);
      setCollections(next);

      if (preserveEditId) {
        const preserved = next.find((collection) => collection.id === preserveEditId) || null;
        if (preserved) {
          hydrateRightEditor(preserved);
          setIsLoadingCollections(false);
          return;
        }
      }
    } catch (err: any) {
      setError(message("library.collectionsReadFailed", { error: String(err) }));
      setCollections(runtimeCollection ? [runtimeCollection] : []);
    } finally {
      setIsLoadingCollections(false);
    }
  }, [hydrateRightEditor, runtimeCollection]);

  useEffect(() => {
    reloadCollections(editCollectionId);
  }, [reloadCollections]);

  const editingCollection = useMemo(
    () => collections.find((collection) => collection.id === editCollectionId) || null,
    [collections, editCollectionId],
  );

  const filteredCollections = useMemo(() => {
    const keyword = searchKeyword.trim().toLowerCase();
    return collections.filter((collection) => {
      const categoryMatch = categoryFilter === "all"
        ? true
        : categoryFilter === "custom"
          ? collection.category === "custom" || collection.category === "runtime"
          : collection.category === categoryFilter;
      if (!categoryMatch) return false;
      if (!keyword) return true;
      return [collectionDisplayName(collection), collection.name, collection.id]
        .filter(Boolean)
        .some((value) => value.toLowerCase().includes(keyword));
    });
  }, [categoryFilter, collections, searchKeyword, locale]);

  const expressionResult = useMemo(
    () => evaluateEnumeratorExpression(sourceExpression, collections),
    [collections, sourceExpression],
  );

  const sourceValues = useMemo(() => {
    if (!subtractRight) return expressionResult.values;
    const currentRight = new Set(rightValues);
    return expressionResult.values.filter((value) => !currentRight.has(value));
  }, [expressionResult.values, rightValues, subtractRight]);

  const leftRows = useMemo(() => buildEnumeratorValueRows(sourceValues, collections), [collections, sourceValues, revision]);
  const rightRows = useMemo(() => buildEnumeratorValueRows(rightValues, collections), [collections, rightValues, revision]);

  const filteredLeftRows = useMemo(() => {
    const keyword = leftSearchKeyword.trim().toLowerCase();
    if (!keyword) return leftRows;
    return leftRows.filter((row) => [row.name, row.id].some((value) => value.toLowerCase().includes(keyword)));
  }, [leftRows, leftSearchKeyword]);

  const filteredRightRows = useMemo(() => {
    const keyword = rightSearchKeyword.trim().toLowerCase();
    if (!keyword) return rightRows;
    return rightRows.filter((row) => [row.name, row.id].some((value) => value.toLowerCase().includes(keyword)));
  }, [rightRows, rightSearchKeyword]);

  const leftValueTableRef = useRef<HTMLDivElement | null>(null);
  const rightValueTableRef = useRef<HTMLDivElement | null>(null);
  // The rendered value row is a 32px icon plus 6px vertical padding plus the collapsed
  // border (see subwindow.css `.material-list-table th, .material-list-table td`), so the
  // spacer must reserve 45px per row rather than the old round 40px.
  const {
    visibleItems: visibleLeftRows,
    startIndex: leftStartIndex,
    topSpacerHeight: leftTopSpacerHeight,
    bottomSpacerHeight: leftBottomSpacerHeight,
  } = useVirtualWindow(filteredLeftRows, leftValueTableRef, 45, 10);
  const {
    visibleItems: visibleRightRows,
    startIndex: rightStartIndex,
    topSpacerHeight: rightTopSpacerHeight,
    bottomSpacerHeight: rightBottomSpacerHeight,
  } = useVirtualWindow(filteredRightRows, rightValueTableRef, 45, 10);

  const handleCreateCollection = () => {
    hydrateRightEditor(null);
    setStatus(message("library.collectionDraftCreated"));
  };

  const handlePickSourceCollection = (collection: EnumeratorCollection) => {
    setSourceCollectionId(collection.id);
    setSourceExpression(collection.name);
    setStatus(message("library.collectionAsSource", { name: collection.readOnly && (collection.category === "runtime" || collection.id.startsWith("base:dv-")) ? message(collection.category === "runtime" ? "library.collection.runtime" : `library.collection.${collection.id}`) : collection.name }));
  };

  const handleLoadRightCollection = (collection: EnumeratorCollection) => {
    hydrateRightEditor(collection);
    setStatus(collection.readOnly ? message("library.collectionAsCopy", { name: collection.readOnly && (collection.category === "runtime" || collection.id.startsWith("base:dv-")) ? message(collection.category === "runtime" ? "library.collection.runtime" : `library.collection.${collection.id}`) : collection.name }) : message("library.collectionForEdit", { name: collection.readOnly && (collection.category === "runtime" || collection.id.startsWith("base:dv-")) ? message(collection.category === "runtime" ? "library.collection.runtime" : `library.collection.${collection.id}`) : collection.name }));
  };

  const handleAddValueToRight = (value: string) => {
    setRightValues((current) => current.includes(value) ? current : [...current, value]);
  };

  const handleRemoveValueFromRight = (value: string) => {
    setRightValues((current) => current.filter((item) => item !== value));
    setRightSelected((current) => current.filter((item) => item !== value));
  };

  const handleToggleRightSelection = (value: string) => {
    setRightSelected((current) => current.includes(value)
      ? current.filter((item) => item !== value)
      : [...current, value]);
  };

  const showCopyToast = (value: string, x: number, y: number) => {
    if (copyToastTimerRef.current !== null) {
      window.clearTimeout(copyToastTimerRef.current);
    }
    setCopyToast({ id: Date.now(), value, x, y });
    copyToastTimerRef.current = window.setTimeout(() => {
      setCopyToast(null);
      copyToastTimerRef.current = null;
    }, 1000);
  };

  const handleCopyValue = async (value: string, event: MouseEvent) => {
    const { clientX, clientY } = event;
    try {
      await writeClipboardText(value);
      showCopyToast(value, clientX, clientY);
    } catch (err: any) {
      setError(message("library.copyFailed", { error: String(err?.message || err) }));
    }
  };

  const handleMoveSelected = (direction: -1 | 1) => {
    setRightValues((current) => reorderValues(current, rightSelected, direction));
  };

  const persistCollection = async (asCopy: boolean) => {
    const name = editName.trim() || (editingCollection?.name || t("library.newCollection"));
    const inputName = asCopy ? t("library.collectionCopyName", { name: name }) : name;
    setIsSaving(true);
    setError("");
    try {
      const saved = await saveEnumeratorCollection({
        id: asCopy ? "" : editCollectionId,
        name: inputName,
        symbol: "",
        valueType: editValueType,
        version: editVersion,
        description: editDescription,
        values: rightValues,
      });
      setStatus(asCopy ? message("library.collectionCopySaved", { name: saved.name }) : message("library.collectionSaved", { name: saved.name }));
      setEditCollectionId(saved.id);
      await reloadCollections(saved.id);
    } catch (err: any) {
      setError(message("library.collectionSaveFailed", { error: String(err) }));
    } finally {
      setIsSaving(false);
    }
  };

  useEffect(() => {
    return () => {
      if (copyToastTimerRef.current !== null) {
        window.clearTimeout(copyToastTimerRef.current);
      }
    };
  }, []);

  const contentClassName = [
    "enumerator-content",
    standalone ? "subwindow-frame subwindow-frame-full enumerator-content-standalone enumerator-window" : "dialog-content subwindow-frame subwindow-frame-wide enumerator-window",
  ].filter(Boolean).join(" ");

  return (
    <div className={contentClassName}>
      <div className="subwindow-title-bar">
        <h3 className="subwindow-title">{t("library.enumeratorTitle")}</h3>
        {onClose ? <Button className="subwindow-close-button" type="button" aria-label={t("library.closeWindow")} onClick={onClose}>×</Button> : null}
      </div>

      <div className="subwindow-body enumerator-body">
        <aside className="enumerator-collections-panel">
          <div className="enumerator-toolbar-row">
            <Input
              className="enumerator-search-input"
              placeholder={t("library.searchCollections")}
              value={searchKeyword}
              onChange={(event) => setSearchKeyword(event.target.value)}
            />
            <Dropdown
              className="enumerator-filter-select"
              value={categoryFilter}
              ariaLabel={t("library.categoryFilter")}
              onChange={(next) => setCategoryFilter(next as CategoryFilter)}
              options={CATEGORY_FILTER_OPTIONS}
            />
          </div>

          <div className="enumerator-panel-note">
            {isLoadingCollections ? t("library.collectionsLoading") : t("library.enumeratorHint")}
          </div>

          <div className="enumerator-collection-table-wrap">
            <Table className="material-list-table enumerator-table">
              <TableHeader>
                <TableRow>
                  <TableHead>{t("library.name")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredCollections.map((collection) => {
                  const isSource = sourceCollectionId === collection.id;
                  const isEditing = editCollectionId === collection.id;
                  return (
                    <TableRow
                      key={collection.id}
                      className={isSource || isEditing ? "enumerator-row-selected" : ""}
                      onClick={() => handlePickSourceCollection(collection)}
                      onContextMenu={(event) => {
                        event.preventDefault();
                        handleLoadRightCollection(collection);
                      }}
                    >
                      <TableCell>
                        <div className="enumerator-collection-name">{collectionDisplayName(collection)}</div>
                        <div className="enumerator-collection-meta">
                          <span>{collectionSummary(collection)}</span>
                          {isSource ? <span>{t("library.leftPane")}</span> : null}
                          {isEditing ? <span>{t("library.rightPane")}</span> : null}
                          {collection.missing ? <span className="enumerator-warning-text">{t("library.missing")}</span> : null}
                        </div>
                      </TableCell>
                    </TableRow>
                  );
                })}
                {!filteredCollections.length ? (
                  <TableRow>
                    <TableCell className="enumerator-empty-cell">{t("library.noCategoryCollections")}</TableCell>
                  </TableRow>
                ) : null}
              </TableBody>
            </Table>
          </div>
        </aside>

        <section className="enumerator-editor-panel">
          <div className="enumerator-editor-grid">
            <section className="enumerator-source-column">
              <div className="enumerator-input-row">
                <label className="enumerator-input-label">{t("library.sourceCollection")}</label>
                <Input
                  className="enumerator-expression-input"
                  value={sourceExpression}
                  onChange={(event) => {
                    setSourceCollectionId("");
                    setSourceExpression(event.target.value);
                  }}
                  placeholder={t("library.collectionExpressionHint")}
                />
              </div>

              <label className="subwindow-check-row enumerator-subtract-toggle">
                <Checkbox checked={subtractRight} onCheckedChange={(checked) => setSubtractRight(checked === true)} />
                {t("library.subtractRight")}</label>

              <div className="enumerator-input-row enumerator-content-search-row">
                <label className="enumerator-input-label">{t("library.searchContent")}</label>
                <Input
                  className="enumerator-content-search-input"
                  value={leftSearchKeyword}
                  onChange={(event) => setLeftSearchKeyword(event.target.value)}
                  placeholder={t("library.searchLeftHint")}
                />
              </div>

              <div className="enumerator-value-table-wrap" ref={leftValueTableRef}>
                <Table className="material-list-table enumerator-table">
                  <TableHeader>
                    <TableRow>
                      <TableHead className="material-list-icon-col">{t("library.icon")}</TableHead>
                      <TableHead>{t("library.name")}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {!filteredLeftRows.length ? (
                      <TableRow>
                        <TableCell colSpan={2} className="enumerator-empty-cell">{t("library.noSourceData")}</TableCell>
                      </TableRow>
                    ) : (
                      <>
                        {leftTopSpacerHeight > 0 ? (
                          <TableRow className="material-list-virtual-spacer" aria-hidden>
                            <VirtualSpacerCell colSpan={2} height={leftTopSpacerHeight} />
                          </TableRow>
                        ) : null}
                        {visibleLeftRows.map((row, index) => (
                          <TableRow
                            key={row.id}
                            className={(leftStartIndex + index) % 2 === 0 ? "material-list-row-even" : "material-list-row-odd"}
                            onClick={(event) => handleCopyValue(row.id, event)}
                            onContextMenu={(event) => {
                              event.preventDefault();
                              handleAddValueToRight(row.id);
                            }}
                          >
                            <TableCell className="material-list-icon-cell"><BlockIcon blockId={row.iconHint} lookupMode={row.iconLookupMode} /></TableCell>
                            <TableCell>{row.name}</TableCell>
                          </TableRow>
                        ))}
                        {leftBottomSpacerHeight > 0 ? (
                          <TableRow className="material-list-virtual-spacer" aria-hidden>
                            <VirtualSpacerCell colSpan={2} height={leftBottomSpacerHeight} />
                          </TableRow>
                        ) : null}
                      </>
                    )}
                  </TableBody>
                </Table>
              </div>
            </section>

            <section className="enumerator-edit-column">
              <div className="enumerator-input-row">
                <label className="enumerator-input-label">{t("library.editCollectionName")}</label>
                <Input
                  className="enumerator-name-input"
                  value={editName}
                  onChange={(event) => setEditName(event.target.value)}
                  placeholder={t("library.editCollectionHint")}
                />
              </div>

              <div className="enumerator-actions-row">
                <Button type="button" onClick={() => handleMoveSelected(-1)} disabled={!rightSelected.length}>{t("library.moveUp")}</Button>
                <Button type="button" onClick={() => handleMoveSelected(1)} disabled={!rightSelected.length}>{t("library.moveDown")}</Button>
              </div>

              <div className="enumerator-input-row enumerator-content-search-row">
                <label className="enumerator-input-label">{t("library.searchContent")}</label>
                <Input
                  className="enumerator-content-search-input"
                  value={rightSearchKeyword}
                  onChange={(event) => setRightSearchKeyword(event.target.value)}
                  placeholder={t("library.searchRightHint")}
                />
              </div>

              <div className="enumerator-value-table-wrap" ref={rightValueTableRef}>
                <Table className="material-list-table enumerator-table">
                  <TableHeader>
                    <TableRow>
                      <TableHead className="material-list-icon-col">{t("library.icon")}</TableHead>
                      <TableHead>{t("library.name")}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {!filteredRightRows.length ? (
                      <TableRow>
                        <TableCell colSpan={2} className="enumerator-empty-cell">{t("library.rightCollectionEmpty")}</TableCell>
                      </TableRow>
                    ) : (
                      <>
                        {rightTopSpacerHeight > 0 ? (
                          <TableRow className="material-list-virtual-spacer" aria-hidden>
                            <VirtualSpacerCell colSpan={2} height={rightTopSpacerHeight} />
                          </TableRow>
                        ) : null}
                        {visibleRightRows.map((row, index) => {
                          const active = rightSelected.includes(row.id);
                          const rowClassName = [
                            (rightStartIndex + index) % 2 === 0 ? "material-list-row-even" : "material-list-row-odd",
                            active ? "enumerator-row-selected" : "",
                          ].filter(Boolean).join(" ");
                          return (
                            <TableRow
                              key={row.id}
                              className={rowClassName}
                              onClick={(event) => {
                                handleToggleRightSelection(row.id);
                                handleCopyValue(row.id, event);
                              }}
                              onContextMenu={(event) => {
                                event.preventDefault();
                                handleRemoveValueFromRight(row.id);
                              }}
                            >
                              <TableCell className="material-list-icon-cell"><BlockIcon blockId={row.iconHint} lookupMode={row.iconLookupMode} /></TableCell>
                              <TableCell>{row.name}</TableCell>
                            </TableRow>
                          );
                        })}
                        {rightBottomSpacerHeight > 0 ? (
                          <TableRow className="material-list-virtual-spacer" aria-hidden>
                            <VirtualSpacerCell colSpan={2} height={rightBottomSpacerHeight} />
                          </TableRow>
                        ) : null}
                      </>
                    )}
                  </TableBody>
                </Table>
              </div>
            </section>
          </div>
        </section>
      </div>

      <div className="enumerator-footer-row">
        <div className="enumerator-footer-status">
          <div>{localizeMessage(error || status || runtimeStatus || (currentFile ? displayFilePath(currentFile) : t("library.noCurrentSchematic")))}</div>
          <div className="enumerator-footer-meta">
            {editCollectionId
              ? t("library.editingCollectionCount", { count: rightValues.length })
              : t("library.draftCollectionCount", { count: rightValues.length })}
            {leftSearchKeyword || rightSearchKeyword
              ? t("library.collectionSearchCounts", { leftFiltered: filteredLeftRows.length, leftTotal: leftRows.length, rightFiltered: filteredRightRows.length, rightTotal: rightRows.length })
              : ""}
          </div>
        </div>
        <div className="enumerator-footer-actions">
          <Button type="button" onClick={handleCreateCollection}>{t("library.addCollection")}</Button>
          <Button type="button" onClick={() => persistCollection(true)} disabled={isSaving || !rightValues.length}>{t("library.saveCopy")}</Button>
          <Button type="button" onClick={() => persistCollection(false)} disabled={isSaving || !rightValues.length}>{t("library.saveCollection")}</Button>
        </div>
      </div>
      {copyToast ? (
        <div
          key={copyToast.id}
          className="material-list-hover-popup enumerator-copy-toast"
          role="status"
          style={{ left: copyToast.x, top: copyToast.y }}
        >
          <div className="material-list-hover-popup-row material-list-hover-popup-item-row">
            <span className="material-list-hover-popup-label">{t("library.copiedLabel")}</span>
            <span className="material-list-hover-popup-name">{copyToast.value}</span>
          </div>
        </div>
      ) : null}
    </div>
  );
}
