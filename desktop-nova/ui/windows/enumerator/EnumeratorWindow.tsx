import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent } from "react";

import {
  loadStructureStats,
  loadUserConfigMigratingLocalStorage,
  normalizeMaterialListWindowBehavior,
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
import { VirtualSpacerCell } from "../../components/VirtualSpacerCell";
import { useVirtualWindow } from "../../components/useVirtualWindow";
import {
  applyThemeStylesheet,
  currentThemeId,
  subscribeToThemeChanges,
  themeClassName,
} from "../../shell/themeRuntime";

const enumeratorOpenFileEvent = "enumerator-open-file";

type CategoryFilter = "all" | "base" | "version" | "system_enum" | "creative" | "custom";
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
      return "全集";
    case "version":
      return "版本";
    case "creative":
      return "创造模式";
    case "system_enum":
      return "系统枚举";
    case "custom":
      return "自定义";
    case "runtime":
      return "运行时";
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

function collectionSummary(collection: EnumeratorCollection): string {
  return `${categoryLabel(collection.category)} · ${collection.values.length} 项`;
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
  const info = await loadUserConfigMigratingLocalStorage().catch(() => null);
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
  return (
    <div className={["dialog-overlay", themeClassName(theme)].filter(Boolean).join(" ")}>
      <EnumeratorContent currentFile={currentFile} runtimeCollection={runtimeCollection} onClose={onClose} />
    </div>
  );
}

export function EnumeratorWindow() {
  const [themeId, setThemeId] = useState(currentThemeId());
  const [currentFile, setCurrentFile] = useState(initialFileFromUrl());
  const [runtimeCollection, setRuntimeCollection] = useState<EnumeratorCollection | null>(null);
  const [runtimeStatus, setRuntimeStatus] = useState("");

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
    setRuntimeStatus("正在从当前投影生成运行时集合...");
    loadStructureStats(currentFile)
      .then((stats) => {
        if (cancelled) return;
        setRuntimeCollection(createRuntimeProjectionCollection(currentFile, stats.materials));
        setRuntimeStatus(`已载入当前投影的 ${stats.materials.length} 个材料项。`);
      })
      .catch((error) => {
        if (cancelled) return;
        setRuntimeCollection(null);
        setRuntimeStatus(`读取当前投影失败：${String(error)}`);
      });
    return () => {
      cancelled = true;
    };
  }, [currentFile]);

  return (
    <main className={["subwindow-standalone-page", "enumerator-window", themeClassName(themeId)].filter(Boolean).join(" ")}>
      <EnumeratorContent currentFile={currentFile} runtimeCollection={runtimeCollection} runtimeStatus={runtimeStatus} standalone />
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
  runtimeStatus?: string;
  onClose?: () => void;
  standalone?: boolean;
}) {
  const [collections, setCollections] = useState<EnumeratorCollection[]>([]);
  const [isLoadingCollections, setIsLoadingCollections] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
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
      setError(`读取枚举器集合失败：${String(err)}`);
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
      return [collection.name, collection.id]
        .filter(Boolean)
        .some((value) => value.toLowerCase().includes(keyword));
    });
  }, [categoryFilter, collections, searchKeyword]);

  const expressionResult = useMemo(
    () => evaluateEnumeratorExpression(sourceExpression, collections),
    [collections, sourceExpression],
  );

  const sourceValues = useMemo(() => {
    if (!subtractRight) return expressionResult.values;
    const currentRight = new Set(rightValues);
    return expressionResult.values.filter((value) => !currentRight.has(value));
  }, [expressionResult.values, rightValues, subtractRight]);

  const leftRows = useMemo(() => buildEnumeratorValueRows(sourceValues, collections), [collections, sourceValues]);
  const rightRows = useMemo(() => buildEnumeratorValueRows(rightValues, collections), [collections, rightValues]);

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
  const {
    visibleItems: visibleLeftRows,
    startIndex: leftStartIndex,
    topSpacerHeight: leftTopSpacerHeight,
    bottomSpacerHeight: leftBottomSpacerHeight,
  } = useVirtualWindow(filteredLeftRows, leftValueTableRef, 40, 10);
  const {
    visibleItems: visibleRightRows,
    startIndex: rightStartIndex,
    topSpacerHeight: rightTopSpacerHeight,
    bottomSpacerHeight: rightBottomSpacerHeight,
  } = useVirtualWindow(filteredRightRows, rightValueTableRef, 40, 10);

  const handleCreateCollection = () => {
    hydrateRightEditor(null);
    setStatus("已创建一个未保存的右侧集合草稿。");
  };

  const handlePickSourceCollection = (collection: EnumeratorCollection) => {
    setSourceCollectionId(collection.id);
    setSourceExpression(collection.name);
    setStatus(`已将 ${collection.name} 放入左侧来源列。`);
  };

  const handleLoadRightCollection = (collection: EnumeratorCollection) => {
    hydrateRightEditor(collection);
    setStatus(collection.readOnly ? `已将 ${collection.name} 放入右侧，可另存为自定义集合。` : `已将 ${collection.name} 放入右侧进行编辑。`);
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
      setError(`复制失败：${String(err?.message || err)}`);
    }
  };

  const handleMoveSelected = (direction: -1 | 1) => {
    setRightValues((current) => reorderValues(current, rightSelected, direction));
  };

  const persistCollection = async (asCopy: boolean) => {
    const name = editName.trim() || (editingCollection?.name || "新建集合");
    const inputName = asCopy ? `${name} 副本` : name;
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
      setStatus(asCopy ? `已保存集合副本：${saved.name}` : `已保存集合：${saved.name}`);
      setEditCollectionId(saved.id);
      await reloadCollections(saved.id);
    } catch (err: any) {
      setError(`保存集合失败：${String(err)}`);
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
        <h3 className="subwindow-title">枚举器</h3>
        {onClose ? <button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={onClose}>×</button> : null}
      </div>

      <div className="subwindow-body enumerator-body">
        <aside className="enumerator-collections-panel">
          <div className="enumerator-toolbar-row">
            <input
              className="input enumerator-search-input"
              placeholder="搜索集合"
              value={searchKeyword}
              onChange={(event) => setSearchKeyword(event.target.value)}
            />
            <select className="input enumerator-filter-select" value={categoryFilter} onChange={(event) => setCategoryFilter(event.target.value as CategoryFilter)}>
              <option value="all">所有分类</option>
              <option value="base">全集</option>
              <option value="version">版本</option>
              <option value="creative">创造模式</option>
              <option value="system_enum">系统枚举</option>
              <option value="custom">自定义</option>
            </select>
          </div>

          <div className="enumerator-panel-note">
            {isLoadingCollections ? "正在加载集合..." : "左键放入左侧来源，右键放入右侧编辑。版本/创造模式/系统/自定义集合均从用户配置目录 enumerator/ 对应子目录读取。"}
          </div>

          <div className="enumerator-collection-table-wrap">
            <table className="material-list-table enumerator-table">
              <thead>
                <tr>
                  <th>名称</th>
                </tr>
              </thead>
              <tbody>
                {filteredCollections.map((collection) => {
                  const isSource = sourceCollectionId === collection.id;
                  const isEditing = editCollectionId === collection.id;
                  return (
                    <tr
                      key={collection.id}
                      className={isSource || isEditing ? "enumerator-row-selected" : ""}
                      onClick={() => handlePickSourceCollection(collection)}
                      onContextMenu={(event) => {
                        event.preventDefault();
                        handleLoadRightCollection(collection);
                      }}
                    >
                      <td>
                        <div className="enumerator-collection-name">{collection.name}</div>
                        <div className="enumerator-collection-meta">
                          <span>{collectionSummary(collection)}</span>
                          {isSource ? <span>左侧</span> : null}
                          {isEditing ? <span>右侧</span> : null}
                          {collection.missing ? <span className="enumerator-warning-text">缺失</span> : null}
                        </div>
                      </td>
                    </tr>
                  );
                })}
                {!filteredCollections.length ? (
                  <tr>
                    <td className="enumerator-empty-cell">当前分类暂无集合</td>
                  </tr>
                ) : null}
              </tbody>
            </table>
          </div>
        </aside>

        <section className="enumerator-editor-panel">
          <div className="enumerator-editor-grid">
            <section className="enumerator-source-column">
              <div className="enumerator-input-row">
                <label className="enumerator-input-label">来源集合</label>
                <input
                  className="input enumerator-expression-input"
                  value={sourceExpression}
                  onChange={(event) => {
                    setSourceCollectionId("");
                    setSourceExpression(event.target.value);
                  }}
                  placeholder="例：A-(map_*)；或左键点击集合填入"
                />
              </div>

              <label className="subwindow-check-row enumerator-subtract-toggle">
                <input type="checkbox" checked={subtractRight} onChange={(event) => setSubtractRight(event.target.checked)} />
                减去右侧
              </label>

              <div className="enumerator-input-row enumerator-content-search-row">
                <label className="enumerator-input-label">搜索内容</label>
                <input
                  className="input enumerator-content-search-input"
                  value={leftSearchKeyword}
                  onChange={(event) => setLeftSearchKeyword(event.target.value)}
                  placeholder="按名称或 ID 搜索左侧内容"
                />
              </div>

              <div className="enumerator-value-table-wrap" ref={leftValueTableRef}>
                <table className="material-list-table enumerator-table">
                  <thead>
                    <tr>
                      <th className="material-list-icon-col">图标</th>
                      <th>名称</th>
                    </tr>
                  </thead>
                  <tbody>
                    {!filteredLeftRows.length ? (
                      <tr>
                        <td colSpan={2} className="enumerator-empty-cell">无来源数据</td>
                      </tr>
                    ) : (
                      <>
                        {leftTopSpacerHeight > 0 ? (
                          <tr className="material-list-virtual-spacer" aria-hidden>
                            <VirtualSpacerCell colSpan={2} height={leftTopSpacerHeight} />
                          </tr>
                        ) : null}
                        {visibleLeftRows.map((row, index) => (
                          <tr
                            key={row.id}
                            className={(leftStartIndex + index) % 2 === 0 ? "material-list-row-even" : "material-list-row-odd"}
                            onClick={(event) => handleCopyValue(row.id, event)}
                            onContextMenu={(event) => {
                              event.preventDefault();
                              handleAddValueToRight(row.id);
                            }}
                          >
                            <td className="material-list-icon-cell"><BlockIcon blockId={row.iconHint} lookupMode={row.iconLookupMode} /></td>
                            <td>{row.name}</td>
                          </tr>
                        ))}
                        {leftBottomSpacerHeight > 0 ? (
                          <tr className="material-list-virtual-spacer" aria-hidden>
                            <VirtualSpacerCell colSpan={2} height={leftBottomSpacerHeight} />
                          </tr>
                        ) : null}
                      </>
                    )}
                  </tbody>
                </table>
              </div>
            </section>

            <section className="enumerator-edit-column">
              <div className="enumerator-input-row">
                <label className="enumerator-input-label">编辑集合名称</label>
                <input
                  className="input enumerator-name-input"
                  value={editName}
                  onChange={(event) => setEditName(event.target.value)}
                  placeholder="右键集合载入右侧，或新建集合"
                />
              </div>

              <div className="enumerator-actions-row">
                <button className="btn" type="button" onClick={() => handleMoveSelected(-1)} disabled={!rightSelected.length}>上移</button>
                <button className="btn" type="button" onClick={() => handleMoveSelected(1)} disabled={!rightSelected.length}>下移</button>
              </div>

              <div className="enumerator-input-row enumerator-content-search-row">
                <label className="enumerator-input-label">搜索内容</label>
                <input
                  className="input enumerator-content-search-input"
                  value={rightSearchKeyword}
                  onChange={(event) => setRightSearchKeyword(event.target.value)}
                  placeholder="按名称或 ID 搜索右侧内容"
                />
              </div>

              <div className="enumerator-value-table-wrap" ref={rightValueTableRef}>
                <table className="material-list-table enumerator-table">
                  <thead>
                    <tr>
                      <th className="material-list-icon-col">图标</th>
                      <th>名称</th>
                    </tr>
                  </thead>
                  <tbody>
                    {!filteredRightRows.length ? (
                      <tr>
                        <td colSpan={2} className="enumerator-empty-cell">当前右侧集合为空</td>
                      </tr>
                    ) : (
                      <>
                        {rightTopSpacerHeight > 0 ? (
                          <tr className="material-list-virtual-spacer" aria-hidden>
                            <VirtualSpacerCell colSpan={2} height={rightTopSpacerHeight} />
                          </tr>
                        ) : null}
                        {visibleRightRows.map((row, index) => {
                          const active = rightSelected.includes(row.id);
                          const rowClassName = [
                            (rightStartIndex + index) % 2 === 0 ? "material-list-row-even" : "material-list-row-odd",
                            active ? "enumerator-row-selected" : "",
                          ].filter(Boolean).join(" ");
                          return (
                            <tr
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
                              <td className="material-list-icon-cell"><BlockIcon blockId={row.iconHint} lookupMode={row.iconLookupMode} /></td>
                              <td>{row.name}</td>
                            </tr>
                          );
                        })}
                        {rightBottomSpacerHeight > 0 ? (
                          <tr className="material-list-virtual-spacer" aria-hidden>
                            <VirtualSpacerCell colSpan={2} height={rightBottomSpacerHeight} />
                          </tr>
                        ) : null}
                      </>
                    )}
                  </tbody>
                </table>
              </div>
            </section>
          </div>
        </section>
      </div>

      <div className="enumerator-footer-row">
        <div className="enumerator-footer-status">
          <div>{error || status || runtimeStatus || (currentFile ? currentFile : "未关联当前投影文件")}</div>
          <div className="enumerator-footer-meta">
            {editCollectionId
              ? `正在编辑现有自定义集合，共 ${rightValues.length} 项。`
              : `当前为右侧草稿，共 ${rightValues.length} 项。`}
            {leftSearchKeyword || rightSearchKeyword
              ? ` 左侧搜索结果 ${filteredLeftRows.length} / ${leftRows.length}，右侧搜索结果 ${filteredRightRows.length} / ${rightRows.length}。`
              : ""}
          </div>
        </div>
        <div className="enumerator-footer-actions">
          <button className="btn" type="button" onClick={handleCreateCollection}>新增集合</button>
          <button className="btn" type="button" onClick={() => persistCollection(true)} disabled={isSaving || !rightValues.length}>保存副本</button>
          <button className="btn" type="button" onClick={() => persistCollection(false)} disabled={isSaving || !rightValues.length}>保存集合</button>
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
            <span className="material-list-hover-popup-label">已复制：</span>
            <span className="material-list-hover-popup-name">{copyToast.value}</span>
          </div>
        </div>
      ) : null}
    </div>
  );
}
