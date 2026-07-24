import React, { useState, useEffect, useCallback, useMemo } from 'react';
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
  isSeparatorItem,
} from '../../../../../src/business/replace';
import { selectLitematicSavePath } from '../../../../../src/business/facade';
import { loadMaterialsScope } from '../../../../../src/services/statsService';
import type {
  ReplaceUnit,
  ReplaceItem,
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

// ── SerialSeparatorBar ────────────────────────────────────────────────────────

function SerialSeparatorBar({ index, total, onRemove, onMoveUp, onMoveDown }: {
  index: number;
  total: number;
  onRemove: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
}) {
  return (
    <div className="replace-separator">
      <div className="replace-separator-line" />
      <span className="replace-separator-label">串行分隔符</span>
      <div className="replace-separator-actions">
        <button className="btn btn-xs" disabled={index === 0} onClick={onMoveUp} title="上移">↑</button>
        <button className="btn btn-xs" disabled={index === total - 1} onClick={onMoveDown} title="下移">↓</button>
        <button className="btn btn-xs" onClick={onRemove} title="删除分隔符">删除</button>
      </div>
      <div className="replace-separator-line" />
    </div>
  );
}

// ── ReplaceUnitCard ──────────────────────────────────────────────────────────

type PickerTarget = { side: 'input' | 'output'; type: 'inventory' | 'enumerator' } | null;

function ReplaceUnitCard({ unit, unitIndex, index, total, onChange, onRemove, onMoveUp, onMoveDown, scanSummary, currentBlocks }: {
  unit: ReplaceUnit;
  /** 当前单元在所有单元（不含分隔符）中的顺序，用于占位标签显示。 */
  unitIndex: number;
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

  const handlePickBlock = (blockId: string) => {
    if (!picker) return;
    if (picker.side === 'input') {
      onChange({ ...unit, input: [...unit.input, { name: blockId }] });
    } else {
      onChange({ ...unit, output: [...unit.output, { name: blockId, weight: 1 }] });
    }
    setPicker(null);
  };

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
        <div className={`replace-unit-header${collapsed ? ' collapsed' : ''}`}>
          <button className="btn btn-xs" onClick={() => setCollapsed(!collapsed)}>
            {collapsed ? '▶' : '▼'}
          </button>
          <input
            className="replace-unit-label-input"
            value={unit.label ?? ''}
            placeholder={`替换单元 ${unitIndex + 1}`}
            onChange={(e) => onChange({ ...unit, label: e.target.value || undefined })}
            onClick={(e) => e.stopPropagation()}
          />
          {scanSummary && (
            <span className="replace-unit-hit-count">命中 {scanSummary.hit_count} 个方块</span>
          )}
          <div className="replace-unit-spacer" />
          <button className="btn btn-xs" disabled={index === 0} onClick={onMoveUp}>↑</button>
          <button className="btn btn-xs" disabled={index === total - 1} onClick={onMoveDown}>↓</button>
          <button className="btn btn-xs" onClick={onRemove}>删除单元</button>
        </div>

        {!collapsed && (
          <div className="replace-unit-body">
            <div className="replace-unit-side">
              <div className="replace-unit-side-title">输入端（Input）</div>
              {unit.input.map((e, i) => (
                <InputEntryRow key={i} entry={e} onChange={(ne) => updateInput(i, ne)} onRemove={() => removeInput(i)} />
              ))}
              {unit.input.length === 0 && <div className="replace-entry-empty">无输入条目</div>}
              <AddEntryButtons side="input" />
            </div>
            <div className="replace-unit-arrow">→</div>
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
  const [items, setItems] = useState<ReplaceItem[]>([]);
  const [presets, setPresets] = useState<string[]>([]);
  const [selectedPreset, setSelectedPreset] = useState('');
  const [scanResult, setScanResult] = useState<ReplacePreviewSummary | null>(null);
  const [showConfirmDialog, setShowConfirmDialog] = useState(false);
  const [outputPath, setOutputPath] = useState('');
  const [materialBlocks, setMaterialBlocks] = useState<string[]>([]);

  /** 预计算 items 中每个元素对应的"单元序号"（仅对 ReplaceUnit 有意义）。 */
  const unitIndexByItemIndex = useMemo(() => {
    const map = new Map<number, number>();
    let ui = 0;
    items.forEach((item, i) => {
      if (!isSeparatorItem(item)) map.set(i, ui++);
    });
    return map;
  }, [items]);

  /** items 中不含分隔符的单元列表，用于确认对话框展示。 */
  const unitItems = useMemo(
    () => items.filter((item): item is ReplaceUnit => !isSeparatorItem(item)),
    [items]
  );

  useEffect(() => {
    listReplacePresets().then(setPresets).catch(() => setPresets([]));
  }, []);

  useEffect(() => {
    if (!currentFile) { setMaterialBlocks([]); return; }
    loadMaterialsScope(currentFile, [])
      .then((mats) => setMaterialBlocks(mats.map((m) => m.id).filter(Boolean)))
      .catch(() => setMaterialBlocks([]));
  }, [currentFile]);

  // Preset handlers
  const handleLoadPreset = useCallback(async (name: string) => {
    if (!name) return;
    const loaded = await loadReplacePreset(name);
    if (loaded) {
      setItems(loaded);
      setSelectedPreset(name);
    } else {
      alert(`无法加载预设：${name}`);
    }
  }, []);

  const handleSavePreset = useCallback(async () => {
    const name = prompt('预设名称：', selectedPreset || '新预设');
    if (!name) return;
    await saveReplacePreset(name, items);
    const updated = await listReplacePresets();
    setPresets(updated);
    setSelectedPreset(name);
  }, [items, selectedPreset]);

  const handleDeletePreset = useCallback(async () => {
    if (!selectedPreset) return alert('请先选择预设');
    if (!confirm(`确定删除预设 "${selectedPreset}"？`)) return;
    await deleteReplacePreset(selectedPreset);
    const updated = await listReplacePresets();
    setPresets(updated);
    setSelectedPreset('');
  }, [selectedPreset]);

  const handleOpenPresetFolder = () => openReplacePresetFolder();

  // Items CRUD
  const addUnit = () =>
    setItems([...items, { input: [], output: [] }]);

  const addSeparator = () =>
    setItems([...items, { _kind: 'separator' }]);

  const updateItem = (i: number, u: ReplaceUnit) => {
    const next = [...items]; next[i] = u; setItems(next);
  };
  const removeItem = (i: number) => setItems(items.filter((_, idx) => idx !== i));
  const moveItem = (i: number, dir: -1 | 1) => {
    const j = i + dir;
    if (j < 0 || j >= items.length) return;
    const next = [...items];
    [next[i], next[j]] = [next[j], next[i]];
    setItems(next);
  };

  // Scan handler
  const handleScan = useCallback(async () => {
    if (!currentFile) return;
    if (unitItems.length === 0) return alert('请先添加至少一个替换单元。');
    try {
      const summary = await dryRunReplaceUnits(currentFile, items);
      setScanResult(summary);
      const defaultOut = currentFile.replace(/\.litematic$/i, '.replaced.litematic');
      setOutputPath(defaultOut);
      setShowConfirmDialog(true);
    } catch (e: any) {
      alert('扫描失败: ' + e);
    }
  }, [currentFile, items, unitItems.length]);

  // Apply handler
  const handleApply = useCallback(async () => {
    if (!currentFile || !outputPath) return;
    if (outputPath === currentFile) return alert('输出路径不能与输入路径相同。');
    if (await checkFileExists(outputPath)) return alert('输出文件已存在，请重命名。');
    setShowConfirmDialog(false);
    try {
      await applyReplaceUnits(currentFile, outputPath, items);
      alert(`替换完成！\n文件已保存至:\n${outputPath}`);
    } catch (e: any) {
      alert('替换失败: ' + e);
    }
  }, [currentFile, outputPath, items]);

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
        <button className="btn btn-md" onClick={addSeparator} disabled={items.length === 0} title="在列表末尾插入串行分隔符">
          + 添加串行分隔符
        </button>
        <button className="btn btn-md" onClick={() => { setItems([]); setScanResult(null); }}>清空所有</button>
      </div>

      {/* Replace items */}
      <div className="replace-content">
        {items.length === 0 && (
          <div className="replace-content-empty">暂无替换单元，点击"添加替换单元"开始</div>
        )}
        {items.map((item, i) => {
          if (isSeparatorItem(item)) {
            return (
              <SerialSeparatorBar
                key={i}
                index={i}
                total={items.length}
                onRemove={() => removeItem(i)}
                onMoveUp={() => moveItem(i, -1)}
                onMoveDown={() => moveItem(i, 1)}
              />
            );
          }
          const ui = unitIndexByItemIndex.get(i) ?? 0;
          return (
            <ReplaceUnitCard
              key={i}
              unit={item}
              unitIndex={ui}
              index={i}
              total={items.length}
              onChange={(nu) => updateItem(i, nu)}
              onRemove={() => removeItem(i)}
              onMoveUp={() => moveItem(i, -1)}
              onMoveDown={() => moveItem(i, 1)}
              scanSummary={scanResult?.per_unit?.[ui]}
              currentBlocks={materialBlocks}
            />
          );
        })}
      </div>

      {/* Footer */}
      <div className="replace-footer">
        <div className="replace-footer-buttons">
          <div className="replace-unit-spacer" />
          <button className="btn btn-action" onClick={handleScan} disabled={unitItems.length === 0}>
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
                    <strong>{unitItems[i]?.label || `替换单元 ${i + 1}`}:</strong> 命中 {u.hit_count} 个方块
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
