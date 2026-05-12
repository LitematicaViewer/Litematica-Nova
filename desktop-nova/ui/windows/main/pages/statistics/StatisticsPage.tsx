import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  exportMaterials,
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
import { Dropdown } from "../../../../components/Dropdown";

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

  if (!item) return null;

  return (
    <div
      ref={popupRef}
      className="material-list-hover-popup"
      role="tooltip"
      style={{ left: position.left, top: position.top }}
    >
      <div className="material-list-hover-popup-row material-list-hover-popup-item-row">
        <span className="material-list-hover-popup-label">项目：</span>
        <BlockIcon blockId={item.iconHint} />
        <span className="material-list-hover-popup-name">{item.name}</span>
      </div>
      <div className="material-list-hover-popup-row">ID：{item.id}</div>
      <div className="material-list-hover-popup-row">
        总计：<strong>{total}</strong> = <strong>{stacks}</strong> × 64 + <strong>{remainder}</strong> = <strong>{shulkerBoxes}</strong> 潜影盒
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
  const contentStyle: React.CSSProperties = {
    width: standalone ? "100%" : 720,
    maxWidth: standalone ? "none" : "90%",
    height: standalone ? "100%" : "80vh",
    display: "flex",
    flexDirection: "column",
    gap: 12,
    padding: 0,
    overflow: "hidden",
  };

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
      <div
        className={standalone ? "material-list-window" : "dialog-content"}
        style={contentStyle}
        onMouseMove={(event) => setMousePos({ x: event.clientX, y: event.clientY })}
      >
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", backgroundColor: "var(--surface-elevated)", padding: "8px 12px", borderBottom: "1px solid var(--border)" }}>
          <h3 style={{ margin: 0 }}>材料列表</h3>
          {onClose && <button className="btn material-list-close-button" type="button" aria-label="关闭材料列表" onClick={onClose}>×</button>}
        </div>

        <div style={{ padding: "0 12px", display: "flex", flexDirection: "column", gap: 12, flex: 1, overflow: "hidden" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span>范围</span>
            <Dropdown value={workbook} options={options} onChange={setWorkbook} />
            <button className="btn" onClick={() => loadMats(workbook)} disabled={isLoading}>重新加载</button>
            <button className="btn" onClick={handleExport} disabled={isLoading || materials.length === 0}>导出材料列表</button>
            <div style={{ flex: 1 }} />
            <span>倍数</span>
            <input
              type="number"
              className="input"
              style={{ width: 70 }}
              value={multiplier}
              onChange={(event) => setMultiplier(Math.max(1, parseInt(event.target.value, 10) || 1))}
            />
          </div>

          <label style={{ display: "flex", alignItems: "center", gap: 6, color: "var(--text)" }}>
            <input
              type="checkbox"
              checked={includeContainerItems}
              onChange={(event) => setIncludeContainerItems(event.target.checked)}
            />
            统计容器内物品
          </label>

          <div style={{ fontSize: "0.9em", color: "var(--text-muted)" }}>
            {isLoading ? "正在分析..." : includeContainerItems ? "已包含容器 BlockEntity/TileEntity 内物品。" : "当前仅统计投影方块。"}
          </div>
          {error && <pre style={{ color: "#ffb3b3", background: "rgba(255,0,0,0.1)", padding: 8, margin: 0, whiteSpace: "pre-wrap" }}>{error}</pre>}

          <div style={{ flex: 1, backgroundColor: "var(--surface)", border: "1px solid var(--border)", overflowY: "auto" }}>
            <table style={{ width: "100%", borderCollapse: "collapse", color: "var(--text)" }}>
              <thead style={{ position: "sticky", top: 0, backgroundColor: "var(--surface-elevated)", zIndex: 1 }}>
                <tr>
                  <th style={thCenter}>图标</th>
                  <th style={thLeft}>名称</th>
                  <th style={thRight}>方块</th>
                  <th style={thRight}>容器物品</th>
                  <th style={thRight}>合计</th>
                </tr>
              </thead>
              <tbody>
                {materials.map((material, index) => (
                  <tr
                    key={material.id}
                    style={{ backgroundColor: index % 2 === 0 ? "rgba(255,255,255,0.02)" : "transparent", borderBottom: "1px solid var(--border)" }}
                    onMouseEnter={() => setHoverItem(material)}
                    onMouseLeave={() => setHoverItem(null)}
                  >
                    <td style={tdCenter}><BlockIcon blockId={material.iconHint} /></td>
                    <td style={tdLeft}>{material.name}</td>
                    <td style={tdRight}>{material.blockCount * multiplier}</td>
                    <td style={tdRight}>{material.containerItemCount * multiplier}</td>
                    <td style={tdRight}>{material.totalCount * multiplier}</td>
                  </tr>
                ))}
                {materials.length === 0 && !isLoading && (
                  <tr>
                    <td colSpan={5} style={{ padding: 16, textAlign: "center", color: "var(--text-muted)" }}>暂无材料数据</td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
        <div style={{ height: 12 }} />
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
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", height: "100%", opacity: 0.7 }}>
        <h2>统计</h2>
        <p>请先在投影库中选择一个 .litematic 文件。</p>
      </div>
    );
  }

  const topMaterials = data ? [...data.materials].sort((a, b) => b.totalCount - a.totalCount) : [];
  const scan = data?.containerScan;

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <button className="btn" onClick={() => openMaterialsWithWindowBehavior(currentFile, () => setShowMaterials(true))} disabled={!data}>材料列表</button>
        <label style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <input
            type="checkbox"
            checked={includeContainerItems}
            onChange={(event) => setIncludeContainerItems(event.target.checked)}
          />
          统计容器内物品
        </label>
        <div style={{ flex: 1, color: "var(--text-muted)", fontSize: "0.9em" }}>
          {includeContainerItems && scan
            ? `容器扫描：${scan.containers_scanned} 个容器，${scan.item_stacks_scanned} 个物品堆。`
            : "当前仅统计投影方块。"}
        </div>
        <div style={{ color: "var(--text-muted)", fontSize: "0.85em", maxWidth: 300, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={currentFile}>
          {currentFile}
        </div>
        <button className="btn" onClick={loadStats}>重新分析</button>
      </div>

      {error && <pre style={{ color: "#ff6666", background: "rgba(255,0,0,0.1)", padding: 8, border: "1px solid #ff6666" }}>{error}</pre>}

      {scan?.warnings?.length ? (
        <details>
          <summary style={{ color: "var(--text-muted)", cursor: "pointer" }}>容器扫描 warning（{scan.warnings.length}）</summary>
          <pre style={{ whiteSpace: "pre-wrap", maxHeight: 120, overflow: "auto", background: "var(--surface)", border: "1px solid var(--border)", padding: 8 }}>
            {scan.warnings.slice(0, 40).join("\n")}
          </pre>
        </details>
      ) : null}

      <div style={{ display: "flex", gap: 12, flex: 1, overflow: "hidden" }}>
        <div className="group-box" style={{ flex: 1, overflowY: "auto" }}>
          <div className="group-box-title">结构分析</div>
          {data ? (
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
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
            <div style={{ color: "var(--text-muted)", padding: 16 }}>加载中...</div>
          )}
        </div>

        <div className="group-box" style={{ flex: 1, display: "flex", flexDirection: "column" }}>
          <div className="group-box-title">主要材料</div>
          <div style={{ flex: 1, backgroundColor: "var(--surface)", border: "1px solid var(--border)", padding: 12, overflowY: "auto", color: "var(--text)" }}>
            {data ? (
              topMaterials.map((material) => (
                <div key={material.id} style={{ marginBottom: 6, fontSize: "1.05em" }}>
                  {material.name} <span style={{ color: "var(--text-muted)" }}>x</span> {material.totalCount}
                  {includeContainerItems && material.containerItemCount > 0 && (
                    <span style={{ color: "var(--text-muted)", marginLeft: 8 }}>
                      方块 {material.blockCount} / 容器 {material.containerItemCount}
                    </span>
                  )}
                </div>
              ))
            ) : (
              <div style={{ color: "var(--text-muted)" }}>加载中...</div>
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
    <div className="form-row" style={{ marginBottom: 0 }}>
      <div className="form-label" style={{ width: 140, padding: "6px 8px", textAlign: "right" }}>{label}</div>
      <div className="form-field" style={{ margin: 0, padding: 0 }}>
        <input className="input" style={{ width: "100%" }} value={value} readOnly />
      </div>
    </div>
  );
}

const thCenter: React.CSSProperties = { padding: 8, textAlign: "center", borderBottom: "1px solid var(--border)", width: 54 };
const thLeft: React.CSSProperties = { padding: 8, textAlign: "left", borderBottom: "1px solid var(--border)" };
const thRight: React.CSSProperties = { padding: 8, textAlign: "right", borderBottom: "1px solid var(--border)" };
const tdCenter: React.CSSProperties = { padding: 6, textAlign: "center", borderBottom: "1px solid var(--border)" };
const tdLeft: React.CSSProperties = { padding: 8, borderBottom: "1px solid var(--border)" };
const tdRight: React.CSSProperties = { padding: 8, textAlign: "right", borderBottom: "1px solid var(--border)" };


