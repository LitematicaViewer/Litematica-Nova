import { t, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
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
import { selectLitematicSavePath, loadStructuralBlocks, windowsPathKey, translateBlockId } from "../../../../../src/business/facade";
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
  const { locale } = useI18n();
  const props = getBlockProperties(blockId);
  const keys = Object.keys(props).sort();
  if (keys.length === 0) return <span className="replace-property-no-props">{t("replace.noProperties")}</span>;
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
              { value: "", label: isOutput ? t("replace.default") : t("replace.all") },
              ...(isOutput ? [{ value: "$keep", label: t("replace.keepState") }] : []),
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
  const { locale } = useI18n();
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
  const { locale } = useI18n();
  return (
    <div className="replace-entry-row">
      <BlockSearchSelect value={entry.name} onChange={name => onChange({...entry,name,properties:undefined})} />
      <div className="replace-entry-top">
        <BlockIcon blockId={entry.name} lookupMode="block_first" />

        <span className="replace-entry-weight-label">{t("replace.weight")}</span>
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
  const { locale } = useI18n();
  return (
    <div className="replace-separator">
      <div className="replace-separator-line" />
      <span className="replace-separator-label">{t("replace.nextStep")}</span>
      <div className="replace-separator-actions">
        <Button disabled={index === 0} onClick={onMoveUp} title={t("replace.moveUp")}>↑</Button>
        <Button disabled={index === total - 1} onClick={onMoveDown} title={t("replace.moveDown")}>↓</Button>
        <Button onClick={onRemove} title={t("replace.removeSeparator")}>{t("editor.delete")}</Button>
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
  const { locale } = useI18n();
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
        {t("replace.addCreative")}
      </Button>
      <Button onClick={() => setPicker({ side, type: 'enumerator' })}>
        {t("replace.addEnumerator")}
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
            placeholder={t("replace.ruleName", {index:unitIndex+1})}
            onChange={(e) => onChange({ ...unit, label: e.target.value || undefined })}
            onClick={(e) => e.stopPropagation()}
          />
          {scanSummary && (
            <span className="replace-unit-hit-count">{t("replace.hitCount", {count:scanSummary.hit_count.toLocaleString(locale)})}</span>
          )}
          <div className="replace-unit-spacer" />
          <Button disabled={index === 0} onClick={onMoveUp}>↑</Button>
          <Button disabled={index === total - 1} onClick={onMoveDown}>↓</Button>
          <Button onClick={onDuplicate}>{t("replace.copyRule")}</Button>
          <Button onClick={onRemove}>{t("replace.deleteRule")}</Button>
        </div>

        {!collapsed && (
          <div className="replace-unit-body">
            <div className="replace-unit-side">
              <div className="replace-unit-side-title">{t("replace.sourceBlocks")}</div>
              {unit.input.map((e, i) => (
                <InputEntryRow key={i} entry={e} onChange={(ne) => updateInput(i, ne)} onRemove={() => removeInput(i)} />
              ))}
              {unit.input.length === 0 && <div className="replace-entry-empty">{t("replace.noBlocks")}</div>}
              <AddEntryButtons side="input" />
            </div>
            <div className="replace-unit-arrow">→</div>
            <div className="replace-unit-side">
              <div className="replace-unit-side-title">{t("replace.targetBlocks")}</div>
              {unit.output.map((e, i) => (
                <OutputEntryRow key={i} entry={e} onChange={(ne) => updateOutput(i, ne)} onRemove={() => removeOutput(i)} />
              ))}
              {unit.output.length === 0 && <div className="replace-entry-empty">{t("replace.noTargets")}</div>}
              <AddEntryButtons side="output" />
            </div>
          </div>
        )}

        {!collapsed && scanSummary && scanSummary.output_distribution.length > 1 && (
          <div className="replace-unit-scan-dist">
            {t("replace.distribution")}
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
          title={picker.side === 'input' ? t("replace.chooseSource") : t("replace.chooseTarget")}
          onClose={() => setPicker(null)}
          onPickBlock={handlePickBlock}
          extraCollections={
            currentBlocks && currentBlocks.length > 0
              ? [{ id: '__current__', name: t("replace.currentResults"), values: currentBlocks }]
              : []
          }
        />
      )}

      {picker?.type === 'enumerator' && (
        <EnumeratorBlockPickerDialog
          title={picker.side === 'input' ? t("replace.enumeratorSource") : t("replace.enumeratorTarget")}
          onClose={() => setPicker(null)}
          onAddBlocks={handleAddBlocks}
          currentBlocks={currentBlocks}
        />
      )}
    </>
  );
}

export function ReplacePage({ currentFile }: { currentFile?: string }) {
  const { locale } = useI18n();
  const [session] = useState(loadReplaceSession);
  const [items, setItems] = useState<ReplaceItem[]>(session?.items || []);
  const [selectedPreset, setSelectedPreset] = useState(session?.selectedPreset || "");
  const [presetName, setPresetName] = useState(session?.selectedPreset || "");
  const [presets, setPresets] = useState<string[]>([]);
  const [log, setLog] = useState<UiMessage>("");
  const [error, setError] = useState<UiMessage>("");
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
  useEffect(() => { listReplacePresets().then(setPresets).catch(error => setError({key:"replace.loadPresetFailed", params:{error:String(error)}})); }, []);
  useEffect(() => {
    let active = true;
    setCurrentBlocks([]);
    if (currentFile) loadStructuralBlocks(currentFile).then(blocks => { if (active) setCurrentBlocks(blocks.map(block => block.id)); })
      .catch(error => { if (active) setError({key:"replace.structureFailed", params:{error:String(error)}}); });
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
        if (!next) { setError({key:"replace.corruptPreset"}); return; }
        setItems(next); setPreview(null); setLog(""); setSelectedPreset(name); setPresetName(name);
      } else if (action === "save") {
        await saveReplacePreset(name, items); setPresets(await listReplacePresets()); setSelectedPreset(name); setPresetName(name); setLog({key:"replace.saved"});
      } else { await deleteReplacePreset(name); setPresets(await listReplacePresets()); setSelectedPreset(""); setDeleteConfirm(false); }
    } catch (error) { setError({key:"replace.presetFailed", params:{error:String(error)}}); }
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
    } catch (error) { if (sourceRef.current === source) setError({key:"replace.previewFailed", params:{error:String(error)}}); }
    finally { busyRef.current = false; setBusy(null); }
  };
  const handleChooseOutput = async () => {
    if (busyRef.current) return;
    busyRef.current = true; setBusy("choose"); const source = currentFile;
    try { const selected = await selectLitematicSavePath(outPath); if (selected && sourceRef.current === source) setOutPath(selected); }
    catch (error) { setError({key:"replace.chooseFailed", params:{error:String(error)}}); }
    finally { busyRef.current = false; setBusy(null); }
  };
  const handleApply = async () => {
    if (busyRef.current || !preview || preview.source !== currentFile) return;
    if (!outPath || windowsPathKey(outPath) === windowsPathKey(preview.source)) { setError({key:"replace.newNameRequired"}); return; }
    const source = preview.source;
    busyRef.current = true; setBusy("apply"); setError("");
    try {
      if (await checkFileExists(outPath)) { setError({key:"replace.fileExists"}); return; }
      const result = await applyReplaceUnits(source, outPath, preview.items);
      if (sourceRef.current !== source) return;
      setPreview(null); setLog({key:"replace.completed", params:{path:displayFilePath(outPath),result:String(result)}});
    } catch (error) { if (sourceRef.current === source) setError({key:"replace.failed", params:{error:String(error)}}); }
    finally { busyRef.current = false; setBusy(null); }
  };
  let unitIndex = 0;
  return <section className="replace-page" aria-busy={!!busy}>
    <h2 className="replace-title">{t("replace.title")}</h2>
    <fieldset className="replace-presets" disabled={!!busy}>
      <legend>{t("replace.preset")}</legend>
      <Dropdown value={selectedPreset} options={[{label:t("replace.choosePreset"),value:""}, ...presets.map(name => ({label:name,value:name}))]} onChange={name => void presetAction("load",name)} ariaLabel={t("replace.preset")} />
      <Input aria-label={t("replace.presetName")} placeholder={t("replace.presetName")} value={presetName} onChange={event => setPresetName(event.target.value)} />
      <Button variant="secondary" disabled={!presetName.trim() || !unitItems.length} onClick={() => void presetAction("save",presetName)}>{t("replace.savePreset")}</Button>
      <Button variant="destructive" disabled={!selectedPreset} onClick={() => setDeleteConfirm(true)}>{t("replace.deletePreset")}</Button>
      <Button variant="outline" onClick={() => openReplacePresetFolder().catch(error => setError({key:"replace.openFolderFailed", params:{error:String(error)}}))}>{t("replace.openPresetFolder")}</Button>
    </fieldset>
    {error && <pre className="replace-error nova-text-danger" role="alert">{displayPathText(localizeMessage(error))}</pre>}
    <ScrollArea className="replace-scroll-area" aria-label={t("replace.rules")}>
      <fieldset className="replace-rules nova-box-plain" disabled={!!busy}>
        <legend className="replace-legend">{t("replace.ruleGroups")}</legend>
        {!items.length && <p className="replace-empty">{t("replace.empty")}</p>}
        {items.map((item,index) => isSeparatorItem(item)
          ? <SerialSeparatorBar key={index} index={index} total={items.length} onRemove={() => changeItems(items.filter((_,i)=>i!==index))} onMoveUp={() => moveItem(index,-1)} onMoveDown={() => moveItem(index,1)} />
          : <ReplaceUnitCard key={index} unit={item} unitIndex={unitIndex++} index={index} total={items.length}
              onChange={next => changeItems(items.map((item,i)=>i===index?next:item))} onRemove={() => changeItems(items.filter((_,i)=>i!==index))}
              onDuplicate={() => changeItems([...items,JSON.parse(JSON.stringify(item))])}
              onMoveUp={() => moveItem(index,-1)} onMoveDown={() => moveItem(index,1)} currentBlocks={currentBlocks} />)}
      </fieldset>
      {log && <div className="replace-result nova-panel-subtle" role="status"><details open><summary>{t("replace.result")}</summary><pre className="nova-code">{displayPathText(localizeMessage(log))}</pre></details></div>}
    </ScrollArea>
    <div className="replace-toolbar">
      <Button variant="secondary" disabled={!!busy} onClick={() => changeItems([...items,{input:[],output:[]}])}>{t("replace.addRule")}</Button>
      <Button variant="secondary" disabled={!!busy || !items.length} onClick={() => changeItems([...items,{_kind:"separator"}])}>{t("replace.addStep")}</Button>
      <Button variant="destructive" disabled={!!busy || !items.length} onClick={() => changeItems([])}>{t("replace.clearRules")}</Button>
      <span className="muted">{t("replace.ruleCount", {count:unitItems.length.toLocaleString(locale)})}</span>
      <Button disabled={!!busy || !unitItems.length || !currentFile} onClick={handleDryRun}>{busy === "preview" ? t("replace.checking") : t("replace.preview")}</Button>
    </div>
    {!currentFile && <p className="beta-statistics-status">{t("replace.chooseSchematic")}</p>}
    {deleteConfirm && <Dialog title={t("replace.deleteTitle")} onClose={() => setDeleteConfirm(false)} busy={!!busy} footer={<Button disabled={!!busy} onClick={() => void presetAction("delete",selectedPreset)}>{t("replace.confirmDelete")}</Button>}><p>{t("replace.deletePrompt", {name:selectedPreset})}</p></Dialog>}
    {preview && <Dialog title={t("replace.confirmTitle")} subtitle={t("replace.confirmSubtitle")} busy={!!busy} onClose={() => setPreview(null)} width="lg" footer={<>
      <Button variant="secondary" disabled={!!busy} onClick={() => setPreview(null)}>{t("replace.returnEdit")}</Button>
      <Button disabled={!!busy || !outPath} onClick={handleApply}>{busy === "apply" ? t("editor.saving") : t("replace.confirmSave")}</Button>
    </>}>
      <div className="replace-preview"><p>{t("replace.previewSummary", {regions:preview.summary.regions_scanned.toLocaleString(locale),blocks:preview.summary.block_positions_affected.toLocaleString(locale)})}</p>
      {preview.summary.per_unit.map((unit,index) => <section key={index}><strong>{unitItems[index]?.label || t("replace.ruleName", {index:index+1})}</strong>: {t("replace.hitCount", {count:unit.hit_count.toLocaleString(locale)})}
        <ul>{unit.output_distribution.map(output => <li key={output.output_index}>{t("replace.outputEstimate", {name:translateBlockId(output.name),count:output.estimated_count.toLocaleString(locale)})}</li>)}</ul>
        {!!unit.warnings.length && <p>{unit.warnings.map(localizeMessage).join(locale === "zh-CN" ? "；" : "; ")}</p>}{!!unit.invalid_entries.length && <p role="alert">{unit.invalid_entries.map(localizeMessage).join(locale === "zh-CN" ? "；" : "; ")}</p>}
      </section>)}
      {!!preview.summary.warnings.length && <p role="alert">{preview.summary.warnings.map(localizeMessage).join(locale === "zh-CN" ? "；" : "; ")}</p>}
      <details><summary>{t("replace.details")}</summary><pre className="nova-code">{JSON.stringify(preview.summary,null,2)}</pre></details></div>
      <label className="replace-output">{t("replace.outputPath")}<Input value={displayFilePath(outPath)} readOnly /></label>
      <Button variant="secondary" disabled={!!busy} onClick={handleChooseOutput}>{t("editor.changeSavePath")}</Button>
      {error && <pre className="replace-error nova-text-danger" role="alert">{displayPathText(localizeMessage(error))}</pre>}
    </Dialog>}
  </section>;
}
