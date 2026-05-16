import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  exportMaterialsArtTable,
  exportMaterialsCsv,
  loadMaterialsScope,
  loadStructureStats,
  MaterialItem,
  StatsData,
} from "../../../../../src/business/facade";
import {
  loadUserConfigMigratingLocalStorage,
  normalizeMaterialListWindowBehavior,
  openMaterialListWindow,
} from "../../../../../src/business/facade";
import { BlockIcon } from "../../../../components/BlockIcon";

function MaterialTooltip({ x, y, item, multiplier }: { x: number; y: number; item: MaterialItem | null; multiplier: number }) {
  const popupRef = useRef<HTMLDivElement | null>(null);
  const total = item ? Math.max(0, Math.floor(item.totalCount * multiplier)) : 0;
  const stacks = Math.floor(total / 64);
  const remainder = total % 64;
  const shulkerBoxes = (total / 1728).toFixed(2);
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
    popup.style.setProperty("--material-list-popup-left", `${position.left}px`);
    popup.style.setProperty("--material-list-popup-top", `${position.top}px`);
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
        总计： <strong>&nbsp;{total}&nbsp;</strong> = <strong>&nbsp;{stacks}&nbsp;</strong> × 64 + <strong>&nbsp;{remainder}&nbsp;</strong> = <strong>&nbsp;{shulkerBoxes}&nbsp;</strong> 潜影盒
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

/**
 * Shared material-list body used by both the modal dialog and the desktop child window.
 */
export function MaterialListContent({
  data,
  onClose,
  currentFile,
  standalone = false,
}: {
  data: StatsData;
  onClose?: () => void;
  currentFile: string;
  standalone?: boolean;
}) {
  const [multiplier, setMultiplier] = useState(1);
  const [includeContainerItems, setIncludeContainerItems] = useState(false);
  const [workbook, setWorkbook] = useState("scope:all");
  const [materials, setMaterials] = useState<MaterialItem[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState("");
  const [hoverItem, setHoverItem] = useState<MaterialItem | null>(null);
  const [mousePos, setMousePos] = useState({ x: 0, y: 0 });
  const contentClassName = [
    standalone ? "material-list-window" : "dialog-content",
    "subwindow-frame",
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

  const handleExportArtTable = async () => {
    try {
      const ok = await exportMaterialsArtTable(currentFile, materials, multiplier);
      if (ok) alert("写入艺术表成功");
    } catch (err: any) {
      setError(`写入艺术表失败：${err}`);
    }
  };

  const handleExportCsv = async () => {
    try {
      const ok = await exportMaterialsCsv(currentFile, materials, multiplier);
      if (ok) alert("写入 CSV 成功");
    } catch (err: any) {
      setError(`写入 CSV 失败：${err}`);
    }
  };

  return (
      <div
        className={contentClassName}
        onMouseMove={(event) => setMousePos({ x: event.clientX, y: event.clientY })}
      >
        <div className="subwindow-title-bar">
          <h3 className="subwindow-title">材料列表</h3>
          {onClose && <button className="btn subwindow-close-button" type="button" aria-label="关闭窗口" onClick={onClose}>×</button>}
        </div>

        <div className="subwindow-body">
          <div className="material-list-control-stack">
            <div className="material-list-control-row material-list-control-row-workbook">
              <span className="material-list-control-label">工作簿</span>
              <select
                className="input material-list-workbook-select"
                value={workbook}
                onChange={(event) => setWorkbook(event.target.value)}
              >
                {options.map((option) => (
                  <option key={option.value} value={option.value}>{option.label}</option>
                ))}
              </select>
            </div>

            <div className="material-list-control-row material-list-control-row-actions">
              <button className="btn" onClick={() => loadMats(workbook)} disabled={isLoading}>重新加载</button>
              <button className="btn" onClick={handleExportArtTable} disabled={isLoading || materials.length === 0}>写入艺术表</button>
              <button className="btn" onClick={handleExportCsv} disabled={isLoading || materials.length === 0}>写入CSV</button>
              <label className="material-list-multiplier-control">
                <span>倍数</span>
                <input
                  type="number"
                  className="input material-list-count-input"
                  value={multiplier}
                  onChange={(event) => setMultiplier(Math.max(1, parseInt(event.target.value, 10) || 1))}
                />
              </label>
            </div>

            <div className="material-list-control-row material-list-control-row-toggles">
              <label className="subwindow-check-row">
                <input
                  type="checkbox"
                  checked={includeContainerItems}
                  onChange={(event) => setIncludeContainerItems(event.target.checked)}
                />
                统计容器
              </label>
              <label className="subwindow-check-row material-list-disabled-toggle" title="统计实体尚未实现">
                <input type="checkbox" disabled />
                统计实体
              </label>
            </div>
          </div>

          <div className="subwindow-status-text">
            {isLoading ? "正在分析..." : includeContainerItems ? "已包含容器 BlockEntity/TileEntity 内物品。" : "当前仅统计投影方块。"}
          </div>
          {error && <pre className="subwindow-error">{error}</pre>}

          <div className="material-list-table-wrap">
            <table className="material-list-table">
              <thead>
                <tr>
                  <th className="material-list-icon-col">图标</th>
                  <th>名称</th>
                  <th className="material-list-number">方块</th>
                  <th className="material-list-number">容器物品</th>
                  <th className="material-list-total-col">合计</th>
                </tr>
              </thead>
              <tbody>
                {materials.map((material) => (
                  <tr
                    key={material.id}
                    onMouseEnter={() => setHoverItem(material)}
                    onMouseLeave={() => setHoverItem(null)}
                  >
                    <td className="material-list-icon-cell"><BlockIcon blockId={material.iconHint} /></td>
                    <td>{material.name}</td>
                    <td className="material-list-number">{material.blockCount * multiplier}</td>
                    <td className="material-list-number">{material.containerItemCount * multiplier}</td>
                    <td className="material-list-number">{material.totalCount * multiplier}</td>
                  </tr>
                ))}
                {materials.length === 0 && !isLoading && (
                  <tr>
                    <td colSpan={5} className="material-list-empty-cell">暂无材料数据</td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
        <MaterialTooltip x={mousePos.x} y={mousePos.y} item={hoverItem} multiplier={multiplier} />
      </div>
  );
}

export function MaterialsDialog({ data, onClose, currentFile }: { data: StatsData; onClose: () => void; currentFile: string }) {
  return (
    <div className="dialog-overlay">
      <MaterialListContent data={data} onClose={onClose} currentFile={currentFile} />
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
      <div className="statistics-empty-state">
        <h2>统计</h2>
        <p>请先在投影库中选择一个 .litematic 文件。</p>
      </div>
    );
  }

  const topMaterials = data ? [...data.materials].sort((a, b) => b.totalCount - a.totalCount) : [];
  const featuredMaterials = topMaterials.slice(0, 5);
  const totalMaterialCount = topMaterials.reduce((sum, material) => sum + material.totalCount, 0);
  const scan = data?.containerScan;

  return (
    <div className="statistics-page">
      <div className="statistics-toolbar">
        <div className="statistics-action-row">
          <button className="btn" onClick={() => openMaterialsWithWindowBehavior(currentFile, () => setShowMaterials(true))} disabled={!data}>材料列表</button>
          <button className="btn" onClick={loadStats}>重新统计</button>
          <button className="btn" type="button" disabled>打开枚举器...</button>
        </div>

        <div className="statistics-toggle-row">
          <label className="statistics-container-toggle">
            <input
              type="checkbox"
              checked={includeContainerItems}
              onChange={(event) => setIncludeContainerItems(event.target.checked)}
            />
            统计容器
          </label>
          <label className="statistics-container-toggle statistics-toggle-disabled">
            <input type="checkbox" disabled />
            统计实体
          </label>
        </div>
      </div>

      {error && <pre className="statistics-error">{error}</pre>}

      {scan?.warnings?.length ? (
        <details className="statistics-warnings">
          <summary className="statistics-warnings-summary">容器扫描 warning（{scan.warnings.length}）</summary>
          <pre className="statistics-warnings-body">
            {scan.warnings.slice(0, 40).join("\n")}
          </pre>
        </details>
      ) : null}

      <div className="statistics-layout">
        <div className="group-box statistics-panel-scroll">
          <div className="group-box-title">统计学信息</div>
          {data ? (
            <div className="statistics-metrics">
              <ReadOnlyRow label="网格数" value={data.enclosingSize.x * data.enclosingSize.y * data.enclosingSize.z} />
              <ReadOnlyRow label="方块数" value={data.totalNonAirBlocks} />
              <ReadOnlyRow label="密度" value={`${(data.density * 100).toFixed(2)}%`} />
              <ReadOnlyRow label="区域数量" value={data.regionCount} />
              <ReadOnlyRow label="包围尺寸" value={`${data.enclosingSize.x}x${data.enclosingSize.y}x${data.enclosingSize.z}`} />
              <ReadOnlyRow label="结构类型" value={data.buildingType} />
              <ReadOnlyRow label="红石偏度" value={`${(data.redstoneRatio * 100).toFixed(2)}%`} />
              <ReadOnlyRow label="液体偏度" value={`${(data.fluidRatio * 100).toFixed(2)}%`} />
              <ReadOnlyRow label="实体种类" value={data.entityCount} />
            </div>
          ) : (
            <div className="statistics-loading">加载中...</div>
          )}
        </div>

        <div className="statistics-side-stack">
          <div className="group-box statistics-materials-panel">
            <div className="group-box-title">主要材料</div>
            <div className="statistics-materials-body">
              {data ? (
                featuredMaterials.map((material) => {
                  const ratio = totalMaterialCount > 0 ? (material.totalCount / totalMaterialCount) * 100 : 0;
                  return (
                    <div key={material.id} className="statistics-material-row">
                      <div className="statistics-material-main">
                        <span className="statistics-material-name">{material.name}</span>
                        <span className="statistics-material-count">{material.totalCount}</span>
                      </div>
                      <div className="statistics-material-meta">
                        <span className="statistics-material-ratio">占比 {ratio.toFixed(2)}%</span>
                        {includeContainerItems && material.containerItemCount > 0 && (
                          <span className="statistics-material-detail">
                            方块 {material.blockCount} / 容器 {material.containerItemCount}
                          </span>
                        )}
                      </div>
                    </div>
                  );
                })
              ) : (
                <div className="statistics-loading">加载中...</div>
              )}
            </div>
          </div>

          <div className="group-box statistics-enumerator-panel">
            <div className="group-box-title">枚举器信息</div>
            <div className="statistics-enumerator-body">
              <div className="statistics-panel-note">枚举器尚未接入，当前先保留占位控件。</div>
              <div className="statistics-enumerator-actions">
                <button className="btn" type="button" disabled>打开枚举器...</button>
                <button className="btn" type="button" disabled>编辑信息框</button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="statistics-footer">
        <div className="statistics-summary">
          {includeContainerItems && scan
            ? `容器扫描：${scan.containers_scanned} 个容器，${scan.item_stacks_scanned} 个物品堆。`
            : "当前仅统计投影方块。"}
        </div>
        <div className="statistics-file-path" title={currentFile}>
          {currentFile}
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
        <input className="input statistics-readonly-input" value={value} readOnly />
      </div>
    </div>
  );
}

