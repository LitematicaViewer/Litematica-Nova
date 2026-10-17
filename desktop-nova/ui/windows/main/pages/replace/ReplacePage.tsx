import { displayPathText } from "../../../../../src/business/facade";
import { displayFilePath } from "../../../../../src/business/facade";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { takeUpdateDraft, useUpdateDraft } from "../../updateRecovery";
import { Dialog } from "../../../../components/Dialog";
import { Dropdown } from "../../../../components/Dropdown";
import { BlockSearchSelect } from "../../../../components/BlockSearchSelect";
import { BlockIcon } from "../../../../components/BlockIcon";
import { BlockPickerDialog } from "../../../../components/BlockPickerDialog";
import { EnumeratorBlockPickerDialog } from "./EnumeratorBlockPickerDialog";
import {
  getBlockProperties, translateKey, translateValue, checkFileExists,
  dryRunReplaceUnits, applyReplaceUnits, listReplacePresets, saveReplacePreset, loadReplacePreset,
  deleteReplacePreset, openReplacePresetFolder, isSeparatorItem,
  type ReplaceUnit, type ReplaceItem, type ReplaceEntry, type ReplaceOutputEntry,
  type ReplacePreviewSummary, type UnitPreviewSummary,
} from "../../../../../src/business/replace";
import { selectLitematicSavePath, loadStructuralBlocks, windowsPathKey } from "../../../../../src/business/facade";
import { loadReplaceSession, saveReplaceSession } from "../../../../../src/services/replaceSessionStore";
import "./replace.css";
import "../../../../styles/replace-units.css";

type Preview = { source: string; items: ReplaceItem[]; summary: ReplacePreviewSummary };
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
          <Dropdown
            ariaLabel={translateKey(k)}
            value={value[k] || ""}
            onChange={(nextValue) => {
              const next = { ...value };
              if (nextValue) next[k] = nextValue; else delete next[k];
              onChange(next);
            }}
            options={[
              { value: "", label: isOutput ? "默认" : "所有" },
              ...(isOutput ? [{ value: "$keep", label: "保持原状态" }] : []),
              ...props[k].map(v => ({ value: v, label: `${translateValue(k, v)} (${v})` })),
            ]}
          />
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
      <BlockSearchSelect value={entry.name} onChange={name => onChange({...entry,name,properties:undefined})} />
      <div className="replace-entry-top">
        <BlockIcon blockId={entry.name} lookupMode="block_first" />

        <Button className="replace-entry-remove-btn" onClick={onRemove}>×</Button>
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
      <BlockSearchSelect value={entry.name} onChange={name => onChange({...entry,name,properties:undefined})} />
      <div className="replace-entry-top">
        <BlockIcon blockId={entry.name} lookupMode="block_first" />

        <span className="replace-entry-weight-label">比例</span>
        <Input
          className="input replace-entry-weight-input"
          type="number"
          min={0}
          value={entry.weight}
          onChange={(e) => onChange({ ...entry, weight: Math.max(0, parseInt(e.target.value) || 0) })}
        />
        <Button className="replace-entry-remove-btn" onClick={onRemove}>×</Button>
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
      <span className="replace-separator-label">下一步替换</span>
      <div className="replace-separator-actions">
        <Button disabled={index === 0} onClick={onMoveUp} title="上移">↑</Button>
        <Button disabled={index === total - 1} onClick={onMoveDown} title="下移">↓</Button>
        <Button onClick={onRemove} title="删除分隔符">删除</Button>
      </div>
      <div className="replace-separator-line" />
    </div>
  );
}

// ── ReplaceUnitCard ──────────────────────────────────────────────────────────

type PickerTarget = { side: 'input' | 'output'; type: 'inventory' | 'enumerator' } | null;

function ReplaceUnitCard({ unit, unitIndex, index, total, onChange, onRemove, onDuplicate, onMoveUp, onMoveDown, scanSummary, currentBlocks }: {
  unit: ReplaceUnit;
  /** 当前单元在所有单元（不含分隔符）中的顺序，用于占位标签显示。 */
  unitIndex: number;
  index: number;
  total: number;
  onChange: (u: ReplaceUnit) => void;
  onRemove: () => void;
  onDuplicate: () => void;
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
      <Button onClick={() => setPicker({ side, type: 'inventory' })}>
        + 创造模式物品栏
      </Button>
      <Button onClick={() => setPicker({ side, type: 'enumerator' })}>
        + 枚举器
      </Button>
    </div>
  );

  return (
    <>
      <div className="card replace-unit-card">
        <div className={`replace-unit-header${collapsed ? ' collapsed' : ''}`}>
          <Button onClick={() => setCollapsed(!collapsed)}>
            {collapsed ? '▶' : '▼'}
          </Button>
          <Input
            className="replace-unit-label-input"
            value={unit.label ?? ''}
            placeholder={`替换规则 ${unitIndex + 1}`}
            onChange={(e) => onChange({ ...unit, label: e.target.value || undefined })}
            onClick={(e) => e.stopPropagation()}
          />
          {scanSummary && (
            <span className="replace-unit-hit-count">命中 {scanSummary.hit_count} 个方块</span>
          )}
          <div className="replace-unit-spacer" />
          <Button disabled={index === 0} onClick={onMoveUp}>↑</Button>
          <Button disabled={index === total - 1} onClick={onMoveDown}>↓</Button>
          <Button onClick={onDuplicate}>复制规则</Button>
          <Button onClick={onRemove}>删除规则</Button>
        </div>

        {!collapsed && (
          <div className="replace-unit-body">
            <div className="replace-unit-side">
              <div className="replace-unit-side-title">要替换的方块</div>
              {unit.input.map((e, i) => (
                <InputEntryRow key={i} entry={e} onChange={(ne) => updateInput(i, ne)} onRemove={() => removeInput(i)} />
              ))}
              {unit.input.length === 0 && <div className="replace-entry-empty">未选择方块</div>}
              <AddEntryButtons side="input" />
            </div>
            <div className="replace-unit-arrow">→</div>
            <div className="replace-unit-side">
              <div className="replace-unit-side-title">替换成</div>
              {unit.output.map((e, i) => (
                <OutputEntryRow key={i} entry={e} onChange={(ne) => updateOutput(i, ne)} onRemove={() => removeOutput(i)} />
              ))}
              {unit.output.length === 0 && <div className="replace-entry-empty">未选择替换结果</div>}
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
          title={picker.side === 'input' ? '选择要替换的方块' : '选择替换成的方块'}
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
          currentBlocks={currentBlocks}
        />
      )}
    </>
  );
}

export function ReplacePage({ currentFile }: { currentFile?: string }) {
  const [session] = useState(loadReplaceSession);
  const [items, setItems] = useState<ReplaceItem[]>(session?.items || []);
  const [selectedPreset, setSelectedPreset] = useState(session?.selectedPreset || "");
  const [presetName, setPresetName] = useState(session?.selectedPreset || "");
  const [presets, setPresets] = useState<string[]>([]);
  const [log, setLog] = useState("");
  const [error, setError] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [outPath, setOutPath] = useState(session?.outputPath || "");
  const [currentBlocks, setCurrentBlocks] = useState<string[]>([]);
  const [deleteConfirm, setDeleteConfirm] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const busyRef = useRef(false);
  const sourceRef = useRef(currentFile);
  sourceRef.current = currentFile;
  const recoverySource = useRef<string | null>(null);
  const unitItems = useMemo(() => items.filter((item): item is ReplaceUnit => !isSeparatorItem(item)), [items]);
  useUpdateDraft(`replace:${currentFile}`, { items, outPath, selectedPreset });
  useEffect(() => { saveReplaceSession({ items, selectedPreset, outputPath: outPath }); }, [items, selectedPreset, outPath]);
  useEffect(() => {
    if (recoverySource.current === currentFile) return;
    recoverySource.current = currentFile || "";
    setPreview(null); setError(""); setLog("");
    const recovered = takeUpdateDraft<{ items?: ReplaceItem[]; rules?: Array<{match:ReplaceEntry;replace:ReplaceEntry}>; outPath:string; selectedPreset?:string }>(`replace:${currentFile}`);
    if (recovered) {
      setItems(recovered.items || (recovered.rules || []).map(rule => ({ input:[rule.match], output:[{...rule.replace,weight:1}] })));
      setOutPath(recovered.outPath || ""); setSelectedPreset(recovered.selectedPreset || "");
    }
  }, [currentFile]);
  useEffect(() => { listReplacePresets().then(setPresets).catch(error => setError(`读取预设失败：${error}`)); }, []);
  useEffect(() => {
    let active = true;
    setCurrentBlocks([]);
    if (currentFile) loadStructuralBlocks(currentFile).then(blocks => { if (active) setCurrentBlocks(blocks.map(block => block.id)); })
      .catch(error => { if (active) setError(`读取结构方块失败：${error}`); });
    return () => { active = false; };
  }, [currentFile]);
  const changeItems = (next: ReplaceItem[]) => { if (!busyRef.current) { setItems(next); setPreview(null); setError(""); setLog(""); } };
  const moveItem = (index:number, direction:number) => {
    const target = index + direction;
    if (target < 0 || target >= items.length) return;
    const next = [...items]; [next[index], next[target]] = [next[target], next[index]]; changeItems(next);
  };
  const presetAction = async (action: "save" | "load" | "delete", name:string) => {
    if (busyRef.current || !name.trim()) return;
    busyRef.current = true; setBusy(action); setError("");
    try {
      if (action === "load") {
        const next = await loadReplacePreset(name);
        if (!next) throw new Error("预设格式损坏或文件不存在");
        setItems(next); setPreview(null); setLog(""); setSelectedPreset(name); setPresetName(name);
      } else if (action === "save") {
        await saveReplacePreset(name, items); setPresets(await listReplacePresets()); setSelectedPreset(name); setPresetName(name); setLog("替换方案已保存");
      } else { await deleteReplacePreset(name); setPresets(await listReplacePresets()); setSelectedPreset(""); setDeleteConfirm(false); }
    } catch (error) { setError(`预设操作失败：${error}`); }
    finally { busyRef.current = false; setBusy(null); }
  };
  const handleDryRun = async () => {
    if (busyRef.current || !unitItems.length || !currentFile) return;
    const source = currentFile; const snapshot: ReplaceItem[] = JSON.parse(JSON.stringify(items));
    busyRef.current = true; setBusy("preview"); setError(""); setLog(""); setPreview(null);
    try {
      const summary = await dryRunReplaceUnits(source, snapshot);
      if (sourceRef.current !== source) return;
      setPreview({ source, items:snapshot, summary });
      if (!outPath) setOutPath(source.replace(/\.litematic$/i, ".replaced.litematic"));
    } catch (error) { if (sourceRef.current === source) setError(`预览未完成：${error}`); }
    finally { busyRef.current = false; setBusy(null); }
  };
  const handleChooseOutput = async () => {
    if (busyRef.current) return;
    busyRef.current = true; setBusy("choose"); const source = currentFile;
    try { const selected = await selectLitematicSavePath(outPath); if (selected && sourceRef.current === source) setOutPath(selected); }
    catch (error) { setError(`选择保存位置失败：${error}`); }
    finally { busyRef.current = false; setBusy(null); }
  };
  const handleApply = async () => {
    if (busyRef.current || !preview || preview.source !== currentFile) return;
    if (!outPath || windowsPathKey(outPath) === windowsPathKey(preview.source)) { setError("请选择新文件名，原投影保持不变。"); return; }
    const source = preview.source;
    busyRef.current = true; setBusy("apply"); setError("");
    try {
      if (await checkFileExists(outPath)) { setError("此文件已存在，请选择另一个文件名。"); return; }
      const result = await applyReplaceUnits(source, outPath, preview.items);
      if (sourceRef.current !== source) return;
      setPreview(null); setLog(`替换完成，原投影未改动。\n新文件：${displayFilePath(outPath)}\n\n${result}`);
    } catch (error) { if (sourceRef.current === source) setError(`替换未完成：${error}`); }
    finally { busyRef.current = false; setBusy(null); }
  };
  let unitIndex = 0;
  return <section className="replace-page" aria-busy={!!busy}>
    <h2 className="replace-title">替换方块</h2>
    <fieldset className="replace-presets" disabled={!!busy}>
      <legend>替换方案</legend>
      <Dropdown value={selectedPreset} options={[{label:"选择方案",value:""}, ...presets.map(name => ({label:name,value:name}))]} onChange={name => void presetAction("load",name)} ariaLabel="替换方案" />
      <Input aria-label="方案名称" placeholder="方案名称" value={presetName} onChange={event => setPresetName(event.target.value)} />
      <Button variant="secondary" disabled={!presetName.trim() || !unitItems.length} onClick={() => void presetAction("save",presetName)}>保存方案</Button>
      <Button variant="ghost" disabled={!selectedPreset} onClick={() => setDeleteConfirm(true)}>删除方案</Button>
      <Button variant="ghost" onClick={() => openReplacePresetFolder().catch(error => setError(`打开方案文件夹失败：${error}`))}>打开方案文件夹</Button>
    </fieldset>
    {error && <pre className="replace-error" role="alert">{displayPathText(error)}</pre>}
    <ScrollArea className="replace-scroll-area" aria-label="替换规则">
      <fieldset className="replace-rules" disabled={!!busy}>
        <legend className="replace-legend">替换组规则</legend>
        {!items.length && <p className="replace-empty">尚未添加替换规则。</p>}
        {items.map((item,index) => isSeparatorItem(item)
          ? <SerialSeparatorBar key={index} index={index} total={items.length} onRemove={() => changeItems(items.filter((_,i)=>i!==index))} onMoveUp={() => moveItem(index,-1)} onMoveDown={() => moveItem(index,1)} />
          : <ReplaceUnitCard key={index} unit={item} unitIndex={unitIndex++} index={index} total={items.length}
              onChange={next => changeItems(items.map((item,i)=>i===index?next:item))} onRemove={() => changeItems(items.filter((_,i)=>i!==index))}
              onDuplicate={() => changeItems([...items,JSON.parse(JSON.stringify(item))])}
              onMoveUp={() => moveItem(index,-1)} onMoveDown={() => moveItem(index,1)} currentBlocks={currentBlocks} />)}
      </fieldset>
      {log && <div className="replace-result" role="status"><details open><summary>替换操作结果</summary><pre>{displayPathText(log)}</pre></details></div>}
    </ScrollArea>
    <div className="replace-toolbar">
      <Button variant="secondary" disabled={!!busy} onClick={() => changeItems([...items,{input:[],output:[]}])}>添加替换规则</Button>
      <Button variant="secondary" disabled={!!busy || !items.length} onClick={() => changeItems([...items,{_kind:"separator"}])}>添加下一步替换</Button>
      <Button variant="ghost" disabled={!!busy || !items.length} onClick={() => changeItems([])}>清空规则</Button>
      <span className="muted">{unitItems.length} 个替换规则</span>
      <Button disabled={!!busy || !unitItems.length || !currentFile} onClick={handleDryRun}>{busy === "preview" ? "正在检查影响…" : "预览替换影响"}</Button>
    </div>
    {!currentFile && <p className="beta-statistics-status">在投影库打开投影后即可预览和执行替换。</p>}
    {deleteConfirm && <Dialog title="删除替换方案" onClose={() => setDeleteConfirm(false)} busy={!!busy} footer={<Button disabled={!!busy} onClick={() => void presetAction("delete",selectedPreset)}>确认删除方案</Button>}><p>删除方案“{selectedPreset}”，当前规则保持不变。</p></Dialog>}
    {preview && <Dialog title="确认替换" subtitle="确认每一步影响，保存为新投影。" busy={!!busy} onClose={() => setPreview(null)} width="lg" footer={<>
      <Button variant="secondary" disabled={!!busy} onClick={() => setPreview(null)}>返回修改</Button>
      <Button disabled={!!busy || !outPath} onClick={handleApply}>{busy === "apply" ? "正在保存…" : "确认替换并另存"}</Button>
    </>}>
      <div className="replace-preview"><p>扫描 {preview.summary.regions_scanned} 个区域，预计执行 {preview.summary.block_positions_affected.toLocaleString()} 次方块替换操作。</p>
      {preview.summary.per_unit.map((unit,index) => <section key={index}><strong>{unitItems[index]?.label || `替换规则 ${index+1}`}</strong>：命中 {unit.hit_count.toLocaleString()} 个方块
        <ul>{unit.output_distribution.map(output => <li key={output.output_index}>{output.name}：预估 {output.estimated_count.toLocaleString()}</li>)}</ul>
        {!!unit.warnings.length && <p>{unit.warnings.join("；")}</p>}{!!unit.invalid_entries.length && <p role="alert">{unit.invalid_entries.join("；")}</p>}
      </section>)}
      {!!preview.summary.warnings.length && <p role="alert">{preview.summary.warnings.join("；")}</p>}
      <details><summary>详细检查结果</summary><pre>{JSON.stringify(preview.summary,null,2)}</pre></details></div>
      <label className="replace-output">新文件保存位置<Input value={displayFilePath(outPath)} readOnly /></label>
      <Button variant="secondary" disabled={!!busy} onClick={handleChooseOutput}>更改保存位置…</Button>
      {error && <pre className="replace-error" role="alert">{displayPathText(error)}</pre>}
    </Dialog>}
  </section>;
}
