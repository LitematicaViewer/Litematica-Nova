import React, { useEffect, useMemo, useState } from "react";
import {
  exportMaterials,
  formatMaterialUnits,
  loadMaterialsScope,
  loadStructureStats,
  MaterialItem,
  StatsData,
} from "../../business/facade";
import { BlockIcon } from "../components/BlockIcon";
import { Dropdown } from "../components/Dropdown";

function MaterialTooltip({ x, y, item, multiplier }: { x: number; y: number; item: MaterialItem | null; multiplier: number }) {
  if (!item) return null;
  const total = Math.max(0, Math.floor(item.totalCount * multiplier));

  return (
    <div
      className="materials-tooltip"
      style={{ left: x + 15, top: y + 15 }}
    >
      <div className="nova-row-tight nova-strong">
        <BlockIcon blockId={item.iconHint} />
        <span>{item.name}</span>
      </div>
      <div className="nova-muted nova-small">{item.id}</div>
      <div>合计：{total} = {formatMaterialUnits(total)}</div>
      {item.containerItemCount > 0 && (
        <div className="nova-muted">
          方块 {item.blockCount * multiplier} / 容器物品 {item.containerItemCount * multiplier}
        </div>
      )}
    </div>
  );
}

export function MaterialsDialog({
  data,
  onClose,
  currentFile,
}: {
  data: StatsData;
  onClose: () => void;
  currentFile: string;
}) {
  const [multiplier, setMultiplier] = useState(1);
  const [includeContainerItems, setIncludeContainerItems] = useState(false);
  const [workbook, setWorkbook] = useState("scope:all");
  const [materials, setMaterials] = useState<MaterialItem[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState("");
  const [hoverItem, setHoverItem] = useState<MaterialItem | null>(null);
  const [mousePos, setMousePos] = useState({ x: 0, y: 0 });

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

  const scopeArgs = (value: string): string[] => {
    if (value.startsWith("scope:")) return ["--scope", value.split(":")[1]];
    if (value.startsWith("region:")) return ["--region", value.slice("region:".length)];
    if (value.startsWith("layer:")) return ["--layer", value.split(":")[1]];
    return ["--scope", "all"];
  };

  const loadMats = async (value: string, includeContainers = includeContainerItems) => {
    setIsLoading(true);
    setError("");
    try {
      const next = await loadMaterialsScope(currentFile, scopeArgs(value), includeContainers);
      setMaterials(next.sort((a, b) => b.totalCount - a.totalCount));
    } catch (err: any) {
      setError(`获取材料失败：${err}`);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadMats(workbook);
  }, [workbook, includeContainerItems]);

  const handleExport = async () => {
    try {
      const ok = await exportMaterials(currentFile, materials, multiplier);
      if (ok) alert("导出成功");
    } catch (err: any) {
      setError(`导出失败：${err}`);
    }
  };

  return (
    <div className="dialog-overlay" onMouseMove={(event) => setMousePos({ x: event.clientX, y: event.clientY })}>
      <div className="dialog-content materials-dialog">
        <div className="materials-dialog-header">
          <h3>材料列表</h3>
          <button className="btn materials-dialog-close" onClick={onClose}>×</button>
        </div>

        <div className="materials-dialog-body">
          <div className="nova-row-tight">
            <span>范围</span>
            <Dropdown value={workbook} options={options} onChange={setWorkbook} />
            <button className="btn" onClick={() => loadMats(workbook)} disabled={isLoading}>重新加载</button>
            <button className="btn" onClick={handleExport} disabled={isLoading || materials.length === 0}>导出材料列表</button>
            <div className="nova-input-flex" />
            <span>倍数</span>
            <input
              type="number"
              className="input properties-number-xs"
              value={multiplier}
              onChange={(event) => setMultiplier(Math.max(1, parseInt(event.target.value, 10) || 1))}
            />
          </div>

          <label className="nova-inline-label">
            <input
              type="checkbox"
              checked={includeContainerItems}
              onChange={(event) => setIncludeContainerItems(event.target.checked)}
            />
            统计容器内物品
          </label>

          <div className="nova-muted nova-small">
            {isLoading ? "正在分析..." : includeContainerItems ? "已包含容器 BlockEntity/TileEntity 内物品。" : "当前仅统计投影方块。"}
          </div>
          {error && <pre className="nova-error">{error}</pre>}

          <div className="materials-table-wrap">
            <table className="nova-table materials-table">
              <thead>
                <tr>
                  <th className="materials-icon-col">图标</th>
                  <th>名称</th>
                  <th className="nova-table-number">方块</th>
                  <th className="nova-table-number">容器物品</th>
                  <th className="nova-table-number">合计</th>
                </tr>
              </thead>
              <tbody>
                {materials.map((material, index) => (
                  <tr
                    key={material.id}
                    className={index % 2 === 0 ? "materials-row-even" : ""}
                    onMouseEnter={() => setHoverItem(material)}
                    onMouseLeave={() => setHoverItem(null)}
                  >
                    <td className="materials-icon-cell"><BlockIcon blockId={material.iconHint} /></td>
                    <td>{material.name}</td>
                    <td className="nova-table-number">{material.blockCount * multiplier}</td>
                    <td className="nova-table-number">{material.containerItemCount * multiplier}</td>
                    <td className="nova-table-number">{material.totalCount * multiplier}</td>
                  </tr>
                ))}
                {materials.length === 0 && !isLoading && (
                  <tr>
                    <td colSpan={5} className="materials-empty">暂无材料数据</td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
        <div className="materials-dialog-footer" />
      </div>
      <MaterialTooltip x={mousePos.x} y={mousePos.y} item={hoverItem} multiplier={multiplier} />
    </div>
  );
}

export function StatisticsPage({ currentFile }: any) {
  const [data, setData] = useState<StatsData | null>(null);
  const [error, setError] = useState("");
  const [showMaterials, setShowMaterials] = useState(false);
  const [includeContainerItems, setIncludeContainerItems] = useState(false);

  const loadStats = async () => {
    if (!currentFile) return;
    setError("");
    try {
      const next = await loadStructureStats(currentFile, includeContainerItems);
      setData(next);
    } catch (err: any) {
      setError(String(err));
    }
  };

  useEffect(() => {
    loadStats();
  }, [currentFile, includeContainerItems]);

  if (!currentFile) {
    return (
      <div className="nova-empty-state">
        <h2>统计</h2>
        <p>请先在投影库中选择一个 .litematic 文件。</p>
      </div>
    );
  }

  const topMaterials = data ? [...data.materials].sort((a, b) => b.totalCount - a.totalCount) : [];
  const scan = data?.containerScan;

  return (
    <div className="nova-page">
      <div className="nova-toolbar">
        <button className="btn" onClick={() => setShowMaterials(true)} disabled={!data}>材料列表</button>
        <label className="nova-inline-label">
          <input
            type="checkbox"
            checked={includeContainerItems}
            onChange={(event) => setIncludeContainerItems(event.target.checked)}
          />
          统计容器内物品
        </label>
        <div className="nova-muted nova-small nova-flex-1">
          {includeContainerItems && scan
            ? `容器扫描：${scan.containers_scanned} 个容器，${scan.item_stacks_scanned} 个物品堆。`
            : "当前仅统计投影方块。"}
        </div>
        <div className="nova-file-chip-wide nova-tiny" title={currentFile}>
          {currentFile}
        </div>
        <button className="btn" onClick={loadStats}>重新分析</button>
      </div>

      {error && <pre className="nova-error">{error}</pre>}

      {scan?.warnings?.length ? (
        <details>
          <summary className="nova-summary">容器扫描 warning（{scan.warnings.length}）</summary>
          <pre className="nova-pre nova-pre-short">
            {scan.warnings.slice(0, 40).join("\n")}
          </pre>
        </details>
      ) : null}

      <div className="statistics-grid">
        <div className="group-box statistics-panel-scroll">
          <div className="group-box-title">结构分析</div>
          {data ? (
            <div className="nova-stack-compact">
              <ReadOnlyRow label="非空气方块" value={data.totalNonAirBlocks} />
              <ReadOnlyRow label="区域数量" value={data.regionCount} />
              <ReadOnlyRow label="包围尺寸" value={`${data.enclosingSize.x}x${data.enclosingSize.y}x${data.enclosingSize.z}`} />
              <ReadOnlyRow label="密度" value={`${(data.density * 100).toFixed(2)}%`} />
              <ReadOnlyRow label="结构类型" value={data.buildingType} />
              <ReadOnlyRow label="红石偏度" value={`${(data.redstoneRatio * 100).toFixed(2)}%`} />
              <ReadOnlyRow label="流体偏度" value={`${(data.fluidRatio * 100).toFixed(2)}%`} />
              <ReadOnlyRow label="实体种类" value={data.entityCount} />
            </div>
          ) : (
            <div className="nova-empty-compact">加载中...</div>
          )}
        </div>

        <div className="group-box nova-flex-column-fill">
          <div className="group-box-title">主要材料</div>
          <div className="statistics-material-list">
            {data ? (
              topMaterials.map((material) => (
                <div key={material.id} className="statistics-material-row">
                  {material.name} <span className="nova-muted">x</span> {material.totalCount}
                  {includeContainerItems && material.containerItemCount > 0 && (
                    <span className="statistics-container-count">
                      方块 {material.blockCount} / 容器 {material.containerItemCount}
                    </span>
                  )}
                </div>
              ))
            ) : (
              <div className="nova-muted">加载中...</div>
            )}
          </div>
        </div>
      </div>

      {showMaterials && data && (
        <MaterialsDialog data={data} onClose={() => setShowMaterials(false)} currentFile={currentFile} />
      )}
    </div>
  );
}

function ReadOnlyRow({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="form-row statistics-readonly-row">
      <div className="form-label statistics-readonly-label">{label}</div>
      <div className="form-field statistics-readonly-field">
        <input className="input nova-input-full" value={value} readOnly />
      </div>
    </div>
  );
}


