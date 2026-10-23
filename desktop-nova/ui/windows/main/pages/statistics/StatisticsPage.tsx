import { t, getUiLocale, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { displayFilePath } from "../../../../../src/business/facade";
import { Checkbox } from "../../../../components/ui/checkbox";
import { TableHeader, TableRow, TableHead, TableBody, TableCell, Table } from "../../../../components/ui/table";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Textarea } from "../../../../components/ui/textarea";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Dialog } from "../../../../components/Dialog";
import { X } from "lucide-react";
﻿import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  exportMaterialListWithOptions,
  fetchMaterialRecipeCache,
  formatMaterialQuantity,
  hasInventoryMaterialSemantics,
  filterMaterialItems,
  materialDisplayName,
  getI18nRevision,
  subscribeI18nChanges,
  loadStructureStats,
  loadMaterialsScope,
  DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES,
  createRuntimeProjectionCollection,
  evaluateEnumeratorExpression,
  loadEnumeratorCollections,
  normalizeStatisticsEnumeratorInfoRules,
  saveStatisticsEnumeratorInfoRulesConfig,
  type EnumeratorCollection,
  mapStructureStats,
  recordStatisticsCommit,
  findRenderOperationForSource,
  getApplicationRenderState,
  isApplicationRenderPending,
  subscribeApplicationRender,
  MaterialItem,
  StatsData,
  getStockpileTask,
  setStockpileTaskQuantity,
  resetStockpileTask,
  prepareMaterialTaskWindow,
  type StockpileTaskView,
  type ApplicationMaterialScope,
} from "../../../../../src/business/facade";
import { listenEvent } from "../../../../../src/platform/events";
import { useSyncExternalStore } from "react";
import {
  loadUserConfigMigratingLocalStorage,
  normalizeMaterialListWindowBehavior,
  openMaterialListWindow,
} from "../../../../../src/business/facade";
import { VirtualSpacerCell, VirtualSpacerItem } from "../../../../components/VirtualSpacerCell";
import { useVirtualWindow } from "../../../../components/useVirtualWindow";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Dropdown } from "../../../../components/Dropdown";
import "./materials.css";
import "../../../../styles/statistics-enumerator.css";
import { EnumeratorDialog, openEnumeratorWithWindowBehavior } from "../../../enumerator";

interface EnumeratorInfoRule {
  title: string;
  expression: string;
  raw: string;
}

interface EnumeratorInfoResult {
  title: string;
  value: string;
  error: string;
  raw: string;
}

function parseEnumeratorInfoRules(rules: string[]): EnumeratorInfoRule[] {
  return normalizeStatisticsEnumeratorInfoRules(rules)
    .map((raw) => {
      const separatorIndex = raw.indexOf("=");
      if (separatorIndex <= 0 || separatorIndex === raw.length - 1) {
        return {
          title: raw.trim() || t("statistics.unnamed"),
          expression: "",
          raw,
        };
      }
      return {
        title: raw === DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES[0] ? t("statistics.defaultVersion") : raw === DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES[1] ? t("statistics.defaultSurvival") : raw.slice(0, separatorIndex).trim() || t("statistics.unnamed"),
        expression: raw.slice(separatorIndex + 1).trim(),
        raw,
      };
    })
    .filter((rule) => rule.title || rule.expression);
}

function parseFunctionCall(expression: string): { name: string; argument: string } | null {
  const trimmed = String(expression || "").trim();
  const openIndex = trimmed.indexOf("(");
  if (openIndex <= 0 || !trimmed.endsWith(")")) return null;
  return {
    name: trimmed.slice(0, openIndex).trim().toLowerCase(),
    argument: trimmed.slice(openIndex + 1, -1).trim(),
  };
}

function uniqValues(values: string[]): string[] {
  return [...new Set(values.map((value) => String(value || "").trim()).filter(Boolean))];
}

function buildBaseUniverse(collections: EnumeratorCollection[]): Set<string> {
  return new Set(
    collections
      .filter((collection) => collection.category === "base")
      .flatMap((collection) => collection.values),
  );
}

function buildVersionUniverse(collections: EnumeratorCollection[]): Set<string> {
  return new Set(
    collections
      .filter((collection) => collection.category === "version")
      .flatMap((collection) => collection.values),
  );
}

function parseVersionToken(collection: EnumeratorCollection): { kind: "older" | "normal"; numbers: number[]; label: string } | null {
  const candidates = [collection.version, collection.name, collection.id];
  for (const candidate of candidates) {
    const normalized = String(candidate || "").trim();
    if (!normalized) continue;
    if (normalized === "v_older") {
      return { kind: "older", numbers: [1, 12, 2], label: t("statistics.before1122") };
    }
    if (!normalized.startsWith("v_")) continue;
    const parts = normalized
      .slice(2)
      .split("_")
      .map((part) => Number.parseInt(part, 10));
    if (!parts.length || parts.some((part) => !Number.isFinite(part))) continue;
    return {
      kind: "normal",
      numbers: parts,
      label: parts.join("."),
    };
  }
  return null;
}

function compareVersionCollections(left: EnumeratorCollection, right: EnumeratorCollection): number {
  const leftToken = parseVersionToken(left);
  const rightToken = parseVersionToken(right);
  if (!leftToken && !rightToken) return left.name.localeCompare(right.name, getUiLocale());
  if (!leftToken) return -1;
  if (!rightToken) return 1;
  if (leftToken.kind !== rightToken.kind) {
    return leftToken.kind === "older" ? -1 : 1;
  }
  const size = Math.max(leftToken.numbers.length, rightToken.numbers.length, 3);
  for (let index = 0; index < size; index += 1) {
    const diff = (leftToken.numbers[index] ?? 0) - (rightToken.numbers[index] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

function formatVersionResult(values: string[], collections: EnumeratorCollection[]): string {
  if (values.length === 0) return "--";
  const targetSet = new Set(values);
  const baseUniverse = buildBaseUniverse(collections);
  const versionCollections = collections.filter((collection) => collection.category === "version");
  const validVersionCollections = versionCollections.filter((collection) => Boolean(parseVersionToken(collection)));
  const versionUniverse = buildVersionUniverse(collections);
  const matchedVersions = versionCollections.filter((collection) => collection.values.some((value) => targetSet.has(value)));
  const hasHigherVersion = [...targetSet].some((value) => baseUniverse.has(value) && !versionUniverse.has(value));
  const sortedMatchedVersions = matchedVersions.sort(compareVersionCollections);
  const latestVersion = sortedMatchedVersions.length > 0 ? sortedMatchedVersions[sortedMatchedVersions.length - 1] : null;
  if (!latestVersion) {
    if (validVersionCollections.length === 0) {
      return t("statistics.noVersionSets");
    }
    return hasHigherVersion ? t("statistics.higherVersion") : "--";
  }
  const versionToken = parseVersionToken(latestVersion);
  const label = versionToken?.label || latestVersion.name;
  return hasHigherVersion ? t("statistics.withHigherVersion", {version:label}) : label;
}

function evaluateEnumeratorInfoRule(rule: EnumeratorInfoRule, data: StatsData, collections: EnumeratorCollection[]): EnumeratorInfoResult {
  if (!rule.expression) {
    return { title: rule.title, value: "", error: t("statistics.missingExpression"), raw: rule.raw };
  }
  if (data && !data.hasStructuralBlockCounts) return { title: rule.title, value: "", error: t("statistics.missingStructure"), raw:rule.raw };
  const call = parseFunctionCall(rule.expression);
  if (!call) {
    return { title: rule.title, value: "", error: t("statistics.expressionForm"), raw: rule.raw };
  }
  const evaluated = evaluateEnumeratorExpression(call.argument, collections);
  if (evaluated.error) {
    return { title: rule.title, value: "", error: evaluated.error, raw: rule.raw };
  }
  const values = uniqValues(evaluated.values);
  switch (call.name) {
    case "version":
      return { title: rule.title, value: formatVersionResult(values, collections), error: "", raw: rule.raw };
    case "any":
      return { title: rule.title, value: values.length > 0 ? t("statistics.yes") : t("statistics.no"), error: "", raw: rule.raw };
    case "ratio": {
      if (data.totalNonAirBlocks <= 0) {
        return { title: rule.title, value: "--", error: "", raw: rule.raw };
      }
      const countById = new Map(data.structuralBlocks.map((block) => [block.id, block.count]));
      const numerator = values.reduce((sum, value) => sum + (countById.get(value) || 0), 0);
      return {
        title: rule.title,
        value: `${((numerator / data.totalNonAirBlocks) * 100).toLocaleString(getUiLocale(), {minimumFractionDigits:1, maximumFractionDigits:1})}%`,
        error: "",
        raw: rule.raw,
      };
    }
    default:
      return { title: rule.title, value: "", error: t("statistics.unsupportedFunction", {name:call.name}), raw: rule.raw };
  }
}

export function buildEnumeratorInfoResults(rules: string[], data: StatsData | null, collections: EnumeratorCollection[]): EnumeratorInfoResult[] {
  const parsedRules = parseEnumeratorInfoRules(rules);
  if (!data) {
    return parsedRules.map((rule) => ({ title: rule.title, value: t("statistics.waiting"), error: "", raw: rule.raw }));
  }
  return parsedRules.map((rule) => evaluateEnumeratorInfoRule(rule, data, collections));
}

function MaterialTooltip({ x, y, item, multiplier }: { x: number; y: number; item: MaterialItem | null; multiplier: number }) {
  const { locale } = useI18n();
  const popupRef = useRef<HTMLDivElement | null>(null);
  const total = item ? Math.max(0, Math.floor(item.totalCount * multiplier)) : 0;

  const [position, setPosition] = useState(() => ({ left: x + 14, top: y + 14 }));

  useLayoutEffect(() => {
    if (!item) return;
    const popup = popupRef.current;
    const offset = 14;
    const margin = 4;
    if (!popup) {
      setPosition({ left: x + offset, top: y + offset });
      return;
    }

    const rect = popup.getBoundingClientRect();
    let left = x + offset;
    let top = y + offset;

    if (left + rect.width > window.innerWidth - margin) {
      left = x - rect.width - offset;
    }
    if (top + rect.height > window.innerHeight - margin) {
      top = y - rect.height - offset;
    }

    left = Math.max(margin, Math.min(left, window.innerWidth - rect.width - margin));
    top = Math.max(margin, Math.min(top, window.innerHeight - rect.height - margin));
    setPosition((previous) => previous.left === left && previous.top === top ? previous : { left, top });
  }, [x, y, item, total, locale]);

  useLayoutEffect(() => {
    const popup = popupRef.current;
    if (!popup) return;
    popup.style.left = `${position.left}px`;
    popup.style.top = `${position.top}px`;
  }, [position.left, position.top]);

  if (!item) return null;

  return (
    <div
      ref={popupRef}
      className="material-list-hover-popup"
      role="tooltip"
    >
      <div className="material-list-hover-popup-row material-list-hover-popup-item-row">
        <span className="material-list-hover-popup-label">{t("statistics.itemLabel")}</span>
        <BlockIcon blockId={item.iconHint} />
        <span className="material-list-hover-popup-name">{materialDisplayName(item)}</span>
      </div>
      <div className="material-list-hover-popup-row">ID：{item.id}</div>
      <div className="material-list-hover-popup-row">
        {t("statistics.totalLabel")}<strong>{total.toLocaleString(getUiLocale())}</strong> · {formatMaterialQuantity(item, total)}
      </div>
      <div className="material-list-hover-popup-row">
        {item.materialStatus !== "item" ? t("statistics.nonPlaceable") : item.hasCustomStackSize ? t("statistics.customStack") : !item.canFitContainer ? t("statistics.cannotFit") : t("statistics.stackInfo", {size:item.stackSize})}
      </div>
    </div>
  );
}

function EnumeratorInfoEditorDialog({
  rules,
  data,
  collections,
  onClose,
  onSave,
}: {
  rules: string[];
  data: StatsData | null;
  collections: EnumeratorCollection[];
  onClose: () => void;
  onSave: (rules: string[]) => Promise<void>;
}) {
  const { locale } = useI18n();
  const [draft, setDraft] = useState(rules.join("\n"));
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState<UiMessage>("");

  useEffect(() => {
    setDraft(rules.join("\n"));
  }, [rules]);

  const parsedDraftRules = useMemo(
    () => normalizeStatisticsEnumeratorInfoRules(draft.split(/\r?\n/)),
    [draft],
  );

  const previewResults = useMemo(
    () => buildEnumeratorInfoResults(parsedDraftRules, data, collections),
    [collections, data, parsedDraftRules, locale],
  );

  const hasErrors = previewResults.some((item) => Boolean(item.error));

  const handleSave = async () => {
    setIsSaving(true);
    setSaveError("");
    try {
      await onSave(parsedDraftRules);
      onClose();
    } catch (error: any) {
      setSaveError(error?.message || error ? String(error?.message || error) : {key:"editor.saveFailed"});
    } finally {
      setIsSaving(false);
    }
  };

  return <Dialog title={t("statistics.customTitle")} subtitle={t("statistics.customSubtitle")} onClose={onClose} busy={isSaving} width="lg" footer={<>
    <Button variant="secondary" disabled={isSaving} onClick={() => setDraft(DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES.join("\n"))}>{t("statistics.restoreDefaults")}</Button>
    <Button variant="secondary" disabled={isSaving} onClick={onClose}>{t("editor.cancel")}</Button>
    <Button disabled={isSaving || hasErrors} onClick={handleSave}>{isSaving ? t("editor.saving") : t("editor.save")}</Button>
  </>}>
    <div className="statistics-enumerator-editor-columns">
      <div className="statistics-enumerator-editor-column">
        <label className="statistics-enumerator-editor-heading" htmlFor="statistics-rules">{t("statistics.rules")}</label>
        <Textarea id="statistics-rules" className="statistics-enumerator-editor-textarea" value={draft} onChange={event => setDraft(event.target.value)} spellCheck={false} disabled={isSaving} />
        <div className="statistics-enumerator-editor-hint">{t("statistics.rulesHint")}</div>
      </div>
      <div className="statistics-enumerator-editor-column"><h3 className="statistics-enumerator-editor-heading">{t("statistics.livePreview")}</h3>
        {previewResults.map((item,index) => <div key={index} className="statistics-enumerator-preview-item">
          <div className="statistics-enumerator-preview-main"><span>{item.title}</span><strong>{item.error ? t("editor.error") : item.value}</strong></div>
          {item.error && <p className="statistics-enumerator-preview-error" role="alert">{localizeMessage(item.error)}</p>}
        </div>)}
      </div>
    </div>
    {saveError && <pre className="subwindow-error" role="alert">{localizeMessage(saveError)}</pre>}
    {hasErrors && <p className="statistics-enumerator-editor-error">{t("statistics.fixRules")}</p>}
  </Dialog>;
}

/**
 * Opens the material list according to the user's configured window behavior.
 */
export async function openMaterialsWithWindowBehavior(currentFile: string, showOverlay: () => void, viewMode: "details" | "export" = "details"): Promise<void> {
  await prepareMaterialTaskWindow();
  const info = await loadUserConfigMigratingLocalStorage().catch(() => null);
  const behavior = normalizeMaterialListWindowBehavior(info?.config.material_list_window_behavior);
  if (behavior === "independent_window") {
    try {
      await openMaterialListWindow(currentFile,viewMode);
      return;
    } catch {
      // Browser preview cannot create a desktop window, so keep the in-window dialog as fallback.
    }
  }
  showOverlay();
}

const MaterialTable = React.memo(function MaterialTable({ materials, asItems, multiplier = 1, breakdown = false, onHover }: {
  materials: MaterialItem[];
  asItems: boolean;
  multiplier?: number;
  breakdown?: boolean;
  onHover?: (material: MaterialItem | null) => void;
}) {
  const { locale } = useI18n();
  const scrollRef = useRef<HTMLDivElement | null>(null);
  // Must be the height the browser actually lays out (see materials.css): detail rows are a
  // 32px icon plus 9px vertical padding plus the collapsed border; summary rows are pinned to
  // 48px. A round number here would make the spacer disagree with the rendered rows.
  const virtual = useVirtualWindow(materials,scrollRef,breakdown ? 51 : 48,8);
  useLayoutEffect(() => { if(scrollRef.current) scrollRef.current.scrollTop = 0; },[materials]);
  // Zebra striping is keyed on the row's position in the *data*, never on its DOM slot: the
  // virtual window slides, so a `:nth-child` stripe would invert on every scrolled row.
  const rowParityClass = (absoluteIndex: number) => (absoluteIndex % 2 === 1 ? "material-list-row-odd" : "material-list-row-even");
  if (!breakdown) return <div className="material-table-scroll material-summary-scroll" ref={scrollRef}><ul className="material-summary-list" aria-label={t("editor.materials")}>
    {virtual.topSpacerHeight > 0 && <VirtualSpacerItem className="material-virtual-spacer" height={virtual.topSpacerHeight} /> }
    {virtual.visibleItems.map((material, index) => <li key={material.id} className={rowParityClass(virtual.startIndex + index)} onMouseEnter={() => onHover?.(material)} onMouseLeave={() => onHover?.(null)}>
      <BlockIcon blockId={material.iconHint} />
      <span className="material-summary-name" title={material.id}>{materialDisplayName(material)}
        {material.materialStatus !== "item" && <small>{material.materialStatus === "no_item" ? t("statistics.noItem") : t("statistics.unknownItem")}</small>}
      </span>
      <span className="material-summary-quantity">{formatMaterialQuantity(material, material.totalCount * multiplier, asItems)}</span>
    </li>)}
    {virtual.bottomSpacerHeight > 0 && <VirtualSpacerItem className="material-virtual-spacer" height={virtual.bottomSpacerHeight} /> }
    {materials.length === 0 && <li className="material-summary-empty">{t("statistics.noMatches")}</li>}
  </ul></div>;
  return <div className="material-table-scroll material-detail-table-scroll" ref={scrollRef}><Table className="material-list-table beta-material-table">
    <TableHeader><TableRow><TableHead scope="col">{t("statistics.material")}</TableHead>{breakdown && <><TableHead scope="col">{t("statistics.buildQuantity")}</TableHead><TableHead scope="col">{t("statistics.containerItems")}</TableHead></>}<TableHead scope="col">{breakdown ? t("statistics.total") : t("statistics.quantity")}</TableHead></TableRow></TableHeader>
    <TableBody>{virtual.topSpacerHeight > 0 && <TableRow aria-hidden><VirtualSpacerCell colSpan={4} height={virtual.topSpacerHeight} /></TableRow>}{virtual.visibleItems.map((material, index) => <TableRow key={material.id} className={rowParityClass(virtual.startIndex + index)} onMouseEnter={() => onHover?.(material)} onMouseLeave={() => onHover?.(null)}>
      <TableCell><div className="beta-material-name"><BlockIcon blockId={material.iconHint} /><span title={material.id}>{materialDisplayName(material)}{material.materialStatus !== "item" && <small>{material.materialStatus === "no_item" ? t("statistics.noItem") : t("statistics.unknownItem")}</small>}</span></div></TableCell>
      {breakdown && <><TableCell className="beta-material-quantity">{formatMaterialQuantity(material, material.blockCount * multiplier, asItems)}</TableCell><TableCell className="beta-material-quantity">{formatMaterialQuantity(material, material.containerItemCount * multiplier, asItems)}</TableCell></>}
      <TableCell className="beta-material-quantity">{formatMaterialQuantity(material, material.totalCount * multiplier, asItems)}</TableCell>
    </TableRow>)}{virtual.bottomSpacerHeight > 0 && <TableRow aria-hidden><VirtualSpacerCell colSpan={4} height={virtual.bottomSpacerHeight} /></TableRow>}{materials.length === 0 && <TableRow><TableCell colSpan={breakdown ? 4 : 2} className="material-list-empty-cell">{t("statistics.noMatches")}</TableCell></TableRow>}</TableBody>
  </Table></div>;
});

function QuantitySwitch({ checked, onChange }: { checked: boolean; onChange: (value: boolean) => void }) {
  useI18n();
  return <label className="beta-material-check"><Checkbox  checked={checked} onCheckedChange={checked => onChange((checked === true))} />{t("statistics.individual")}</label>;
}

type StockpileWriteQueue = {
  epoch: number;
  view: StockpileTaskView;
  pending: Map<string, number>;
  running: boolean;
  reset: boolean;
  refresh: boolean;
};

function StockpileQuantityInput({ quantity, required, label, disabled, onQuantity }: {
  quantity: number; required: number; label: string; disabled: boolean; onQuantity: (quantity: number) => void;
}) {
  const [text, setText] = useState(String(quantity));
  const focused = useRef(false);
  const submitted = useRef(quantity);
  useEffect(() => {
    if (!focused.current) { setText(String(quantity)); submitted.current = quantity; }
  }, [quantity]);
  return <Input className="stockpile-quantity-input" type="number" inputMode="numeric" min={0} max={required} step={1}
    aria-label={label} disabled={disabled} value={text}
    onFocus={() => { focused.current = true; }}
    onChange={event => {
      const raw = event.target.value;
      setText(raw);
      if (!/^\d+$/.test(raw)) return;
      const value = Math.min(required, Math.max(0, Math.trunc(Number(raw))));
      if (!Number.isSafeInteger(value)) return;
      if (value !== Number(raw)) setText(String(value));
      submitted.current = value;
      onQuantity(value);
    }}
    onBlur={() => { focused.current = false; setText(String(submitted.current)); }}
    onKeyDown={event => {
      if (event.key === "Enter") event.currentTarget.blur();
      if (event.key === "Escape") { event.stopPropagation(); event.currentTarget.blur(); }
    }} />;
}

function StockpileTaskList({ sourcePath, scope, includeContainerItems, multiplier, materials, asItems, search, onSearch, loadingMaterials, onHover, onPendingChange }: {
  sourcePath: string; scope: ApplicationMaterialScope; includeContainerItems: boolean; multiplier: number;
  materials: MaterialItem[]; asItems: boolean; search: string; onSearch: (value: string) => void;
  loadingMaterials: boolean; onHover: (material: MaterialItem | null) => void;
  onPendingChange: (pending: boolean) => void;
}) {
  const { locale } = useI18n();
  const [view, setView] = useState<StockpileTaskView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [pending, setPending] = useState(0);
  useEffect(() => { onPendingChange(pending > 0); }, [pending, onPendingChange]);
  const [filter, setFilter] = useState("all");
  const [resetOpen, setResetOpen] = useState(false);
  const epoch = useRef(0);
  const requestSequence = useRef(0);
  const queue = useRef<StockpileWriteQueue | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const input = useMemo(() => ({ sourcePath, scope, includeContainerItems, multiplier }), [sourcePath, scope, includeContainerItems, multiplier]);
  const inputRef = useRef(input);
  inputRef.current = input;
  const loadRef = useRef<() => void>(() => {});
  const load = async (expected = epoch.current, showLoading = false) => {
    const sequence = ++requestSequence.current;
    if (showLoading) setLoading(true);
    setError("");
    try {
      const next = await getStockpileTask(inputRef.current);
      if (expected !== epoch.current || sequence !== requestSequence.current) return;
      setView(next);
      if (!queue.current || queue.current.epoch !== expected) {
        queue.current = { epoch: expected, view: next, pending: new Map(), running: false, reset: false, refresh: false };
      } else { queue.current.view = next; }
    } catch (failure) {
      if (expected === epoch.current && sequence === requestSequence.current) setError(String(failure));
    } finally {
      if (expected === epoch.current && sequence === requestSequence.current) setLoading(false);
    }
  };
  loadRef.current = () => { void load(epoch.current, !view); };
  useEffect(() => {
    const expected = ++epoch.current;
    queue.current = null;
    setView(null); setPending(0); setResetOpen(false);
    void load(expected, true);
    return () => { epoch.current += 1; requestSequence.current += 1; };
  }, [input]);
  useEffect(() => {
    const unlisten = listenEvent<{ taskId: string }>("stockpile-task-changed", event => {
      const work = queue.current;
      if (!work || event.payload.taskId !== work.view.taskId) return;
      if (work.running || work.pending.size || work.reset) work.refresh = true;
      else loadRef.current();
    }).catch(() => undefined);
    return () => { void unlisten.then(stop => stop?.()); };
  }, []);
  useEffect(() => {
    const work = queue.current;
    if (work?.epoch === epoch.current) {
      if (work.running || work.pending.size || work.reset) work.refresh = true;
      else loadRef.current();
    }
  }, [materials]);

  const drain = async (work: StockpileWriteQueue) => {
    if (work.running) return;
    work.running = true;
    if (work.epoch === epoch.current) { setPending(work.pending.size + Number(work.reset)); setError(""); }
    try {
      while (work.reset || work.pending.size) {
        const entry = work.pending.entries().next().value as [string, number] | undefined;
        const next = work.reset
          ? await resetStockpileTask({ taskId: work.view.taskId, sourceRevision: work.view.sourceRevision })
          : await setStockpileTaskQuantity({ taskId: work.view.taskId, sourceRevision: work.view.sourceRevision, materialId: entry![0], quantity: entry![1] });
        if (work.reset) work.reset = false;
        else if (work.pending.get(entry![0]) === entry![1]) work.pending.delete(entry![0]);
        work.view = next;
        if (work.epoch === epoch.current) {
          requestSequence.current += 1;
          setView(next); setPending(work.pending.size + Number(work.reset));
        }
      }
      if (work.refresh && work.epoch === epoch.current) { work.refresh = false; loadRef.current(); }
    } catch (failure) {
      if (work.epoch === epoch.current) setError(String(failure));
    } finally {
      work.running = false;
      if (work.epoch === epoch.current) setPending(work.pending.size + Number(work.reset));
    }
  };
  const changeQuantity = (materialId: string, quantity: number) => {
    const work = queue.current;
    const item = work?.view.items.find(item => item.materialId === materialId);
    if (!work || !item || !Number.isFinite(quantity)) return;
    work.pending.set(materialId, Math.min(item.requiredCount, Math.max(0, Math.trunc(quantity))));
    setPending(work.pending.size + Number(work.reset));
    void drain(work);
  };
  const retry = async () => {
    const work = queue.current;
    if (!work || (!work.pending.size && !work.reset)) { loadRef.current(); return; }
    if (work.running) return;
    try {
      // A source edit invalidates the old write revision. Refresh only this
      // task, then rebase unsaved quantities by stable item identity.
      const next = await getStockpileTask(inputRef.current);
      if (work.epoch !== epoch.current) return;
      work.view = next;
      const required = new Map(next.items.map(item => [item.materialId, item.requiredCount]));
      for (const [id, quantity] of work.pending) {
        if (!required.has(id)) work.pending.delete(id);
        else work.pending.set(id, Math.min(quantity, required.get(id)!));
      }
      setView(next); setPending(work.pending.size + Number(work.reset));
      await drain(work);
    } catch (failure) {
      if (work.epoch === epoch.current) setError(String(failure));
    }
  };
  const materialsById = useMemo(() => new Map(materials.map(item => [item.id, item])), [materials]);
  const rows = useMemo(() => {
    const query = search.trim().toLocaleLowerCase(locale);
    return (view?.items || []).filter(item => {
      if (filter === "remaining" && item.complete || filter === "done" && !item.complete) return false;
      const material = materialsById.get(item.materialId);
      return !query || `${material ? materialDisplayName(material) : ""} ${item.materialId}`.toLocaleLowerCase(locale).includes(query);
    });
  }, [view, filter, search, materialsById, locale]);
  const virtual = useVirtualWindow(rows, scrollRef, 57, 8);
  useLayoutEffect(() => { if (scrollRef.current) scrollRef.current.scrollTop = 0; }, [search, filter, input]);
  const count = (value: number) => value.toLocaleString(locale);
  const percent = view && view.requiredCount > 0 ? Math.min(100, view.preparedCount / view.requiredCount * 100) : view ? 100 : 0;
  return <section className="stockpile-task" aria-busy={loading}>
    <div className="stockpile-summary" aria-live="polite">
      <span>{t("stockpile.quantityProgress", { prepared: count(view?.preparedCount || 0), required: count(view?.requiredCount || 0), remaining: count(view?.remainingCount || 0) })}</span>
      <span>{t("stockpile.itemProgress", { complete: count(view?.completedItems || 0), total: count(view?.totalItems || 0) })}</span>
      <progress value={percent} max={100} aria-label={t("stockpile.progress")} />
    </div>
    <div className="beta-material-options stockpile-filters">
      <Input type="search" aria-label={t("statistics.searchDetails")} placeholder={t("editor.nameSearchHint")} value={search} onChange={event => onSearch(event.target.value)} />
      <Dropdown value={filter} options={[{ label: t("stockpile.all"), value: "all" }, { label: t("stockpile.remaining"), value: "remaining" }, { label: t("stockpile.done"), value: "done" }]} onChange={setFilter} ariaLabel={t("stockpile.filter")} />
      <Button size="sm" variant="ghost" disabled={!view || pending > 0 || loading} onClick={() => setResetOpen(true)}>{t("stockpile.reset")}</Button>
    </div>
    <div className="stockpile-save-status muted" role="status">{loading ? t("editor.loading") : error ? t("stockpile.notSaved") : pending ? t("editor.saving") : view ? t("stockpile.saved") : ""}</div>
    {error && <div className="subwindow-error stockpile-error" role="alert"><span>{t("stockpile.failed", { error })}</span><Button size="sm" onClick={() => void retry()}>{t("stockpile.retry")}</Button></div>}
    {loading && <p role="status">{t("stockpile.loading")}</p>}
    <div className="material-table-scroll stockpile-table-scroll" ref={scrollRef}>
      <Table className="material-list-table beta-material-table stockpile-table">
        <TableHeader><TableRow><TableHead scope="col">{t("stockpile.complete")}</TableHead><TableHead scope="col">{t("statistics.material")}</TableHead><TableHead scope="col">{t("stockpile.required")}</TableHead><TableHead scope="col">{t("stockpile.prepared")}</TableHead><TableHead scope="col">{t("stockpile.remainingQuantity")}</TableHead></TableRow></TableHeader>
        <TableBody>{virtual.topSpacerHeight > 0 && <TableRow aria-hidden><VirtualSpacerCell colSpan={5} height={virtual.topSpacerHeight} /></TableRow>}
          {virtual.visibleItems.map((item, index) => {
            const material = materialsById.get(item.materialId);
            const name = material ? materialDisplayName(material) : item.materialId;
            const quantity = (value: number) => material ? formatMaterialQuantity(material, value, asItems) : count(value);
            return <TableRow key={item.materialId} className={(virtual.startIndex + index) % 2 ? "material-list-row-odd" : "material-list-row-even"} onMouseEnter={() => onHover(material || null)} onMouseLeave={() => onHover(null)}>
              <TableCell><Checkbox checked={item.complete} disabled={loadingMaterials || item.requiredCount === 0} aria-label={t("stockpile.markComplete", { name })} onCheckedChange={checked => changeQuantity(item.materialId, checked === true ? item.requiredCount : 0)} /></TableCell>
              <TableCell><div className="beta-material-name"><BlockIcon blockId={material?.iconHint || item.materialId} /><span title={item.materialId}>{name}</span></div></TableCell>
              <TableCell className="beta-material-quantity">{quantity(item.requiredCount)}</TableCell>
              <TableCell><StockpileQuantityInput quantity={item.preparedCount} required={item.requiredCount} label={t("stockpile.preparedFor", { name })} disabled={loadingMaterials || item.requiredCount === 0} onQuantity={value => changeQuantity(item.materialId, value)} /></TableCell>
              <TableCell className="beta-material-quantity">{quantity(item.remainingCount)}</TableCell>
            </TableRow>;
          })}
          {virtual.bottomSpacerHeight > 0 && <TableRow aria-hidden><VirtualSpacerCell colSpan={5} height={virtual.bottomSpacerHeight} /></TableRow>}
          {!loading && view && rows.length === 0 && <TableRow><TableCell colSpan={5} className="material-list-empty-cell">{view.totalItems === 0 ? t("stockpile.empty") : t("statistics.noMatches")}</TableCell></TableRow>}
        </TableBody>
      </Table>
    </div>
    {resetOpen && <Dialog title={t("stockpile.reset")} onClose={() => setResetOpen(false)} width="sm" footer={<><Button variant="ghost" onClick={() => setResetOpen(false)}>{t("editor.cancel")}</Button><Button variant="destructive" onClick={() => { setResetOpen(false); const work = queue.current; if (work) { work.reset = true; void drain(work); } }}>{t("stockpile.confirmReset")}</Button></>}><p>{t("stockpile.resetPrompt")}</p></Dialog>}
  </section>;
}

/**
 * Shared material-list body used by both the modal dialog and the desktop child window.
 */
export function MaterialListContent({ data, onClose, currentFile, standalone = false, embedded = false, viewMode = "details", onViewModeChange }: {
  data: StatsData; onClose?: () => void; currentFile: string; standalone?: boolean; embedded?: boolean; viewMode?:"details"|"export"; onViewModeChange?:(mode:"details"|"export")=>void;
}) {
  const { locale } = useI18n();
  const languageRevision = useSyncExternalStore(subscribeI18nChanges,getI18nRevision,getI18nRevision);
  const [contentMode,setContentMode] = useState(viewMode);
  useEffect(() => setContentMode(viewMode),[viewMode]);
  const [multiplier, setMultiplier] = useState(1);
  const [asItems, setAsItems] = useState(false);
  const [search, setSearch] = useState("");
  const [scopeMaterials, setScopeMaterials] = useState(data.materials);
  const [detailsLoading, setDetailsLoading] = useState(false);
  const [taskPending, setTaskPending] = useState(false);
  const [hoverItem, setHoverItem] = useState<MaterialItem | null>(null);
  const [mousePos, setMousePos] = useState({ x:0, y:0 });
  const detailsGeneration = useRef(0);
  const [includeContainerItems, setIncludeContainerItems] = useState(data.containerScan.enabled);
  const [exportFormat, setExportFormat] = useState<"svg" | "txt" | "html" | "csv">("svg");
  const [terminalRawMaterials, setTerminalRawMaterials] = useState(false);
  const [workbook, setWorkbook] = useState("scope:all");
  useEffect(() => {
    setWorkbook("scope:all"); setMultiplier(1); setSearch("");
    setIncludeContainerItems(data.containerScan.enabled);
  }, [currentFile]);
  const [webMode, setWebMode] = useState<"single" | "multi">("single");
  const [accessPassword, setAccessPassword] = useState("");
  const [adminPassword, setAdminPassword] = useState("");
  const [whitelist, setWhitelist] = useState("");
  const [whitelistEnabled, setWhitelistEnabled] = useState(false);
  const [allowGuestReadonly, setAllowGuestReadonly] = useState(false);
  const [adminPageEnabled, setAdminPageEnabled] = useState(true);
  const [webTargets, setWebTargets] = useState<Array<"windows-x64" | "linux-x64" | "macos-x64" | "macos-arm64">>(["windows-x64"]);
  const [isExporting, setIsExporting] = useState(false);
  const [isPreparingRecipes, setIsPreparingRecipes] = useState(false);
  const [recipeStatus, setRecipeStatus] = useState<UiMessage>("");
  const [exportStatus, setExportStatus] = useState<UiMessage>("");
  const [error, setError] = useState<UiMessage>("");
  const exportInFlight = useRef(false);
  const prepareRecipesInFlight = useRef(false);
  const busy = isExporting || isPreparingRecipes;
  const selectedScope = useMemo(() => workbook.startsWith("region:") ? { kind:"region" as const, value:workbook.slice(7) }
    : workbook.startsWith("layer:") ? { kind:"layer" as const, value:workbook.slice(6) } : { kind:"all" as const, value:null },[workbook]);
  useEffect(() => {
    const generation = ++detailsGeneration.current;
    if (contentMode !== "details") return;
    setHoverItem(null); setError("");
    if (selectedScope.kind === "all" && includeContainerItems === data.containerScan.enabled) {
      setScopeMaterials(data.materials); setDetailsLoading(false); return;
    }
    setDetailsLoading(true);
    loadMaterialsScope(currentFile, selectedScope, includeContainerItems)
      .then(materials => { if (generation === detailsGeneration.current) setScopeMaterials(materials); })
      .catch(error => { if (generation === detailsGeneration.current) { setScopeMaterials([]); setError({ key: "statistics.detailsFailed", params: {error:String(error)} }); } })
      .finally(() => { if (generation === detailsGeneration.current) setDetailsLoading(false); });
    return () => { detailsGeneration.current += 1; };
  },[currentFile,data,selectedScope,includeContainerItems,contentMode]);
  const contentClassName = [standalone ? "material-list-window" : embedded ? "material-list-content-embedded" : "dialog-content",
    embedded ? "" : "subwindow-frame", "material-list-content", standalone ? "subwindow-frame-full material-list-content-standalone" : ""].filter(Boolean).join(" ");
  const options = useMemo(() => [{ label: t("statistics.all"), value: "scope:all" },
    ...(data.regions || []).map(region => ({ label: t("statistics.regionScope", {name:region.name}), value: `region:${region.name}` })),
    ...(data.layers || []).map(layer => ({ label: t("statistics.layerScope", {y:layer.world_y, layer:layer.layer}), value: `layer:${layer.layer}` }))], [data.regions, data.layers, locale]);
  const handleExport = async () => {
    if (exportInFlight.current) return;
    exportInFlight.current = true; setIsExporting(true); setExportStatus(""); setError("");
    try {
      const scope = workbook.startsWith("region:") ? { kind: "region" as const, value: workbook.slice(7) }
        : workbook.startsWith("layer:") ? { kind: "layer" as const, value: workbook.slice(6) } : { kind: "all" as const, value: null };
      const ok = await exportMaterialListWithOptions(currentFile, {
        scope, includeContainerItems, multiplier, minecraftVersion: "26.3", terminalRawMaterials, format: exportFormat,
        webOptions: exportFormat === "html" ? { mode: webMode,
          accessPassword: webMode === "multi" ? accessPassword : undefined,
          adminPassword: webMode === "multi" ? adminPassword : undefined,
          whitelist: whitelist.split(/[\s,，;；]+/).filter(Boolean), whitelistEnabled, allowGuestReadonly, adminPageEnabled, targets: webTargets } : undefined,
      });
      if (ok) { setExportStatus({key:"statistics.exported"}); setAccessPassword(""); setAdminPassword(""); }
    } catch (error: any) { setError({ key: "statistics.exportFailed", params: {error:String(error)} }); }
    finally { exportInFlight.current = false; setIsExporting(false); }
  };
  const handlePrepareRecipes = async () => {
    if (prepareRecipesInFlight.current) return;
    prepareRecipesInFlight.current = true; setIsPreparingRecipes(true); setRecipeStatus(""); setError("");
    try { await fetchMaterialRecipeCache(); setRecipeStatus({key:"statistics.recipesReady"}); }
    catch (error: any) { setError({ key: "statistics.recipesFailed", params: {error:String(error)} }); }
    finally { prepareRecipesInFlight.current = false; setIsPreparingRecipes(false); }
  };
  return <div className={contentClassName} onMouseMove={event => setMousePos({ x:event.clientX, y:event.clientY })} onMouseLeave={() => setHoverItem(null)}>
    {!embedded && <div className="subwindow-title-bar"><h3 className="subwindow-title">{contentMode === "details" ? t("editor.materialDetails") : t("editor.exportMaterials")}</h3>
      {onClose && <Button variant="ghost" size="icon" className="subwindow-close-button" aria-label={t("editor.closeWindow")} onClick={onClose}><X size={18} /></Button>}
    </div>}
    <div className="subwindow-body">
      <ScrollArea className="material-export-settings">
        {contentMode === "details" && <section className="material-details-section" aria-busy={detailsLoading}>
          <div className="material-export-grid">
            <label className="material-export-field"><span>{t("statistics.scope")}</span><Dropdown value={workbook} options={options} onChange={setWorkbook} disabled={taskPending} ariaLabel={t("statistics.detailsScope")} /></label>
            <label className="material-export-field"><span>{t("statistics.copies")}</span><Input type="number" min={1} max={1000000} aria-label={t("statistics.detailsCopies")} value={multiplier} disabled={taskPending} onChange={event => setMultiplier(Math.min(1000000,Math.max(1,Math.floor(Number(event.target.value)||1))))} /></label>
          </div>
          <label className="beta-material-check"><Checkbox checked={includeContainerItems} disabled={taskPending} onCheckedChange={checked => setIncludeContainerItems(checked === true)} />{t("statistics.includeContainers")}</label>
          <div className="stockpile-source muted" title={displayFilePath(currentFile)}>{displayFilePath(currentFile)}</div>
          <QuantitySwitch checked={asItems} onChange={setAsItems} />
          <StockpileTaskList sourcePath={currentFile} scope={selectedScope} includeContainerItems={includeContainerItems} multiplier={multiplier} materials={scopeMaterials} asItems={asItems} search={search} onSearch={setSearch} loadingMaterials={detailsLoading} onHover={setHoverItem} onPendingChange={setTaskPending} />
        </section>}
        {contentMode === "export" && <>
        <fieldset className="material-export-section" disabled={busy}>
          <legend>{t("statistics.materialFormat")}</legend>
          <div className="material-export-grid">
            <label className="material-export-field"><span>{t("statistics.scope")}</span><Dropdown value={workbook} options={options} onChange={setWorkbook} disabled={busy} ariaLabel={t("statistics.exportScope")} /></label>
            <label className="material-export-field"><span>{t("statistics.copies")}</span><Input type="number" min={1} max={1000000} aria-label={t("statistics.copies")} value={multiplier}
              onChange={event => setMultiplier(Math.min(1000000, Math.max(1, Math.floor(Number(event.target.value) || 1))))} /></label>
            <label className="material-export-field"><span>{t("statistics.exportFormat")}</span><Dropdown value={exportFormat} options={[{ label: t("statistics.svgArt"), value: "svg" }, { label: "CSV", value: "csv" }, { label: "TXT", value: "txt" }, { label: t("statistics.web"), value: "html" }]}
              onChange={value => setExportFormat(value as typeof exportFormat)} disabled={busy} ariaLabel={t("statistics.exportFormatLabel")} /></label>
          </div>
          <div className="beta-material-options">
            <label className="beta-material-check"><Checkbox checked={includeContainerItems} disabled={busy} onCheckedChange={checked => setIncludeContainerItems(checked === true)} />{t("statistics.includeContainers")}</label>
            <label className="beta-material-check"><Checkbox checked={terminalRawMaterials} disabled={busy} onCheckedChange={checked => setTerminalRawMaterials(checked === true)} />{t("statistics.terminalExport")}</label>
          </div>
          <p className="beta-statistics-status">{t("statistics.scopeHint")}</p>
        </fieldset>
        {terminalRawMaterials && <section className="material-export-section">
          <h4>{t("statistics.terminalRecipes")}</h4><p className="beta-statistics-status">{t("statistics.recipesHint")}</p>
          <Button title={t("statistics.prepareRecipesHint")} onClick={handlePrepareRecipes} disabled={busy}>{isPreparingRecipes ? t("statistics.preparingRecipes") : t("statistics.prepareRecipes")}</Button>
          {recipeStatus && <p role="status">{localizeMessage(recipeStatus)}</p>}
        </section>}
        {exportFormat === "html" && <fieldset className="material-export-section" disabled={busy}>
          <legend>{t("statistics.webOptions")}</legend>
          <label className="material-export-field"><span>{t("statistics.webMode")}</span><Dropdown value={webMode} options={[{ label: t("statistics.webSingle"), value: "single" }, { label: t("statistics.webMulti"), value: "multi" }]}
            onChange={value => setWebMode(value as typeof webMode)} disabled={busy} ariaLabel={t("statistics.webUsage")} /></label>
          {webMode === "multi" && <>
            <div className="material-export-grid">
              <label className="material-export-field"><span>{t("statistics.accessPassword")}</span><Input type="password" autoComplete="new-password" aria-label={t("statistics.webAccessPassword")} value={accessPassword} onChange={event => setAccessPassword(event.target.value)} /></label>
              <label className="material-export-field"><span>{t("statistics.adminPassword")}</span><Input type="password" autoComplete="new-password" aria-label={t("statistics.webAdminPassword")} value={adminPassword} onChange={event => setAdminPassword(event.target.value)} /></label>
              <label className="material-export-field material-export-field-wide"><span>{t("statistics.whitelist")}</span><Input aria-label={t("statistics.whitelist")} placeholder={t("statistics.whitelistHint")} value={whitelist} onChange={event => setWhitelist(event.target.value)} /></label>
            </div>
            <div className="beta-material-options">
              <label className="beta-material-check"><Checkbox checked={whitelistEnabled} disabled={busy} onCheckedChange={checked => setWhitelistEnabled(checked === true)} />{t("statistics.enableWhitelist")}</label>
              <label className="beta-material-check"><Checkbox checked={allowGuestReadonly} disabled={busy} onCheckedChange={checked => setAllowGuestReadonly(checked === true)} />{t("statistics.guestReadonly")}</label>
              <label className="beta-material-check"><Checkbox checked={adminPageEnabled} disabled={busy} onCheckedChange={checked => setAdminPageEnabled(checked === true)} />{t("statistics.enableAdmin")}</label>
            </div>
            <fieldset className="material-export-targets"><legend>{t("statistics.targets")}</legend>
              {([["windows-x64", "Windows x64"], ["linux-x64", "Linux x64"], ["macos-x64", "macOS x64"], ["macos-arm64", "macOS ARM64"]] as const).map(([value, label]) =>
                <label className="beta-material-check" key={value}><Checkbox checked={webTargets.includes(value)} disabled={busy}
                  onCheckedChange={checked => setWebTargets(previous => checked === true ? [...previous.filter(target => target !== value), value] : previous.filter(target => target !== value))} />{label}</label>)}
            </fieldset>
          </>}
        </fieldset>}
        </>}
      </ScrollArea>
      <div className="material-export-footer">
        <Button variant="secondary" disabled={busy || taskPending} onClick={() => { const mode = contentMode === "details" ? "export" : "details"; setContentMode(mode); onViewModeChange?.(mode); setHoverItem(null); }}>{contentMode === "details" ? t("statistics.openExport") : t("statistics.returnDetails")}</Button>
        {contentMode === "export" && <Button onClick={handleExport} disabled={busy || !currentFile}>{isExporting ? t("statistics.exporting") : t("editor.exportMaterials")}</Button>}
        {exportStatus && <span role="status">{localizeMessage(exportStatus)}</span>}
        {error && <pre className="subwindow-error" role="alert">{localizeMessage(error)}</pre>}
      </div>
    </div>
    <MaterialTooltip x={mousePos.x} y={mousePos.y} item={hoverItem} multiplier={multiplier} />
  </div>;
}

export function MaterialsDialog({ data, onClose, currentFile, viewMode = "details" }: { data: StatsData; onClose: () => void; currentFile: string; viewMode?:"details"|"export" }) {
  const { locale } = useI18n();
  const [mode,setMode] = useState(viewMode);
  return (
    <Dialog title={mode === "details" ? t("editor.materialDetails") : t("editor.exportMaterials")} onClose={onClose} width="lg" className="materials-dialog">
      <MaterialListContent data={data} currentFile={currentFile} embedded viewMode={mode} onViewModeChange={setMode} />
    </Dialog>
  );
}

export function StatisticsPage({ currentFile, theme, debugMode = false }: any) {
  const { locale } = useI18n();
  const languageRevision = useSyncExternalStore(subscribeI18nChanges,getI18nRevision,getI18nRevision);
  const applicationRender = useSyncExternalStore(
    subscribeApplicationRender,
    getApplicationRenderState,
    getApplicationRenderState,
  );
  const [data, setData] = useState<StatsData | null>(null);
  const [error, setError] = useState<UiMessage>("");
  const [showMaterials, setShowMaterials] = useState(false);
  const [materialsViewMode,setMaterialsViewMode] = useState<"details"|"export">("details");
  const [materialSearch, setMaterialSearch] = useState("");
  const [includeContainerItems, setIncludeContainerItems] = useState(false);
  const [asItems, setAsItems] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const statsGeneration = useRef(0);
  const [showEnumerator, setShowEnumerator] = useState(false);
  useEffect(() => { if (!debugMode) setShowEnumerator(false); }, [debugMode]);
  const [showEnumeratorInfoEditor, setShowEnumeratorInfoEditor] = useState(false);
  const [enumeratorCollections, setEnumeratorCollections] = useState<EnumeratorCollection[]>([]);
  const [enumeratorError, setEnumeratorError] = useState<UiMessage>("");
  const [enumeratorLoading, setEnumeratorLoading] = useState(false);
  const [enumeratorRules, setEnumeratorRules] = useState<string[]>([...DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES]);
  const [configLoading, setConfigLoading] = useState(true);
  const runtimeCollection = useMemo(() => data?.hasStructuralBlockCounts ? createRuntimeProjectionCollection(currentFile, data.structuralBlocks) : null,[currentFile,data]);
  useEffect(() => {
    let active = true;
    loadUserConfigMigratingLocalStorage().then(info => { if(active) setEnumeratorRules(normalizeStatisticsEnumeratorInfoRules(info.config.statistics_enumerator_info_rules)); })
      .catch(error => { if(active) setEnumeratorError({ key: "statistics.rulesFailed", params: {error:String(error)} }); })
      .finally(() => { if(active) setConfigLoading(false); });
    return () => { active = false; };
  },[]);
  useEffect(() => {
    let active = true;
    if(!runtimeCollection) { setEnumeratorCollections([]); setEnumeratorLoading(false); return; }
    setEnumeratorLoading(true); setEnumeratorError("");
    loadEnumeratorCollections([runtimeCollection]).then(collections => { if(active) setEnumeratorCollections(collections); })
      .catch(error => { if(active) { setEnumeratorCollections([]); setEnumeratorError({ key: "statistics.collectionsFailed", params: {error:String(error)} }); } })
      .finally(() => { if(active) setEnumeratorLoading(false); });
    return () => { active = false; };
  },[runtimeCollection]);
  const enumeratorResults = useMemo(() => buildEnumeratorInfoResults(enumeratorRules,data,enumeratorCollections),[enumeratorRules,data,enumeratorCollections,locale]);
  const saveEnumeratorRules = async (rules:string[]) => {
    const info = await saveStatisticsEnumeratorInfoRulesConfig(rules);
    setEnumeratorRules(normalizeStatisticsEnumeratorInfoRules(info.config.statistics_enumerator_info_rules));
  };

  const renderOperation = useMemo(
    () => findRenderOperationForSource(currentFile, applicationRender),
    [applicationRender.operations, currentFile],
  );
  const applicationAnalysis = hasInventoryMaterialSemantics(renderOperation?.analysis) && Array.isArray(renderOperation?.analysis?.structural_block_counts) ? renderOperation!.analysis : null;
  const applicationAnalysisKey = applicationAnalysis ? renderOperation?.operationId ?? "analysis" : "";
  const renderPending = isApplicationRenderPending(renderOperation);
  const sortedMaterials = useMemo(() => data ? data.materials.map(material => ({...material,name:materialDisplayName(material)})).sort((a, b) => b.totalCount - a.totalCount) : [], [data,languageRevision]);
  const topMaterials = useMemo(() => filterMaterialItems(sortedMaterials, materialSearch), [sortedMaterials, materialSearch]);
  useLayoutEffect(() => {
    if (!data?.timing) return;
    console.info(`[NOVA_STATS_TIMING] request_to_list_commit_ms=${(performance.now() - data.timing.requestStarted).toFixed(3)} map_to_commit_ms=${(performance.now() - data.timing.mappedAt).toFixed(3)} cache_reused=${data.timing.cacheReused}`);
  }, [data, isLoading]);
  const loadStats = async () => {
    if (!currentFile) return;
    const generation = ++statsGeneration.current;
    const source = applicationAnalysis && !includeContainerItems ? "application" : "source";
    console.info(`[NOVA_STATS_REQUEST] source=${source} path=${currentFile} request started`);
    setError("");
    setIsLoading(true);
    try {
      const next = applicationAnalysis && !includeContainerItems
        ? mapStructureStats(applicationAnalysis)
        : await loadStructureStats(currentFile, includeContainerItems);
      if (generation !== statsGeneration.current) return;
      setData(next);
      console.info(`[NOVA_STATS_REQUEST] source=${source} path=${currentFile} request succeeded`);
    } catch (err: any) {
      if (generation === statsGeneration.current) {
        setData(null);
        setError(String(err));
        console.error(`[NOVA_STATS_REQUEST] source=${source} path=${currentFile} request failed: ${String(err)}`);
      }
    } finally {
      if (generation === statsGeneration.current) setIsLoading(false);
    }
  };

  useEffect(() => {
    if (!currentFile) {
      statsGeneration.current += 1;
      setData(null);
      setError("");
      return;
    }
    setData(null);
    void loadStats();
    return () => { statsGeneration.current += 1; };
  }, [currentFile, includeContainerItems, applicationAnalysisKey]);

  if (!currentFile) {
    return <div className="beta-statistics-empty"><h2>{t("statistics.title")}</h2><p>{t("statistics.empty")}</p></div>;
  }
  const scan = data?.containerScan;
  return <React.Profiler id="statistics" onRender={recordStatisticsCommit}><div className="beta-statistics-page">
    <div className="beta-statistics-toolbar">
      <Button onClick={() => openMaterialsWithWindowBehavior(currentFile, () => { setMaterialsViewMode("details"); setShowMaterials(true); })} disabled={!data || isLoading}>{t("editor.materialDetails")}</Button>
      <Button onClick={() => openMaterialsWithWindowBehavior(currentFile, () => { setMaterialsViewMode("export"); setShowMaterials(true); }, "export")} disabled={!data || isLoading}>{t("editor.exportMaterials")}</Button>
      {debugMode && <Button variant="secondary" disabled={!data} onClick={() => openEnumeratorWithWindowBehavior(currentFile,() => setShowEnumerator(true))}>{t("statistics.openEnumerator")}</Button>}
      <label className="beta-material-check"><Checkbox  checked={includeContainerItems} onCheckedChange={checked => setIncludeContainerItems((checked === true))} />{t("statistics.includeContainers")}</label>
      <QuantitySwitch checked={asItems} onChange={setAsItems} />
      <span className="beta-statistics-file" title={displayFilePath(currentFile)}>{currentFile.split(/[\\/]/).pop()}</span>
      <Button onClick={loadStats} disabled={isLoading}>{isLoading ? t("statistics.analyzing") : t("statistics.reanalyze")}</Button>
    </div>
    {includeContainerItems && scan && <div className="beta-statistics-status" role="status">{t("statistics.scanned", {containers:scan.containers_scanned.toLocaleString(locale), stacks:scan.item_stacks_scanned.toLocaleString(locale)})}</div>}
    {error && <pre className="subwindow-error" role="alert">{localizeMessage(error)}</pre>}
    {!!scan?.warnings?.length && <details className="beta-statistics-warnings"><summary>{t("statistics.containerWarnings", {count:scan.warnings.length.toLocaleString(locale)})}</summary><pre>{scan.warnings.slice(0, 40).map(localizeMessage).join("\n")}</pre></details>}
    <div className="beta-statistics-panes">
      <section className="beta-statistics-structure"><h2>{t("statistics.structure")}</h2>
        {data ? <ScrollArea className="beta-statistics-facts-scroll"><dl className="beta-statistics-facts">
          <ReadOnlyRow label={t("statistics.nonAir")} value={data.totalNonAirBlocks.toLocaleString(getUiLocale())} />
          <ReadOnlyRow label={t("statistics.regions")} value={data.regionCount} />
          <ReadOnlyRow label={t("statistics.bounds")} value={`${data.enclosingSize.x} × ${data.enclosingSize.y} × ${data.enclosingSize.z}`} />
          <ReadOnlyRow label={t("statistics.density")} value={`${(data.density * 100).toLocaleString(locale, {minimumFractionDigits:2,maximumFractionDigits:2})}%`} />
          <ReadOnlyRow label={t("statistics.buildingType")} value={localizeMessage(data.buildingType)} />
          <ReadOnlyRow label={t("statistics.redstoneRatio")} value={data.redstoneRatio === null ? t("statistics.notProvided") : `${(data.redstoneRatio * 100).toLocaleString(locale, {minimumFractionDigits:2,maximumFractionDigits:2})}%`} />
          <ReadOnlyRow label={t("statistics.fluidRatio")} value={data.fluidRatio === null ? t("statistics.notProvided") : `${(data.fluidRatio * 100).toLocaleString(locale, {minimumFractionDigits:2,maximumFractionDigits:2})}%`} />
          <ReadOnlyRow label={t("statistics.entities")} value={data.entityCount} />
        </dl>
          <section className="statistics-enumerator-panel" aria-busy={enumeratorLoading || configLoading}>
            <header className="material-summary-header"><h3>{t("statistics.customTitle")}</h3><Button variant="secondary" disabled={configLoading || enumeratorLoading || !enumeratorCollections.length || !!enumeratorError} onClick={() => setShowEnumeratorInfoEditor(true)}>{t("statistics.editRules")}</Button></header>
            {enumeratorLoading || configLoading ? <p role="status">{t("statistics.loadingRules")}</p> : enumeratorError ? <pre role="alert">{localizeMessage(enumeratorError)}</pre>
              : enumeratorResults.map((item,index) => <div key={index} className="statistics-enumerator-row"><div className="statistics-enumerator-row-main"><span>{item.title}</span><strong>{item.error ? t("editor.error") : item.value}</strong></div>{item.error && <p className="statistics-enumerator-row-error">{localizeMessage(item.error)}</p>}</div>)}
          </section>
        </ScrollArea> : <p className="beta-statistics-status">{renderPending ? t("statistics.reading") : isLoading ? t("statistics.analyzing") : t("statistics.noData")}</p>}
      </section>
      <section className="beta-statistics-materials"><header className="material-summary-header"><h2>{t("editor.materials")}</h2>{data && <span>{t("statistics.materialKinds", {visible:topMaterials.length.toLocaleString(locale), total:data.materials.length.toLocaleString(locale)})}</span>}</header>
        <Input className="input material-summary-search" type="search" aria-label={t("editor.materialSearch")} placeholder={t("editor.materialSearchHint")} value={materialSearch} onChange={event => setMaterialSearch(event.target.value)} />
        {data ? <MaterialTable materials={topMaterials} asItems={asItems} /> : <p className="beta-statistics-status">{isLoading ? t("statistics.countingMaterials") : t("statistics.noMaterials")}</p>}
      </section>
    </div>
    {showEnumerator && <EnumeratorDialog currentFile={currentFile} theme={theme} runtimeCollection={runtimeCollection} onClose={() => setShowEnumerator(false)} />}
    {showEnumeratorInfoEditor && <EnumeratorInfoEditorDialog rules={enumeratorRules} data={data} collections={enumeratorCollections} onClose={() => setShowEnumeratorInfoEditor(false)} onSave={saveEnumeratorRules} />}
    {showMaterials && data && <MaterialsDialog data={data} onClose={() => setShowMaterials(false)} currentFile={currentFile} viewMode={materialsViewMode} />}
  </div></React.Profiler>;
}

function ReadOnlyRow({ label, value }: { label: string; value: string | number }) {
  return <div><dt>{label}</dt><dd>{typeof value === "number" ? value.toLocaleString(getUiLocale()) : value}</dd></div>;
}
