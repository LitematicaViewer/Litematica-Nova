import { Checkbox } from "../../../../components/ui/checkbox";
import { TableHeader, TableRow, TableHead, TableBody, TableCell, Table } from "../../../../components/ui/table";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
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
} from "../../../../../src/business/facade";
import { useSyncExternalStore } from "react";
import {
  loadUserConfigMigratingLocalStorage,
  normalizeMaterialListWindowBehavior,
  openMaterialListWindow,
} from "../../../../../src/business/facade";
import { VirtualSpacerCell } from "../../../../components/VirtualSpacerCell";
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
          title: raw.trim() || "未命名项",
          expression: "",
          raw,
        };
      }
      return {
        title: raw.slice(0, separatorIndex).trim() || "未命名项",
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
      return { kind: "older", numbers: [1, 12, 2], label: "1.12.2之前" };
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
  if (!leftToken && !rightToken) return left.name.localeCompare(right.name, "zh-CN");
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
      return "未配置版本集合";
    }
    return hasHigherVersion ? "更高版本" : "--";
  }
  const versionToken = parseVersionToken(latestVersion);
  const label = versionToken?.label || latestVersion.name;
  return hasHigherVersion ? `${label} / 更高版本` : label;
}

function evaluateEnumeratorInfoRule(rule: EnumeratorInfoRule, data: StatsData, collections: EnumeratorCollection[]): EnumeratorInfoResult {
  if (!rule.expression) {
    return { title: rule.title, value: "", error: "缺少表达式。", raw: rule.raw };
  }
  if (data && !data.hasStructuralBlockCounts) return { title: rule.title, value: "", error: "统计快照缺少原始结构方块数据，请重新分析", raw:rule.raw };
  const call = parseFunctionCall(rule.expression);
  if (!call) {
    return { title: rule.title, value: "", error: "表达式必须为 函数(集合表达式) 形式。", raw: rule.raw };
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
      return { title: rule.title, value: values.length > 0 ? "是" : "否", error: "", raw: rule.raw };
    case "ratio": {
      if (data.totalNonAirBlocks <= 0) {
        return { title: rule.title, value: "--", error: "", raw: rule.raw };
      }
      const countById = new Map(data.structuralBlocks.map((block) => [block.id, block.count]));
      const numerator = values.reduce((sum, value) => sum + (countById.get(value) || 0), 0);
      return {
        title: rule.title,
        value: `${((numerator / data.totalNonAirBlocks) * 100).toFixed(1)}%`,
        error: "",
        raw: rule.raw,
      };
    }
    default:
      return { title: rule.title, value: "", error: `不支持的函数：${call.name}`, raw: rule.raw };
  }
}

export function buildEnumeratorInfoResults(rules: string[], data: StatsData | null, collections: EnumeratorCollection[]): EnumeratorInfoResult[] {
  const parsedRules = parseEnumeratorInfoRules(rules);
  if (!data) {
    return parsedRules.map((rule) => ({ title: rule.title, value: "等待统计结果", error: "", raw: rule.raw }));
  }
  return parsedRules.map((rule) => evaluateEnumeratorInfoRule(rule, data, collections));
}

function MaterialTooltip({ x, y, item, multiplier }: { x: number; y: number; item: MaterialItem | null; multiplier: number }) {
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
  }, [x, y, item, total]);

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
        <span className="material-list-hover-popup-label">项目：</span>
        <BlockIcon blockId={item.iconHint} />
        <span className="material-list-hover-popup-name">{item.name}</span>
      </div>
      <div className="material-list-hover-popup-row">ID：{item.id}</div>
      <div className="material-list-hover-popup-row">
        总计：<strong>{total.toLocaleString("zh-CN")}</strong> · {formatMaterialQuantity(item, total)}
      </div>
      <div className="material-list-hover-popup-row">
        {item.materialStatus !== "item" ? "特殊方块：没有可直接放置的对应物品" : item.hasCustomStackSize ? "包含自定义堆叠上限，按个显示" : !item.canFitContainer ? "此物品不能装入潜影盒，按个显示" : `每组 ${item.stackSize} 个；1 盒 = 27 组；1 箱盒 = 27 盒`}
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
  const [draft, setDraft] = useState(rules.join("\n"));
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState("");

  useEffect(() => {
    setDraft(rules.join("\n"));
  }, [rules]);

  const parsedDraftRules = useMemo(
    () => normalizeStatisticsEnumeratorInfoRules(draft.split(/\r?\n/)),
    [draft],
  );

  const previewResults = useMemo(
    () => buildEnumeratorInfoResults(parsedDraftRules, data, collections),
    [collections, data, parsedDraftRules],
  );

  const hasErrors = previewResults.some((item) => Boolean(item.error));

  const handleSave = async () => {
    setIsSaving(true);
    setSaveError("");
    try {
      await onSave(parsedDraftRules);
      onClose();
    } catch (error: any) {
      setSaveError(String(error?.message || error || "保存失败"));
    } finally {
      setIsSaving(false);
    }
  };

  return <Dialog title="自定义统计" subtitle="按原始结构方块计算，容器物品不混入比例。" onClose={onClose} busy={isSaving} width="lg" footer={<>
    <Button variant="secondary" disabled={isSaving} onClick={() => setDraft(DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES.join("\n"))}>恢复默认</Button>
    <Button variant="secondary" disabled={isSaving} onClick={onClose}>取消</Button>
    <Button disabled={isSaving || hasErrors} onClick={handleSave}>{isSaving ? "正在保存…" : "保存"}</Button>
  </>}>
    <div className="statistics-enumerator-editor-columns">
      <div className="statistics-enumerator-editor-column">
        <label className="statistics-enumerator-editor-heading" htmlFor="statistics-rules">统计规则（一行一项）</label>
        <textarea id="statistics-rules" className="input statistics-enumerator-editor-textarea" value={draft} onChange={event => setDraft(event.target.value)} spellCheck={false} disabled={isSaving} />
        <div className="statistics-enumerator-editor-hint">标题 = 函数(集合表达式)。函数：version、any、ratio。集合运算：|、&amp;、-；L 表示当前结构方块集合。</div>
      </div>
      <div className="statistics-enumerator-editor-column"><h3 className="statistics-enumerator-editor-heading">实时预览</h3>
        {previewResults.map((item,index) => <div key={index} className="statistics-enumerator-preview-item">
          <div className="statistics-enumerator-preview-main"><span>{item.title}</span><strong>{item.error ? "错误" : item.value}</strong></div>
          {item.error && <p className="statistics-enumerator-preview-error" role="alert">{item.error}</p>}
        </div>)}
      </div>
    </div>
    {saveError && <pre className="subwindow-error" role="alert">{saveError}</pre>}
    {hasErrors && <p className="statistics-enumerator-editor-error">存在语法或集合错误，修复后即可保存。</p>}
  </Dialog>;
}

/**
 * Opens the material list according to the user's configured window behavior.
 */
export async function openMaterialsWithWindowBehavior(currentFile: string, showOverlay: () => void, viewMode: "details" | "export" = "details"): Promise<void> {
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
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const virtual = useVirtualWindow(materials,scrollRef,48,8);
  useLayoutEffect(() => { if(scrollRef.current) scrollRef.current.scrollTop = 0; },[materials]);
  if (!breakdown) return <div className="material-table-scroll material-summary-scroll" ref={scrollRef}><ul className="material-summary-list" aria-label="材料列表">
    {virtual.topSpacerHeight > 0 && <li className="material-virtual-spacer" aria-hidden style={{height:virtual.topSpacerHeight}} /> }
    {virtual.visibleItems.map(material => <li key={material.id} onMouseEnter={() => onHover?.(material)} onMouseLeave={() => onHover?.(null)}>
      <BlockIcon blockId={material.iconHint} />
      <span className="material-summary-name" title={material.id}>{material.name}
        {material.materialStatus !== "item" && <small>{material.materialStatus === "no_item" ? "无直接对应物品" : "未知物品"}</small>}
      </span>
      <span className="material-summary-quantity">{formatMaterialQuantity(material, material.totalCount * multiplier, asItems)}</span>
    </li>)}
    {virtual.bottomSpacerHeight > 0 && <li className="material-virtual-spacer" aria-hidden style={{height:virtual.bottomSpacerHeight}} /> }
    {materials.length === 0 && <li className="material-summary-empty">没有符合条件的材料</li>}
  </ul></div>;
  return <div className="material-table-scroll material-detail-table-scroll" ref={scrollRef}><Table className="material-list-table beta-material-table">
    <TableHeader><TableRow><TableHead scope="col">材料</TableHead>{breakdown && <><TableHead scope="col">建造用量</TableHead><TableHead scope="col">容器物品</TableHead></>}<TableHead scope="col">{breakdown ? "合计" : "数量"}</TableHead></TableRow></TableHeader>
    <TableBody>{virtual.topSpacerHeight > 0 && <TableRow aria-hidden><VirtualSpacerCell colSpan={4} height={virtual.topSpacerHeight} /></TableRow>}{virtual.visibleItems.map(material => <TableRow key={material.id} onMouseEnter={() => onHover?.(material)} onMouseLeave={() => onHover?.(null)}>
      <TableCell><div className="beta-material-name"><BlockIcon blockId={material.iconHint} /><span title={material.id}>{material.name}{material.materialStatus !== "item" && <small>{material.materialStatus === "no_item" ? "无直接对应物品" : "未知物品"}</small>}</span></div></TableCell>
      {breakdown && <><TableCell className="beta-material-quantity">{formatMaterialQuantity(material, material.blockCount * multiplier, asItems)}</TableCell><TableCell className="beta-material-quantity">{formatMaterialQuantity(material, material.containerItemCount * multiplier, asItems)}</TableCell></>}
      <TableCell className="beta-material-quantity">{formatMaterialQuantity(material, material.totalCount * multiplier, asItems)}</TableCell>
    </TableRow>)}{virtual.bottomSpacerHeight > 0 && <TableRow aria-hidden><VirtualSpacerCell colSpan={4} height={virtual.bottomSpacerHeight} /></TableRow>}{materials.length === 0 && <TableRow><TableCell colSpan={breakdown ? 4 : 2} className="material-list-empty-cell">没有符合条件的材料</TableCell></TableRow>}</TableBody>
  </Table></div>;
});

function QuantitySwitch({ checked, onChange }: { checked: boolean; onChange: (value: boolean) => void }) {
  return <label className="beta-material-check"><Checkbox  checked={checked} onCheckedChange={checked => onChange((checked === true))} />按个显示</label>;
}

/**
 * Shared material-list body used by both the modal dialog and the desktop child window.
 */
export function MaterialListContent({ data, onClose, currentFile, standalone = false, embedded = false, viewMode = "details", onViewModeChange }: {
  data: StatsData; onClose?: () => void; currentFile: string; standalone?: boolean; embedded?: boolean; viewMode?:"details"|"export"; onViewModeChange?:(mode:"details"|"export")=>void;
}) {
  const languageRevision = useSyncExternalStore(subscribeI18nChanges,getI18nRevision,getI18nRevision);
  const [contentMode,setContentMode] = useState(viewMode);
  useEffect(() => setContentMode(viewMode),[viewMode]);
  const [multiplier, setMultiplier] = useState(1);
  const [asItems, setAsItems] = useState(false);
  const [search, setSearch] = useState("");
  const [scopeMaterials, setScopeMaterials] = useState(data.materials);
  const [detailsLoading, setDetailsLoading] = useState(false);
  const [hoverItem, setHoverItem] = useState<MaterialItem | null>(null);
  const [mousePos, setMousePos] = useState({ x:0, y:0 });
  const detailsGeneration = useRef(0);
  const [includeContainerItems, setIncludeContainerItems] = useState(data.containerScan.enabled);
  const [exportFormat, setExportFormat] = useState<"svg" | "txt" | "html" | "csv">("svg");
  const [terminalRawMaterials, setTerminalRawMaterials] = useState(false);
  const [workbook, setWorkbook] = useState("scope:all");
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
  const [recipeStatus, setRecipeStatus] = useState("");
  const [exportStatus, setExportStatus] = useState("");
  const [error, setError] = useState("");
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
      .catch(error => { if (generation === detailsGeneration.current) { setScopeMaterials([]); setError(`读取材料明细失败：${error}`); } })
      .finally(() => { if (generation === detailsGeneration.current) setDetailsLoading(false); });
    return () => { detailsGeneration.current += 1; };
  },[currentFile,data,selectedScope,includeContainerItems,contentMode]);
  const detailRows = useMemo(() => filterMaterialItems(scopeMaterials.map(material => ({...material,name:materialDisplayName(material)})).sort((a,b) => b.totalCount-a.totalCount),search),[scopeMaterials,search,languageRevision]);
  const contentClassName = [standalone ? "material-list-window" : embedded ? "material-list-content-embedded" : "dialog-content",
    embedded ? "" : "subwindow-frame", "material-list-content", standalone ? "subwindow-frame-full material-list-content-standalone" : ""].filter(Boolean).join(" ");
  const options = useMemo(() => [{ label: "整个投影", value: "scope:all" },
    ...(data.regions || []).map(region => ({ label: `选定区域：${region.name}`, value: `region:${region.name}` })),
    ...(data.layers || []).map(layer => ({ label: `选定层级：y=${layer.world_y}（层 ${layer.layer}）`, value: `layer:${layer.layer}` }))], [data.regions, data.layers]);
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
      if (ok) { setExportStatus("材料列表已导出"); setAccessPassword(""); setAdminPassword(""); }
    } catch (error: any) { setError(`导出失败：${error}`); }
    finally { exportInFlight.current = false; setIsExporting(false); }
  };
  const handlePrepareRecipes = async () => {
    if (prepareRecipesInFlight.current) return;
    prepareRecipesInFlight.current = true; setIsPreparingRecipes(true); setRecipeStatus(""); setError("");
    try { await fetchMaterialRecipeCache(); setRecipeStatus("Minecraft 26.3 配方缓存已就绪"); }
    catch (error: any) { setError(`准备配方失败：${error}`); }
    finally { prepareRecipesInFlight.current = false; setIsPreparingRecipes(false); }
  };
  return <div className={contentClassName} onMouseMove={event => setMousePos({ x:event.clientX, y:event.clientY })} onMouseLeave={() => setHoverItem(null)}>
    {!embedded && <div className="subwindow-title-bar"><h3 className="subwindow-title">{contentMode === "details" ? "材料明细" : "导出材料列表"}</h3>
      {onClose && <Button variant="ghost" size="icon" className="subwindow-close-button" aria-label="关闭窗口" onClick={onClose}><X size={18} /></Button>}
    </div>}
    <div className="subwindow-body">
      <ScrollArea className="material-export-settings">
        {contentMode === "details" && <section className="material-details-section" aria-busy={detailsLoading}>
          <div className="material-export-grid">
            <label className="material-export-field"><span>统计范围</span><Dropdown value={workbook} options={options} onChange={setWorkbook} ariaLabel="材料明细范围" /></label>
            <label className="material-export-field"><span>建造份数</span><Input type="number" min={1} max={1000000} aria-label="材料明细份数" value={multiplier} onChange={event => setMultiplier(Math.min(1000000,Math.max(1,Math.floor(Number(event.target.value)||1))))} /></label>
          </div>
          <label className="beta-material-check"><Checkbox checked={includeContainerItems} onCheckedChange={checked => setIncludeContainerItems(checked === true)} />统计容器内物品</label>
          <header className="material-summary-header"><h3>材料明细</h3><span>{detailRows.length} / {scopeMaterials.length} 种</span></header>
          <div className="beta-material-options"><Input type="search" aria-label="搜索材料明细" placeholder="搜索名称或 ID" value={search} onChange={event => setSearch(event.target.value)} /><QuantitySwitch checked={asItems} onChange={setAsItems} /></div>
          {detailsLoading ? <p role="status">正在读取所选范围…</p> : <MaterialTable materials={detailRows} asItems={asItems} multiplier={multiplier} breakdown onHover={setHoverItem} />}
        </section>}
        {contentMode === "export" && <>
        <fieldset className="material-export-section" disabled={busy}>
          <legend>材料与格式</legend>
          <div className="material-export-grid">
            <label className="material-export-field"><span>统计范围</span><Dropdown value={workbook} options={options} onChange={setWorkbook} disabled={busy} ariaLabel="材料统计范围" /></label>
            <label className="material-export-field"><span>建造份数</span><Input type="number" min={1} max={1000000} aria-label="建造份数" value={multiplier}
              onChange={event => setMultiplier(Math.min(1000000, Math.max(1, Math.floor(Number(event.target.value) || 1))))} /></label>
            <label className="material-export-field"><span>导出格式</span><Dropdown value={exportFormat} options={[{ label: "SVG 艺术表", value: "svg" }, { label: "CSV", value: "csv" }, { label: "TXT", value: "txt" }, { label: "网页", value: "html" }]}
              onChange={value => setExportFormat(value as typeof exportFormat)} disabled={busy} ariaLabel="材料列表导出格式" /></label>
          </div>
          <div className="beta-material-options">
            <label className="beta-material-check"><Checkbox checked={includeContainerItems} disabled={busy} onCheckedChange={checked => setIncludeContainerItems(checked === true)} />统计容器内物品</label>
            <label className="beta-material-check"><Checkbox checked={terminalRawMaterials} disabled={busy} onCheckedChange={checked => setTerminalRawMaterials(checked === true)} />导出末端原料（本地配方）</label>
          </div>
          <p className="beta-statistics-status">份数与范围同时用于材料明细及实际导出；建造用量、容器物品与合计分别统计。</p>
        </fieldset>
        {terminalRawMaterials && <section className="material-export-section">
          <h4>末端原料配方</h4><p className="beta-statistics-status">复用本地 26.3 配方。未确定标签、循环和特殊配方会报告；烧制燃料另计。</p>
          <Button className="btn" title="点击后联网下载 Minecraft 客户端提取配方，下载可能较大" onClick={handlePrepareRecipes} disabled={busy}>{isPreparingRecipes ? "正在准备配方…" : "准备26.3配方（需联网）"}</Button>
          {recipeStatus && <p role="status">{recipeStatus}</p>}
        </section>}
        {exportFormat === "html" && <fieldset className="material-export-section" disabled={busy}>
          <legend>网页选项</legend>
          <label className="material-export-field"><span>使用方式</span><Dropdown value={webMode} options={[{ label: "单人离线网页（HTML）", value: "single" }, { label: "多人协作网页包（ZIP）", value: "multi" }]}
            onChange={value => setWebMode(value as typeof webMode)} disabled={busy} ariaLabel="网页使用方式" /></label>
          {webMode === "multi" && <>
            <div className="material-export-grid">
              <label className="material-export-field"><span>访问密码</span><Input type="password" autoComplete="new-password" aria-label="网页访问密码" value={accessPassword} onChange={event => setAccessPassword(event.target.value)} /></label>
              <label className="material-export-field"><span>管理员密码</span><Input type="password" autoComplete="new-password" aria-label="网页管理员密码" value={adminPassword} onChange={event => setAdminPassword(event.target.value)} /></label>
              <label className="material-export-field material-export-field-wide"><span>白名单玩家</span><Input aria-label="白名单玩家" placeholder="玩家名，以逗号或空格分隔" value={whitelist} onChange={event => setWhitelist(event.target.value)} /></label>
            </div>
            <div className="beta-material-options">
              <label className="beta-material-check"><Checkbox checked={whitelistEnabled} disabled={busy} onCheckedChange={checked => setWhitelistEnabled(checked === true)} />启用白名单</label>
              <label className="beta-material-check"><Checkbox checked={allowGuestReadonly} disabled={busy} onCheckedChange={checked => setAllowGuestReadonly(checked === true)} />允许访客只读</label>
              <label className="beta-material-check"><Checkbox checked={adminPageEnabled} disabled={busy} onCheckedChange={checked => setAdminPageEnabled(checked === true)} />启用管理页面</label>
            </div>
            <fieldset className="material-export-targets"><legend>服务平台（至少选择一项）</legend>
              {([["windows-x64", "Windows x64"], ["linux-x64", "Linux x64"], ["macos-x64", "macOS x64"], ["macos-arm64", "macOS ARM64"]] as const).map(([value, label]) =>
                <label className="beta-material-check" key={value}><Checkbox checked={webTargets.includes(value)} disabled={busy}
                  onCheckedChange={checked => setWebTargets(previous => checked === true ? [...previous.filter(target => target !== value), value] : previous.filter(target => target !== value))} />{label}</label>)}
            </fieldset>
          </>}
        </fieldset>}
        </>}
      </ScrollArea>
      <div className="material-export-footer">
        <Button variant="secondary" disabled={busy} onClick={() => { const mode = contentMode === "details" ? "export" : "details"; setContentMode(mode); onViewModeChange?.(mode); setHoverItem(null); }}>{contentMode === "details" ? "打开导出设置" : "返回材料明细"}</Button>
        {contentMode === "export" && <Button className="btn" onClick={handleExport} disabled={busy || !currentFile}>{isExporting ? "正在导出…" : "导出材料列表"}</Button>}
        {exportStatus && <span role="status">{exportStatus}</span>}
        {error && <pre className="subwindow-error" role="alert">{error}</pre>}
      </div>
    </div>
    <MaterialTooltip x={mousePos.x} y={mousePos.y} item={hoverItem} multiplier={multiplier} />
  </div>;
}

export function MaterialsDialog({ data, onClose, currentFile, viewMode = "details" }: { data: StatsData; onClose: () => void; currentFile: string; viewMode?:"details"|"export" }) {
  const [mode,setMode] = useState(viewMode);
  return (
    <Dialog title={mode === "details" ? "材料明细" : "导出材料列表"} onClose={onClose} width="lg" className="materials-dialog">
      <MaterialListContent data={data} currentFile={currentFile} embedded viewMode={mode} onViewModeChange={setMode} />
    </Dialog>
  );
}

export function StatisticsPage({ currentFile, theme }: any) {
  const languageRevision = useSyncExternalStore(subscribeI18nChanges,getI18nRevision,getI18nRevision);
  const applicationRender = useSyncExternalStore(
    subscribeApplicationRender,
    getApplicationRenderState,
    getApplicationRenderState,
  );
  const [data, setData] = useState<StatsData | null>(null);
  const [error, setError] = useState("");
  const [showMaterials, setShowMaterials] = useState(false);
  const [materialsViewMode,setMaterialsViewMode] = useState<"details"|"export">("details");
  const [materialSearch, setMaterialSearch] = useState("");
  const [includeContainerItems, setIncludeContainerItems] = useState(false);
  const [asItems, setAsItems] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const statsGeneration = useRef(0);
  const [showEnumerator, setShowEnumerator] = useState(false);
  const [showEnumeratorInfoEditor, setShowEnumeratorInfoEditor] = useState(false);
  const [enumeratorCollections, setEnumeratorCollections] = useState<EnumeratorCollection[]>([]);
  const [enumeratorError, setEnumeratorError] = useState("");
  const [enumeratorLoading, setEnumeratorLoading] = useState(false);
  const [enumeratorRules, setEnumeratorRules] = useState<string[]>([...DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES]);
  const [configLoading, setConfigLoading] = useState(true);
  const runtimeCollection = useMemo(() => data?.hasStructuralBlockCounts ? createRuntimeProjectionCollection(currentFile, data.structuralBlocks) : null,[currentFile,data]);
  useEffect(() => {
    let active = true;
    loadUserConfigMigratingLocalStorage().then(info => { if(active) setEnumeratorRules(normalizeStatisticsEnumeratorInfoRules(info.config.statistics_enumerator_info_rules)); })
      .catch(error => { if(active) setEnumeratorError(`读取自定义统计设置失败：${error}`); })
      .finally(() => { if(active) setConfigLoading(false); });
    return () => { active = false; };
  },[]);
  useEffect(() => {
    let active = true;
    if(!runtimeCollection) { setEnumeratorCollections([]); setEnumeratorLoading(false); return; }
    setEnumeratorLoading(true); setEnumeratorError("");
    loadEnumeratorCollections([runtimeCollection]).then(collections => { if(active) setEnumeratorCollections(collections); })
      .catch(error => { if(active) { setEnumeratorCollections([]); setEnumeratorError(`读取枚举集合失败：${error}`); } })
      .finally(() => { if(active) setEnumeratorLoading(false); });
    return () => { active = false; };
  },[runtimeCollection]);
  const enumeratorResults = useMemo(() => buildEnumeratorInfoResults(enumeratorRules,data,enumeratorCollections),[enumeratorRules,data,enumeratorCollections]);
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
    return <div className="beta-statistics-empty"><h2>统计</h2><p>先在投影库选择一个投影，这里会显示尺寸和所需材料。</p></div>;
  }
  const scan = data?.containerScan;
  return <React.Profiler id="statistics" onRender={recordStatisticsCommit}><div className="beta-statistics-page">
    <div className="beta-statistics-toolbar">
      <Button className="btn" onClick={() => openMaterialsWithWindowBehavior(currentFile, () => { setMaterialsViewMode("details"); setShowMaterials(true); })} disabled={!data || isLoading}>材料明细</Button>
      <Button className="btn" onClick={() => openMaterialsWithWindowBehavior(currentFile, () => { setMaterialsViewMode("export"); setShowMaterials(true); }, "export")} disabled={!data || isLoading}>导出材料列表</Button>
      <Button variant="secondary" disabled={!data} onClick={() => openEnumeratorWithWindowBehavior(currentFile,() => setShowEnumerator(true))}>打开枚举器</Button>
      <label className="beta-material-check"><Checkbox  checked={includeContainerItems} onCheckedChange={checked => setIncludeContainerItems((checked === true))} />统计容器内物品</label>
      <QuantitySwitch checked={asItems} onChange={setAsItems} />
      <span className="beta-statistics-file" title={currentFile}>{currentFile.split(/[\\/]/).pop()}</span>
      <Button className="btn" onClick={loadStats} disabled={isLoading}>{isLoading ? "正在分析…" : "重新分析"}</Button>
    </div>
    {includeContainerItems && scan && <div className="beta-statistics-status" role="status">已扫描 {scan.containers_scanned} 个容器、{scan.item_stacks_scanned} 堆物品。</div>}
    {error && <pre className="subwindow-error" role="alert">{error}</pre>}
    {!!scan?.warnings?.length && <details className="beta-statistics-warnings"><summary>有 {scan.warnings.length} 项容器数据需要注意</summary><pre>{scan.warnings.slice(0, 40).join("\n")}</pre></details>}
    <div className="beta-statistics-panes">
      <section className="beta-statistics-structure"><h2>结构分析</h2>
        {data ? <ScrollArea className="beta-statistics-facts-scroll"><dl className="beta-statistics-facts">
          <ReadOnlyRow label="非空气方块" value={data.totalNonAirBlocks.toLocaleString("zh-CN")} />
          <ReadOnlyRow label="区域数量" value={data.regionCount} />
          <ReadOnlyRow label="包围尺寸" value={`${data.enclosingSize.x} × ${data.enclosingSize.y} × ${data.enclosingSize.z}`} />
          <ReadOnlyRow label="空间占用率" value={`${(data.density * 100).toFixed(2)}%`} />
          <ReadOnlyRow label="结构类型" value={data.buildingType} />
          <ReadOnlyRow label="红石比例" value={data.redstoneRatio === null ? "未提供" : `${(data.redstoneRatio * 100).toFixed(2)}%`} />
          <ReadOnlyRow label="流体比例" value={data.fluidRatio === null ? "未提供" : `${(data.fluidRatio * 100).toFixed(2)}%`} />
          <ReadOnlyRow label="实体数量" value={data.entityCount} />
        </dl>
          <section className="statistics-enumerator-panel" aria-busy={enumeratorLoading || configLoading}>
            <header className="material-summary-header"><h3>自定义统计</h3><Button variant="secondary" disabled={configLoading || enumeratorLoading || !enumeratorCollections.length || !!enumeratorError} onClick={() => setShowEnumeratorInfoEditor(true)}>编辑规则</Button></header>
            {enumeratorLoading || configLoading ? <p role="status">正在读取规则与集合…</p> : enumeratorError ? <pre role="alert">{enumeratorError}</pre>
              : enumeratorResults.map((item,index) => <div key={index} className="statistics-enumerator-row"><div className="statistics-enumerator-row-main"><span>{item.title}</span><strong>{item.error ? "错误" : item.value}</strong></div>{item.error && <p className="statistics-enumerator-row-error">{item.error}</p>}</div>)}
          </section>
        </ScrollArea> : <p className="beta-statistics-status">{renderPending ? "正在读取投影统计…" : isLoading ? "正在分析…" : "暂无统计数据"}</p>}
      </section>
      <section className="beta-statistics-materials"><header className="material-summary-header"><h2>材料列表</h2>{data && <span>{topMaterials.length} / {data.materials.length} 种</span>}</header>
        <Input className="input material-summary-search" type="search" aria-label="搜索材料" placeholder="搜索材料名称或 ID" value={materialSearch} onChange={event => setMaterialSearch(event.target.value)} />
        {data ? <MaterialTable materials={topMaterials} asItems={asItems} /> : <p className="beta-statistics-status">{isLoading ? "正在统计材料…" : "暂无材料数据"}</p>}
      </section>
    </div>
    {showEnumerator && <EnumeratorDialog currentFile={currentFile} theme={theme} runtimeCollection={runtimeCollection} onClose={() => setShowEnumerator(false)} />}
    {showEnumeratorInfoEditor && <EnumeratorInfoEditorDialog rules={enumeratorRules} data={data} collections={enumeratorCollections} onClose={() => setShowEnumeratorInfoEditor(false)} onSave={saveEnumeratorRules} />}
    {showMaterials && data && <MaterialsDialog data={data} onClose={() => setShowMaterials(false)} currentFile={currentFile} viewMode={materialsViewMode} />}
  </div></React.Profiler>;
}

function ReadOnlyRow({ label, value }: { label: string; value: string | number }) {
  return <div><dt>{label}</dt><dd>{value}</dd></div>;
}
