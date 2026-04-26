import React, { useState, useEffect } from 'react';
import { loadStructureStats, loadMaterialsScope, StatsData, exportMaterials, MaterialItem } from '../services/statsService';
import { BlockIcon } from '../components/BlockIcon';
import { Dropdown } from '../components/Dropdown';

function MaterialTooltip({ x, y, item, multiplier }: any) {
  if (!item) return null;
  const total = item.count * multiplier;
  const stacks = Math.floor(total / 64);
  const remainder = total % 64;
  const shulker = (total / (64 * 27)).toFixed(2);

  return (
    <div style={{
      position: 'fixed',
      left: x + 15,
      top: y + 15,
      backgroundColor: '#1a1a1a',
      border: '1px solid #555',
      color: '#fff',
      padding: '8px 12px',
      zIndex: 9999,
      pointerEvents: 'none',
      display: 'flex',
      flexDirection: 'column',
      gap: 4,
      minWidth: 200,
      boxShadow: '2px 2px 5px rgba(0,0,0,0.5)',
      fontFamily: 'inherit',
      fontSize: '12pt'
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontWeight: 'bold' }}>
        <BlockIcon blockId={item.iconHint} />
        <span>{item.name}</span>
      </div>
      <div style={{ color: '#aaa', fontSize: '0.9em' }}>{item.id}</div>
      <div style={{ marginTop: 4 }}>
        总计: {total} = {stacks} × 64 {remainder > 0 ? `+ ${remainder}` : ''} = {shulker} 潜影盒
      </div>
    </div>
  );
}

export function MaterialsDialog({ data, onClose, currentFile }: { data: StatsData, onClose: () => void, currentFile: string }) {
  const [multiplier, setMultiplier] = useState(1);
  const [includeEntities, setIncludeEntities] = useState(false);
  const [workbook, setWorkbook] = useState("scope:all");
  const [materials, setMaterials] = useState<MaterialItem[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  
  const [hoverItem, setHoverItem] = useState<any>(null);
  const [mousePos, setMousePos] = useState({ x: 0, y: 0 });

  // Generate dropdown options based on the backend regions and layers
  const options = [
    { label: "整个投影", value: "scope:all" }
  ];
  
  if (data.regions && data.regions.length > 0) {
    data.regions.forEach(r => {
      options.push({ label: `选定区域: ${r.name}`, value: `region:${r.name}` });
    });
  }
  
  if (data.layers && data.layers.length > 0) {
    data.layers.forEach(l => {
      options.push({ label: `选定层级: y=${l.world_y} (层 ${l.layer})`, value: `layer:${l.layer}` });
    });
  }

  const loadMats = async (val: string) => {
    setIsLoading(true);
    try {
      let args: string[] = [];
      if (val.startsWith("scope:")) {
        args = ["--scope", val.split(":")[1]];
      } else if (val.startsWith("region:")) {
        args = ["--region", val.split(":")[1]];
      } else if (val.startsWith("layer:")) {
        args = ["--layer", val.split(":")[1]];
      }
      
      const newMats = await loadMaterialsScope(currentFile, args);
      setMaterials(newMats.sort((a,b) => b.count - a.count));
    } catch(e) {
      alert("获取材料失败: " + e);
    }
    setIsLoading(false);
  };

  useEffect(() => {
    loadMats(workbook);
  }, [workbook]);

  const handleExport = async () => {
    const ok = await exportMaterials(currentFile, materials, multiplier, includeEntities);
    if (ok) alert("导出成功");
  };

  return (
    <div className="dialog-overlay" onMouseMove={e => setMousePos({ x: e.clientX, y: e.clientY })}>
      <div className="dialog-content" style={{ width: 600, display: 'flex', flexDirection: 'column', gap: 12, height: '80vh', padding: 0 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', backgroundColor: '#333', padding: '8px 12px', borderBottom: '2px solid #555' }}>
          <h3 style={{ margin: 0 }}>材料列表</h3>
          <button className="btn" onClick={onClose} style={{ minWidth: 32, padding: '4px 8px' }}>×</button>
        </div>

        <div style={{ padding: '0 12px', display: 'flex', flexDirection: 'column', gap: 12, flex: 1, overflow: 'hidden' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span>工作簿:</span>
            <Dropdown
              value={workbook}
              options={options}
              onChange={v => setWorkbook(v)}
            />
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <button className="btn" onClick={() => loadMats(workbook)}>重新加载</button>
            <button className="btn" onClick={handleExport}>写入文件...</button>
            <div style={{ flex: 1 }} />
            <span>倍数</span>
            <input 
              type="number" 
              className="input" 
              style={{ width: 60 }} 
              value={multiplier} 
              onChange={e => setMultiplier(Math.max(1, parseInt(e.target.value) || 1))} 
            />
            <label style={{ display: 'flex', alignItems: 'center', gap: 4, marginLeft: 8 }}>
              <input 
                type="checkbox" 
                checked={includeEntities} 
                onChange={e => {
                  alert("当前后端暂不支持合并实体到材料列表，等待后续接口升级。");
                }} 
              />
              统计实体
            </label>
          </div>

          <div style={{ fontSize: '0.9em', color: '#aaa' }}>
            {isLoading ? "正在分析..." : "分析完成"}
          </div>

          <div style={{ flex: 1, backgroundColor: '#000', border: '2px solid #555', overflowY: 'auto' }}>
            <table style={{ width: '100%', borderCollapse: 'collapse', color: '#ccc' }}>
              <thead style={{ position: 'sticky', top: 0, backgroundColor: '#222', zIndex: 1 }}>
                <tr>
                  <th style={{ padding: '8px', textAlign: 'center', borderRight: '2px solid #555', borderBottom: '2px solid #555', width: 50 }}>图标</th>
                  <th style={{ padding: '8px', textAlign: 'left', borderRight: '2px solid #555', borderBottom: '2px solid #555' }}>名称</th>
                  <th style={{ padding: '8px', textAlign: 'right', borderBottom: '2px solid #555' }}>总计</th>
                </tr>
              </thead>
              <tbody>
                {materials.map((m, i) => (
                  <tr 
                    key={m.id} 
                    style={{ backgroundColor: i % 2 === 0 ? '#111' : '#1a1a1a', borderBottom: '1px solid #333' }}
                    onMouseEnter={() => setHoverItem(m)}
                    onMouseLeave={() => setHoverItem(null)}
                  >
                    <td style={{ padding: '4px', textAlign: 'center', borderRight: '1px solid #444' }}>
                      <div style={{ display: 'inline-block' }}>
                        <BlockIcon blockId={m.iconHint} />
                      </div>
                    </td>
                    <td style={{ padding: '8px', borderRight: '1px solid #444' }}>
                      {m.name}
                    </td>
                    <td style={{ padding: '8px', textAlign: 'right' }}>
                      {m.count * multiplier}
                    </td>
                  </tr>
                ))}
                {materials.length === 0 && !isLoading && (
                  <tr>
                    <td colSpan={3} style={{ padding: '16px', textAlign: 'center', color: '#555' }}>暂无材料数据</td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
        <div style={{ height: 12 }} />
      </div>
      <MaterialTooltip x={mousePos.x} y={mousePos.y} item={hoverItem} multiplier={multiplier} />
    </div>
  );
}

export function StatisticsPage({ currentFile }: any) {
  const [data, setData] = useState<StatsData | null>(null);
  const [error, setError] = useState("");
  const [showMaterials, setShowMaterials] = useState(false);

  useEffect(() => {
    if (!currentFile) return;
    loadStructureStats(currentFile)
      .then(setData)
      .catch(e => setError(e.toString()));
  }, [currentFile]);

  if (!currentFile) {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100%', opacity: 0.7 }}>
        <h2>统计</h2>
        <p>请先在投影库中选择一个 .litematic 文件。</p>
      </div>
    );
  }

  const handleReanalyze = () => {
    loadStructureStats(currentFile)
      .then(setData)
      .catch(e => setError(e.toString()));
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <button className="btn" onClick={() => setShowMaterials(true)} disabled={!data}>材料列表</button>
        <div style={{ flex: 1, color: '#aaa', fontSize: '0.9em' }}>
          分析完成，当前统计结果来自现有 Rust 后端。
        </div>
        <div style={{ color: '#888', fontSize: '0.85em', maxWidth: 300, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={currentFile}>
          {currentFile}
        </div>
        <button className="btn" onClick={handleReanalyze}>重新分析</button>
      </div>

      {error && <pre style={{ color: '#ff6666', background: 'rgba(255,0,0,0.1)', padding: 8, border: '1px solid #ff6666' }}>{error}</pre>}
      
      <div style={{ display: 'flex', gap: 12, flex: 1, overflow: 'hidden' }}>
        {/* 左侧：结构分析 */}
        <div className="group-box" style={{ flex: 1, overflowY: 'auto' }}>
          <div className="group-box-title">结构分析</div>
          {data ? (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
              <div className="form-row" style={{ marginBottom: 0 }}>
                <div className="form-label" style={{ width: 140, background: '#444', padding: '6px 8px', color: '#ccc', textAlign: 'right', border: '2px solid #222' }}>非空气方块:</div>
                <div className="form-field" style={{ margin: 0, padding: 0 }}><input className="input" style={{ width: '100%', border: '2px solid #222', borderLeft: 'none' }} value={data.totalNonAirBlocks} readOnly /></div>
              </div>
              <div className="form-row" style={{ marginBottom: 0 }}>
                <div className="form-label" style={{ width: 140, background: '#444', padding: '6px 8px', color: '#ccc', textAlign: 'right', border: '2px solid #222' }}>区域数量:</div>
                <div className="form-field" style={{ margin: 0, padding: 0 }}><input className="input" style={{ width: '100%', border: '2px solid #222', borderLeft: 'none' }} value={data.regionCount} readOnly /></div>
              </div>
              <div className="form-row" style={{ marginBottom: 0 }}>
                <div className="form-label" style={{ width: 140, background: '#444', padding: '6px 8px', color: '#ccc', textAlign: 'right', border: '2px solid #222' }}>包围尺寸:</div>
                <div className="form-field" style={{ margin: 0, padding: 0 }}><input className="input" style={{ width: '100%', border: '2px solid #222', borderLeft: 'none' }} value={`${data.enclosingSize.x}x${data.enclosingSize.y}x${data.enclosingSize.z}`} readOnly /></div>
              </div>
              <div className="form-row" style={{ marginBottom: 0 }}>
                <div className="form-label" style={{ width: 140, background: '#444', padding: '6px 8px', color: '#ccc', textAlign: 'right', border: '2px solid #222' }}>密度:</div>
                <div className="form-field" style={{ margin: 0, padding: 0 }}><input className="input" style={{ width: '100%', border: '2px solid #222', borderLeft: 'none' }} value={(data.density * 100).toFixed(2) + "%"} readOnly /></div>
              </div>
              <div className="form-row" style={{ marginBottom: 0 }}>
                <div className="form-label" style={{ width: 140, background: '#444', padding: '6px 8px', color: '#ccc', textAlign: 'right', border: '2px solid #222' }}>结构类型:</div>
                <div className="form-field" style={{ margin: 0, padding: 0 }}><input className="input" style={{ width: '100%', border: '2px solid #222', borderLeft: 'none' }} value={data.buildingType} readOnly /></div>
              </div>
              <div className="form-row" style={{ marginBottom: 0 }}>
                <div className="form-label" style={{ width: 140, background: '#444', padding: '6px 8px', color: '#ccc', textAlign: 'right', border: '2px solid #222' }}>红石偏度:</div>
                <div className="form-field" style={{ margin: 0, padding: 0 }}><input className="input" style={{ width: '100%', border: '2px solid #222', borderLeft: 'none' }} value={(data.redstoneRatio * 100).toFixed(2) + "%"} readOnly /></div>
              </div>
              <div className="form-row" style={{ marginBottom: 0 }}>
                <div className="form-label" style={{ width: 140, background: '#444', padding: '6px 8px', color: '#ccc', textAlign: 'right', border: '2px solid #222' }}>流体偏度:</div>
                <div className="form-field" style={{ margin: 0, padding: 0 }}><input className="input" style={{ width: '100%', border: '2px solid #222', borderLeft: 'none' }} value={(data.fluidRatio * 100).toFixed(2) + "%"} readOnly /></div>
              </div>
              <div className="form-row" style={{ marginBottom: 0 }}>
                <div className="form-label" style={{ width: 140, background: '#444', padding: '6px 8px', color: '#ccc', textAlign: 'right', border: '2px solid #222' }}>实体种类:</div>
                <div className="form-field" style={{ margin: 0, padding: 0 }}><input className="input" style={{ width: '100%', border: '2px solid #222', borderLeft: 'none' }} value={data.entityCount} readOnly /></div>
              </div>
            </div>
          ) : (
            <div style={{ color: '#777', padding: 16 }}>加载中...</div>
          )}
        </div>

        {/* 右侧：主要材料 */}
        <div className="group-box" style={{ flex: 1, display: 'flex', flexDirection: 'column' }}>
          <div className="group-box-title">主要材料</div>
          <div style={{ flex: 1, backgroundColor: '#1a1a1a', border: '2px solid #555', padding: 12, overflowY: 'auto', color: '#ccc' }}>
            {data ? (
              [...data.materials].sort((a,b) => b.count - a.count).map((m, i) => (
                <div key={m.id} style={{ marginBottom: 6, fontSize: '1.05em' }}>
                  {m.name} <span style={{ color: '#888' }}>x</span> {m.count}
                </div>
              ))
            ) : (
              <div style={{ color: '#777' }}>加载中...</div>
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
