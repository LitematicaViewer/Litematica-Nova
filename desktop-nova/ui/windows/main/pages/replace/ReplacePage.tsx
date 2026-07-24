import React, { useState, useEffect, useCallback } from 'react';
import { BlockIcon } from '../../../../components/BlockIcon';
import { BlockPickerDialog } from '../../../../components/BlockPickerDialog';
import {
  getAllBlocks,
  getBlockProperties,
  translateKey,
  translateValue,
  checkFileExists,
} from '../../../../../src/business/replace';
import { EnumeratorBlockPickerDialog } from './EnumeratorBlockPickerDialog';
import {
  dryRunReplaceUnits,
  applyReplaceUnits,
  listReplacePresets,
  saveReplacePreset,
  loadReplacePreset,
  deleteReplacePreset,
  openReplacePresetFolder,
} from '../../../../../src/business/replace';
import { selectLitematicSavePath } from '../../../../../src/business/facade';
import { loadMaterialsScope } from '../../../../../src/services/statsService';
import type {
  ReplaceUnit,
  ReplaceEntry,
  ReplaceOutputEntry,
  ReplacePreviewSummary,
  UnitPreviewSummary,
} from '../../../../../src/business/replace';

// ── PropertySelector ─────────────────────────────────────────────────────────

function PropertySelector({ blockId, value, onChange, isOutput }: {
  blockId: string;
  value: Record<string, string>;
  onChange: (props: Record<string, string>) => void;
  isOutput: boolean;
}) {
  const props = getBlockProperties(blockId);
  const keys = Object.keys(props).sort();
  if (keys.length === 0) return <span className="replace-property-no-props">无属性</span>;
  return (
    <div className="replace-entry-properties">
      {keys.map((k) => (
        <div key={k} className="replace-property-item">
          <span className="replace-property-label">{translateKey(k)}:</span>
          <select
            className="input"
            value={value[k] || ''}
            onChange={(e) => {
              const nv = { ...value };
              if (!e.target.value) delete nv[k]; else nv[k] = e.target.value;
              onChange(nv);
            }}
          >
            <option value="">{isOutput ? '默认' : '所有'}</option>
            {props[k].map((v) => (
              <option key={v} value={v}>{translateValue(k, v)} ({v})</option>
            ))}
          </select>
        </div>
      ))}
    </div>
  );
}
// ── Entry row components ─────────────────────────────────────────────────────

function InputEntryRow({ entry, onChange, onRemove }: {
  entry: ReplaceEntry;
  onChange: (e: ReplaceEntry) => void;
  onRemove: () => void;
}) {
  return (
    <div className="replace-entry-row">
      <div className="replace-entry-top">
        <BlockIcon blockId={entry.name} />
        <input
          className="input replace-entry-name-input"
          value={entry.name}
          placeholder="minecraft:stone"
          onChange={(e) => onChange({ ...entry, name: e.target.value })}
        />
        <button className="btn replace-entry-remove-btn" onClick={onRemove}>×</button>
      </div>
      <PropertySelector
        blockId={entry.name}
        value={entry.properties || {}}
        onChange={(p) => onChange({ ...entry, properties: Object.keys(p).length ? p : undefined })}
        isOutput={false}
      />
    </div>
  );
}

function OutputEntryRow({ entry, onChange, onRemove }: {
  entry: ReplaceOutputEntry;
  onChange: (e: ReplaceOutputEntry) => void;
  onRemove: () => void;
}) {
  return (
    <div className="replace-entry-row">
      <div className="replace-entry-top">
        <BlockIcon blockId={entry.name} />
        <input
          className="input replace-entry-name-input"
          value={entry.name}
          placeholder="minecraft:stone"
          onChange={(e) => onChange({ ...entry, name: e.target.value })}
        />
        <span className="replace-entry-weight-label">权重</span>
        <input
          className="input replace-entry-weight-input"
          type="number"
          min={0}
          value={entry.weight}
          onChange={(e) => onChange({ ...entry, weight: Math.max(0, parseInt(e.target.value) || 0) })}
        />
        <button className="btn replace-entry-remove-btn" onClick={onRemove}>×</button>
      </div>
      <PropertySelector
        blockId={entry.name}
        value={entry.properties || {}}
        onChange={(p) => onChange({ ...entry, properties: Object.keys(p).length ? p : undefined })}
        isOutput={true}
      />
    </div>
  );
}

// ── ReplaceUnitCard ──────────────────────────────────────────────────────────

type PickerTarget = { side: 'input' | 'output'; type: 'inventory' | 'enumerator' } | null;

function ReplaceUnitCard({ unit, index, total, onChange, onRemove, onMoveUp, onMoveDown, scanSummary, currentBlocks }: {
  unit: ReplaceUnit;
  index: number;
  total: number;
  onChange: (u: ReplaceUnit) => void;
  onRemove: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
  scanSummary?: UnitPreviewSummary;
  currentBlocks?: string[];
}) {
  const [collapsed, setCollapsed] = useState(false);
  const [picker, setPicker] = useState<PickerTarget>(null);

  const updateInput = (i: number, e: ReplaceEntry) => {
    const next = [...unit.input]; next[i] = e; onChange({ ...unit, input: next });
  };
  const removeInput = (i: number) =>
    onChange({ ...unit, input: unit.input.filter((_, idx) => idx !== i) });

  const updateOutput = (i: number, e: ReplaceOutputEntry) => {
    const next = [...unit.output]; next[i] = e; onChange({ ...unit, output: next });
  };
  const removeOutput = (i: number) =>
    onChange({ ...unit, output: unit.output.filter((_, idx) => idx !== i) });

  // Called from BlockPickerDialog — adds single entry
  const handlePickBlock = (blockId: string) => {
    if (!picker) return;
    if (picker.side === 'input') {
      onChange({ ...unit, input: [...unit.input, { name: blockId }] });
    } else {
      onChange({ ...unit, output: [...unit.output, { name: blockId, weight: 1 }] });
    }
    setPicker(null);
  };

  // Called from EnumeratorBlockPickerDialog — adds multiple entries
  const handleAddBlocks = (blockIds: string[]) => {
    if (!picker) return;
    if (picker.side === 'input') {
      const added = blockIds.map((id) => ({ name: id } as ReplaceEntry));
      onChange({ ...unit, input: [...unit.input, ...added] });
    } else {
      const added = blockIds.map((id) => ({ name: id, weight: 1 } as ReplaceOutputEntry));
      onChange({ ...unit, output: [...unit.output, ...added] });
    }
    setPicker(null);
  };

  /** Two buttons for adding entries from different sources */
  const AddEntryButtons = ({ side }: { side: 'input' | 'output' }) => (
    <div className="replace-add-buttons">
      <button className="btn btn-sm" onClick={() => setPicker({ side, type: 'inventory' })}>
        + 创造模式物品栏
      </button>
      <button className="btn btn-sm" onClick={() => setPicker({ side, type: 'enumerator' })}>
        + 枚举器
      </button>
    </div>
  );

  return (
    <>
      <div className="card replace-unit-card">
        {/* Card header */}
        <div className={`replace-unit-header${collapsed ? ' collapsed' : ''}`}>
          <button className="btn btn-xs" onClick={() => setCollapsed(!collapsed)}>
            {collapsed ? '▶' : '▼'}
          </button>
          <span className="replace-unit-title">替换单元 {index + 1}</span>
          {scanSummary && (
            <span className="replace-unit-hit-count">命中 {scanSummary.hit_count} 个方块</span>
          )}
          <div className="replace-unit-spacer" />
          <button className="btn btn-xs" disabled={index === 0} onClick={onMoveUp}>↑</button>
          <button className="btn btn-xs" disabled={index === total - 1} onClick={onMoveDown}>↓</button>
          <button className="btn btn-xs" onClick={onRemove}>删除单元</button>
        </div>

        {/* Card body */}
        {!collapsed && (
          <div className="replace-unit-body">
            {/* Input side */}
            <div className="replace-unit-side">
              <div className="replace-unit-side-title">输入端（Input）</div>
              {unit.input.map((e, i) => (
                <InputEntryRow key={i} entry={e} onChange={(ne) => updateInput(i, ne)} onRemove={() => removeInput(i)} />
              ))}
              {unit.input.length === 0 && <div className="replace-entry-empty">无输入条目</div>}
              <AddEntryButtons side="input" />
            </div>

            {/* Arrow */}
            <div className="replace-unit-arrow">→</div>

            {/* Output side */}
            <div className="replace-unit-side">
              <div className="replace-unit-side-title">输出端（Output）</div>
              {unit.output.map((e, i) => (
                <OutputEntryRow key={i} entry={e} onChange={(ne) => updateOutput(i, ne)} onRemove={() => removeOutput(i)} />
              ))}
              {unit.output.length === 0 && <div className="replace-entry-empty">无输出条目</div>}
              <AddEntryButtons side="output" />
            </div>
          </div>
        )}

        {/* Scan result detail */}
        {!collapsed && scanSummary && scanSummary.output_distribution.length > 1 && (
          <div className="replace-unit-scan-dist">
            输出分布预估：
            {scanSummary.output_distribution.map((d, i) => (
              <span key={i} className="replace-unit-scan-dist-item">{d.name}：{d.estimated_count}</span>
            ))}
          </div>
        )}
        {!collapsed && scanSummary && scanSummary.warnings.length > 0 && (
          <div className="replace-unit-warnings">
            ⚠ {scanSummary.warnings.join('; ')}
          </div>
        )}
      </div>

      {/* Block picker dialog */}
      {picker?.type === 'inventory' && (
        <BlockPickerDialog
          title={picker.side === 'input' ? '选择输入方块' : '选择输出方块'}
          onClose={() => setPicker(null)}
          onPickBlock={handlePickBlock}
          extraCollections={
            currentBlocks && currentBlocks.length > 0
              ? [{ id: '__current__', name: '当前统计结果', values: currentBlocks }]
              : []
          }
        />
      )}

      {/* Enumerator picker dialog */}
      {picker?.type === 'enumerator' && (
        <EnumeratorBlockPickerDialog
          title={picker.side === 'input' ? '从枚举器批量添加输入方块' : '从枚举器批量添加输出方块'}
          onClose={() => setPicker(null)}
          onAddBlocks={handleAddBlocks}
        />
      )}
    </>
  );
}

// ── Main component ───────────────────────────────────────────────────────────

export function ReplacePage({ currentFile }: { currentFile?: string }) {
  const [units, setUnits] = useState<ReplaceUnit[]>([]);
  const [presets, setPresets] = useState<string[]>([]);
  const [selectedPreset, setSelectedPreset] = useState('');
  const [scanResult, setScanResult] = useState<ReplacePreviewSummary | null>(null);
  const [showConfirmDialog, setShowConfirmDialog] = useState(false);
  const [outputPath, setOutputPath] = useState('');
  const [log, setLog] = useState('');
  const [materialBlocks, setMaterialBlocks] = useState<string[]>([]);

  // Load preset list on mount
  useEffect(() => {
    listReplacePresets().then(setPresets).catch(() => setPresets([]));
  }, []);

  // Load material block list whenever the active file changes
  useEffect(() => {
    if (!currentFile) { setMaterialBlocks([]); return; }
    loadMaterialsScope(currentFile, [])
      .then((items) => setMaterialBlocks(items.map((m) => m.id).filter(Boolean)))
      .catch(() => setMaterialBlocks([]));
  }, [currentFile]);

  // Preset handlers
  const handleLoadPreset = useCallback(async (name: string) => {
    if (!name) return;
    const loaded = await loadReplacePreset(name);
    if (loaded) {
      setUnits(loaded);
      setSelectedPreset(name);
      setLog(`> 已加载预设：${name}`);
    } else {
      alert(`无法加载预设：${name}`);
    }
  }, []);

  const handleSavePreset = useCallback(async () => {
    const name = prompt('预设名称：', selectedPreset || '新预设');
    if (!name) return;
    await saveReplacePreset(name, units);
    const updated = await listReplacePresets();
    setPresets(updated);
    setSelectedPreset(name);
    setLog(`> 已保存预设：${name}`);
  }, [units, selectedPreset]);

  const handleDeletePreset = useCallback(async () => {
    if (!selectedPreset) return alert('请先选择预设');
    if (!confirm(`确定删除预设 "${selectedPreset}"？`)) return;
    await deleteReplacePreset(selectedPreset);
    const updated = await listReplacePresets();
    setPresets(updated);
    setSelectedPreset('');
    setLog(`> 已删除预设：${selectedPreset}`);
  }, [selectedPreset]);

  const handleOpenPresetFolder = () => openReplacePresetFolder();

  // Unit CRUD helpers
  const addUnit = () =>
    setUnits([...units, { input: [], output: [] }]);

  const updateUnit = (i: number, u: ReplaceUnit) => {
    const next = [...units]; next[i] = u; setUnits(next);
  };
  const removeUnit = (i: number) => setUnits(units.filter((_, idx) => idx !== i));
  const moveUnit = (i: number, dir: -1 | 1) => {
    const j = i + dir;
    if (j < 0 || j >= units.length) return;
    const next = [...units];
    [next[i], next[j]] = [next[j], next[i]];
    setUnits(next);
  };

  // Scan handler
  const handleScan = useCallback(async () => {
    if (!currentFile) return;
    if (units.length === 0) return alert('请先添加至少一个替换单元。');
    setLog('> 扫描中...');
    try {
      const summary = await dryRunReplaceUnits(currentFile, units);
      setScanResult(summary);
      const defaultOut = currentFile.replace(/\.litematic$/i, '.replaced.litematic');
      setOutputPath(defaultOut);
      setShowConfirmDialog(true);
      const total = summary.per_unit.reduce((acc, u) => acc + u.hit_count, 0);
      setLog(`> 扫描完成，共命中 ${total} 个方块`);
    } catch (e: any) {
      alert('扫描失败: ' + e);
      setLog('> 扫描失败: ' + e);
    }
  }, [currentFile, units]);

  // Apply handler
  const handleApply = useCallback(async () => {
    if (!currentFile || !outputPath) return;
    if (outputPath === currentFile) return alert('输出路径不能与输入路径相同。');
    if (await checkFileExists(outputPath)) return alert('输出文件已存在，请重命名。');
    setShowConfirmDialog(false);
    setLog('> 替换中...');
    try {
      await applyReplaceUnits(currentFile, outputPath, units);
      alert(`替换完成！\n文件已保存至:\n${outputPath}`);
      setLog(`> 替换完成，文件保存至 ${outputPath}`);
    } catch (e: any) {
      alert('替换失败: ' + e);
      setLog('> 替换失败: ' + e);
    }
  }, [currentFile, outputPath, units]);

  // Render
  if (!currentFile) {
    return (
      <div className="replace-no-file">
        <h2>方块替换</h2>
        <p>请先在投影库中选择并打开一个 .litematic 文件</p>
      </div>
    );
  }

  return (
    <div className="replace-page">
      {/* Toolbar line 1: presets */}
      <div className="replace-toolbar">
        <select
          className="input"
          value={selectedPreset}
          onChange={(e) => handleLoadPreset(e.target.value)}
        >
          <option value="">(选择预设)</option>
          {presets.map((p) => (
            <option key={p} value={p}>{p}</option>
          ))}
        </select>
        <button className="btn btn-md" onClick={handleSavePreset}>保存预设</button>
        <button className="btn btn-md" onClick={handleDeletePreset} disabled={!selectedPreset}>删除该预设</button>
        <button className="btn btn-md" onClick={handleOpenPresetFolder}>打开预设文件夹...</button>
      </div>

      {/* Toolbar line 2: unit management */}
      <div className="replace-toolbar-2">
        <button className="btn btn-md" onClick={addUnit}>+ 添加替换单元</button>
        <button className="btn btn-md" onClick={() => { setUnits([]); setScanResult(null); }}>清空所有</button>
      </div>

      {/* Replace unit cards */}
      <div className="replace-content">
        {units.length === 0 && (
          <div className="replace-content-empty">暂无替换单元，点击"添加替换单元"开始</div>
        )}
        {units.map((u, i) => (
          <ReplaceUnitCard
            key={i}
            unit={u}
            index={i}
            total={units.length}
            onChange={(nu) => updateUnit(i, nu)}
            onRemove={() => removeUnit(i)}
            onMoveUp={() => moveUnit(i, -1)}
            onMoveDown={() => moveUnit(i, 1)}
            scanSummary={scanResult?.per_unit?.[i]}
            currentBlocks={materialBlocks}
          />
        ))}
      </div>

      {/* Footer: action buttons + log */}
      <div className="replace-footer">
        <div className="replace-footer-buttons">
          <div className="replace-unit-spacer" />
          <button className="btn btn-action" onClick={handleScan} disabled={units.length === 0}>
            扫描并评估
          </button>
          <button
            className={`btn btn-action${scanResult ? ' btn-primary' : ''}`}
            onClick={handleApply}
            disabled={!scanResult}
          >
            替换方块
          </button>
        </div>
        <textarea
          className="replace-log-area"
          readOnly
          value={log}
          placeholder="执行日志..."
        />
      </div>

      {/* Confirm dialog */}
      {showConfirmDialog && scanResult && (
        <div className="dialog-overlay">
          <div className="dialog-content dialog-content--wide">
            <h3>扫描结果确认</h3>
            <div className="replace-confirm-summary-block">
              <p className="replace-confirm-summary">
                <strong>共扫描 {scanResult.regions_scanned} 个区域</strong>，命中 <strong>{scanResult.block_positions_affected}</strong> 个方块位置
              </p>
              <div className="replace-confirm-details">
                {scanResult.per_unit.map((u, i) => (
                  <div key={i} className="replace-confirm-unit">
                    <strong>替换单元 {i + 1}:</strong> 命中 {u.hit_count} 个方块
                    {u.output_distribution.length > 1 && (
                      <div className="replace-confirm-dist">
                        输出分布预估：
                        {u.output_distribution.map((d, j) => (
                          <span key={j} className="replace-confirm-dist-item">
                            {d.name.replace('minecraft:', '')}={d.estimated_count}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                ))}
                {scanResult.warnings.length > 0 && (
                  <div className="replace-confirm-warning">⚠ 警告：{scanResult.warnings.join('; ')}</div>
                )}
              </div>
            </div>
            {/* Diagnostic log */}
            {scanResult.debug_log && scanResult.debug_log.length > 0 && (
              <details className="replace-debug-log">
                <summary className="replace-debug-log-summary">
                  诊断日志（{scanResult.debug_log.length} 行）
                  {scanResult.debug_log.some(l => l.includes('[警告]')) && (
                    <span className="replace-debug-log-warn-badge"> ⚠ 含警告</span>
                  )}
                </summary>
                <pre className="replace-debug-log-content">
                  {scanResult.debug_log.join('\n')}
                </pre>
              </details>
            )}
            <p className="replace-confirm-output-label">选择输出路径：</p>
            <div className="replace-confirm-output-row">
              <input className="input replace-confirm-output-input" value={outputPath} readOnly />
              <button className="btn btn-md" onClick={async () => {
                const res = await selectLitematicSavePath(outputPath);
                if (res) setOutputPath(res);
              }}>浏览...</button>
            </div>
            <div className="replace-confirm-buttons">
              <button className="btn btn-md" onClick={() => setShowConfirmDialog(false)}>取消</button>
              <button className="btn btn-action btn-primary" onClick={handleApply}>确认并替换</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

