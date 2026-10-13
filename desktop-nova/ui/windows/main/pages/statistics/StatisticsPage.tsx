import { Checkbox } from "../../../../components/ui/checkbox";
import { TableHeader, TableRow, TableHead, TableBody, TableCell, Table } from "../../../../components/ui/table";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Dialog } from "../../../../components/Dialog";
import { X } from "lucide-react";
﻿import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  exportMaterials,
  formatMaterialQuantity,
  hasInventoryMaterialSemantics,
  loadMaterialsScope,
  loadStructureStats,
  mapStructureStats,
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
import { BlockIcon } from "../../../../components/BlockIcon";
import { Dropdown } from "../../../../components/Dropdown";
import "./materials.css";

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

/**
 * Opens the material list according to the user's configured window behavior.
 */
export async function openMaterialsWithWindowBehavior(currentFile: string, showOverlay: () => void): Promise<void> {
  const info = await loadUserConfigMigratingLocalStorage().catch(() => null);
  const behavior = normalizeMaterialListWindowBehavior(info?.config.material_list_window_behavior);
  if (behavior === "independent_window") {
    try {
      await openMaterialListWindow(currentFile);
      return;
    } catch {
      // Browser preview cannot create a desktop window, so keep the in-window dialog as fallback.
    }
  }
  showOverlay();
}

function MaterialTable({ materials, asItems, multiplier = 1, breakdown = false, onHover }: {
  materials: MaterialItem[];
  asItems: boolean;
  multiplier?: number;
  breakdown?: boolean;
  onHover?: (material: MaterialItem | null) => void;
}) {
  if (!breakdown) return <ul className="material-summary-list" aria-label="主要材料">
    {materials.map(material => <li key={material.id} onMouseEnter={() => onHover?.(material)} onMouseLeave={() => onHover?.(null)}>
      <BlockIcon blockId={material.iconHint} />
      <span className="material-summary-name" title={material.id}>{material.name}
        {material.materialStatus !== "item" && <small>{material.materialStatus === "no_item" ? "无直接对应物品" : "未知物品"}</small>}
      </span>
      <span className="material-summary-quantity">{formatMaterialQuantity(material, material.totalCount * multiplier, asItems)}</span>
    </li>)}
    {materials.length === 0 && <li className="material-summary-empty">没有符合条件的材料</li>}
  </ul>;
  return <Table className="material-list-table beta-material-table">
    <TableHeader><TableRow><TableHead scope="col">材料</TableHead>{breakdown && <><TableHead scope="col">建造用量</TableHead><TableHead scope="col">容器物品</TableHead></>}<TableHead scope="col">{breakdown ? "合计" : "数量"}</TableHead></TableRow></TableHeader>
    <TableBody>{materials.map(material => <TableRow key={material.id} onMouseEnter={() => onHover?.(material)} onMouseLeave={() => onHover?.(null)}>
      <TableCell><div className="beta-material-name"><BlockIcon blockId={material.iconHint} /><span title={material.id}>{material.name}{material.materialStatus !== "item" && <small>{material.materialStatus === "no_item" ? "无直接对应物品" : "未知物品"}</small>}</span></div></TableCell>
      {breakdown && <><TableCell className="beta-material-quantity">{formatMaterialQuantity(material, material.blockCount * multiplier, asItems)}</TableCell><TableCell className="beta-material-quantity">{formatMaterialQuantity(material, material.containerItemCount * multiplier, asItems)}</TableCell></>}
      <TableCell className="beta-material-quantity">{formatMaterialQuantity(material, material.totalCount * multiplier, asItems)}</TableCell>
    </TableRow>)}{materials.length === 0 && <TableRow><TableCell colSpan={breakdown ? 4 : 2} className="material-list-empty-cell">没有符合条件的材料</TableCell></TableRow>}</TableBody>
  </Table>;
}

function QuantitySwitch({ checked, onChange }: { checked: boolean; onChange: (value: boolean) => void }) {
  return <label className="beta-material-check"><Checkbox  checked={checked} onCheckedChange={checked => onChange((checked === true))} />按个显示</label>;
}

/**
 * Shared material-list body used by both the modal dialog and the desktop child window.
 */
export function MaterialListContent({
  data,
  onClose,
  currentFile,
  standalone = false,
  embedded = false,
}: {
  data: StatsData;
  onClose?: () => void;
  currentFile: string;
  standalone?: boolean;
  embedded?: boolean;
}) {
  const [multiplier, setMultiplier] = useState(1);
  const [includeContainerItems, setIncludeContainerItems] = useState(data.containerScan.enabled);
  const [asItems, setAsItems] = useState(false);
  const [search, setSearch] = useState("");
  const requestGeneration = useRef(0);
  const [workbook, setWorkbook] = useState("scope:all");
  const [materials, setMaterials] = useState<MaterialItem[]>(
    () => [...(data.materials || [])].sort((a, b) => b.totalCount - a.totalCount),
  );
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState("");
  const [hoverItem, setHoverItem] = useState<MaterialItem | null>(null);
  const [mousePos, setMousePos] = useState({ x: 0, y: 0 });
  const contentClassName = [
    standalone ? "material-list-window" : embedded ? "material-list-content-embedded" : "dialog-content",
    embedded ? "" : "subwindow-frame",
    "material-list-content",
    standalone ? "subwindow-frame-full material-list-content-standalone" : "",
  ].filter(Boolean).join(" ");

  const options = useMemo(() => {
    const next = [{ label: "整个投影", value: "scope:all" }];
    for (const region of data.regions || []) {
      next.push({ label: `选定区域：${region.name}`, value: `region:${region.name}` });
    }
    for (const layer of data.layers || []) {
      next.push({ label: `选定层级：y=${layer.world_y}（层 ${layer.layer}）`, value: `layer:${layer.layer}` });
    }
    return next;
  }, [data.regions, data.layers]);

  const materialScope = (value: string) => {
    if (value.startsWith("region:")) {
      return { kind: "region" as const, value: value.slice("region:".length) };
    }
    if (value.startsWith("layer:")) {
      return { kind: "layer" as const, value: value.split(":")[1] };
    }
    return { kind: "all" as const, value: null };
  };

  const materialsFromSummary = (value: string): MaterialItem[] => {
    if (value === "scope:all") return data.materials || [];
    return (data.materials || []).map((material) => {
      const count = value.startsWith("region:")
        ? material.regionCounts.find((entry) => entry.region === value.slice("region:".length))?.count || 0
        : material.layerCounts.find((entry) => entry.layer === Number(value.slice("layer:".length)))?.count || 0;
      return {
        ...material,
        count,
        blockCount: count,
        containerItemCount: 0,
        totalCount: count,
      };
    }).filter((material) => material.totalCount > 0);
  };

  const loadMats = async (value: string, includeContainers = includeContainerItems) => {
    const generation = ++requestGeneration.current;
    setIsLoading(true);
    setError("");
    try {
      if (!includeContainers && !data.containerScan.enabled) {
        setMaterials([...materialsFromSummary(value)].sort((a, b) => b.totalCount - a.totalCount));
        return;
      }
      const next = await loadMaterialsScope(currentFile, materialScope(value), includeContainers);
      if (generation === requestGeneration.current) setMaterials(next.sort((a, b) => b.totalCount - a.totalCount));
    } catch (err: any) {
      if (generation === requestGeneration.current) setError(`获取材料失败：${err}`);
    } finally {
      if (generation === requestGeneration.current) setIsLoading(false);
    }
  };

  useEffect(() => {
    void loadMats(workbook);
    return () => { requestGeneration.current += 1; };
  }, [workbook, includeContainerItems, currentFile, data]);

  const [exportStatus, setExportStatus] = useState("");
  const filteredMaterials = materials.filter(material => `${material.name} ${material.id}`.toLowerCase().includes(search.trim().toLowerCase()));
  const handleExport = async () => {
    setExportStatus("");
    try {
      const ok = await exportMaterials(currentFile, materials, multiplier);
      if (ok) setExportStatus("材料列表已导出");
    } catch (err: any) {
      setError(`导出失败：${err}`);
    }
  };

  return (
      <div
        className={contentClassName}
        onMouseMove={(event) => setMousePos({ x: event.clientX, y: event.clientY })}
      >
        {!embedded && <div className="subwindow-title-bar">
          <h3 className="subwindow-title">材料列表</h3>
          {onClose && <Button variant="ghost" size="icon" className="subwindow-close-button" aria-label="关闭窗口" onClick={onClose}><X size={18} /></Button>}
        </div>}

        <div className="subwindow-body">
          <div className="subwindow-toolbar">
            <span>范围</span>
            <Dropdown value={workbook} options={options} onChange={setWorkbook} ariaLabel="材料统计范围" />
            <Button className="btn" onClick={() => loadMats(workbook)} disabled={isLoading}>重新加载</Button>
            <Button className="btn" onClick={handleExport} disabled={isLoading || materials.length === 0}>导出材料列表</Button>
            <div className="subwindow-toolbar-spacer" />
            <span>建造份数</span>
            <Input
              type="number"
              min={1}
              max={1000000}
              aria-label="建造份数"
              className="input material-list-count-input"
              value={multiplier}
              onChange={(event) => setMultiplier(Math.min(1000000, Math.max(1, parseInt(event.target.value, 10) || 1)))}
            />
          </div>

          <div className="beta-material-options">
          <Input className="input beta-material-search" type="search" aria-label="搜索材料" placeholder="搜索材料名称或 ID" value={search} onChange={event => setSearch(event.target.value)} />
          <QuantitySwitch checked={asItems} onChange={setAsItems} />
          <label className="beta-material-check">
            <Checkbox

              checked={includeContainerItems}
              onCheckedChange={(checked) => setIncludeContainerItems((checked === true))}
            />
            统计容器内物品
          </label>
          </div>

          <div className="subwindow-status-text">
            {isLoading ? "正在分析..." : includeContainerItems ? "已包含箱子等容器中的物品。" : "当前仅统计建造所需材料。"}
          </div>
          {exportStatus && <div className="subwindow-status-text" role="status">{exportStatus}</div>}
          {error && <pre className="subwindow-error" role="alert">{error}</pre>}

          <ScrollArea horizontal className="material-list-table-wrap">
            <MaterialTable materials={filteredMaterials} asItems={asItems} multiplier={multiplier} breakdown onHover={setHoverItem} />
          </ScrollArea>
        </div>
        <MaterialTooltip x={mousePos.x} y={mousePos.y} item={hoverItem} multiplier={multiplier} />
      </div>
  );
}

export function MaterialsDialog({ data, onClose, currentFile }: { data: StatsData; onClose: () => void; currentFile: string }) {
  return (
    <Dialog title="材料列表" onClose={onClose} width="lg" className="materials-dialog">
      <MaterialListContent data={data} currentFile={currentFile} embedded />
    </Dialog>
  );
}

export function StatisticsPage({ currentFile }: any) {
  const applicationRender = useSyncExternalStore(
    subscribeApplicationRender,
    getApplicationRenderState,
    getApplicationRenderState,
  );
  const [data, setData] = useState<StatsData | null>(null);
  const [error, setError] = useState("");
  const [showMaterials, setShowMaterials] = useState(false);
  const [includeContainerItems, setIncludeContainerItems] = useState(false);
  const [asItems, setAsItems] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const statsGeneration = useRef(0);

  const renderOperation = useMemo(
    () => findRenderOperationForSource(currentFile, applicationRender),
    [applicationRender.operations, currentFile],
  );
  const applicationAnalysis = hasInventoryMaterialSemantics(renderOperation?.analysis) ? renderOperation!.analysis : null;
  const applicationAnalysisKey = applicationAnalysis ? renderOperation?.operationId ?? "analysis" : "";
  const renderPending = isApplicationRenderPending(renderOperation);
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
  const topMaterials = data ? [...data.materials].sort((a, b) => b.totalCount - a.totalCount) : [];
  const scan = data?.containerScan;
  return <div className="beta-statistics-page">
    <div className="beta-statistics-toolbar">
      <Button className="btn" onClick={() => applicationAnalysis ? setShowMaterials(true) : openMaterialsWithWindowBehavior(currentFile, () => setShowMaterials(true))} disabled={!data || isLoading}>材料列表与导出</Button>
      <label className="beta-material-check"><Checkbox  checked={includeContainerItems} onCheckedChange={checked => setIncludeContainerItems((checked === true))} />统计容器内物品</label>
      <QuantitySwitch checked={asItems} onChange={setAsItems} />
      <span className="beta-statistics-file" title={currentFile}>{currentFile.split(/[\\/]/).pop()}</span>
      <Button className="btn" onClick={loadStats} disabled={isLoading}>{isLoading ? "正在分析…" : "重新分析"}</Button>
    </div>
    <div className="beta-statistics-status" role="status">{includeContainerItems && scan ? `已扫描 ${scan.containers_scanned} 个容器、${scan.item_stacks_scanned} 堆物品。` : "按 Minecraft 26.3 实际物品归类；结构方块数与材料用量分别统计。"}</div>
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
          {data.nonItemBlocks.length > 0 && <details className="beta-statistics-non-items">
            <summary>不计入材料的方块状态 · {data.nonItemBlocks.reduce((sum, block) => sum + block.count, 0).toLocaleString("zh-CN")} 个</summary>
            <p>已计入结构方块数；没有独立物品或尚未识别的状态不作为材料计费。</p>
            <ul>{data.nonItemBlocks.map(block => <li key={block.id}>
              <span title={block.id}>{block.name}{block.reason === "unknown" && <small>物品映射未知</small>}</span>
              <span>{block.count.toLocaleString("zh-CN")}</span>
            </li>)}</ul>
          </details>}
        </ScrollArea> : <p className="beta-statistics-status">{renderPending ? "正在读取投影统计…" : isLoading ? "正在分析…" : "暂无统计数据"}</p>}
      </section>
      <section className="beta-statistics-materials"><header className="material-summary-header"><h2>主要材料</h2>{data && <span>{topMaterials.length} 种</span>}</header>
        <ScrollArea className="material-summary-scroll">{data ? <MaterialTable materials={topMaterials} asItems={asItems} /> : <p className="beta-statistics-status">{isLoading ? "正在统计材料…" : "暂无材料数据"}</p>}</ScrollArea>
      </section>
    </div>
    {showMaterials && data && <MaterialsDialog data={data} onClose={() => setShowMaterials(false)} currentFile={currentFile} />}
  </div>;
}

function ReadOnlyRow({ label, value }: { label: string; value: string | number }) {
  return <div><dt>{label}</dt><dd>{value}</dd></div>;
}
