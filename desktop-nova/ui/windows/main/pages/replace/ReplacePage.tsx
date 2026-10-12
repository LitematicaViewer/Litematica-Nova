import { ScrollArea } from "../../../../components/ui/scroll-area";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import React, { useEffect, useRef, useState } from "react";
import { takeUpdateDraft, useUpdateDraft } from "../../updateRecovery";
import { BlockSearchSelect } from "../../../../components/BlockSearchSelect";
import { Dialog } from "../../../../components/Dialog";
import { Dropdown } from "../../../../components/Dropdown";
import {
  getBlockProperties, translateKey, translateValue, translateBlockId,
  applyReplaceBlocks, checkFileExists, dryRunReplaceBlocks, selectLitematicSavePath,
} from "../../../../../src/business/facade";
import "./replace.css";

type Rule = {
  match: { name: string; properties: Record<string, string> };
  replace: { name: string; properties: Record<string, string> };
  property_mode: string;
};
type ReplaceSummary = {
  block_positions_affected: number;
  regions_scanned: number;
  warnings: string[];
  per_rule: { rule_index: number; match_name: string; replace_name: string; affected_block_count: number }[];
};
type Preview = { source: string; rules: Rule[]; raw: string; summary: ReplaceSummary | null };

function parseSummary(raw: string): ReplaceSummary | null {
  try {
    const data = JSON.parse(raw);
    return typeof data.block_positions_affected === "number" ? data : null;
  } catch { return null; }
}

function PropertySelector({ blockId, value, onChange, isTo }: {
  blockId: string; value: Record<string, string>; onChange: (value: Record<string, string>) => void; isTo: boolean;
}) {
  const props = getBlockProperties(blockId);
  const keys = Object.keys(props).sort();
  if (keys.length === 0) return <span className="muted">这个方块没有可选状态。</span>;
  return <div className="replace-property-list">{keys.map((key) => (
    <div className="replace-property" key={key}>
      <span>{translateKey(key)}</span>
      <Dropdown value={value[key] || (isTo ? "$keep" : "")} options={[
        ...(isTo ? [{ label: "保持原状态", value: "$keep" }] : [{ label: "全部状态", value: "" }]),
        ...props[key].map((item) => ({ label: `${translateValue(key, item)} (${item})`, value: item })),
      ]} onChange={(item) => {
        const next = { ...value };
        if (!item) delete next[key]; else next[key] = item;
        onChange(next);
      }} />
    </div>
  ))}</div>;
}

function RuleRow({ rule, index, onChange, onRemove, onDuplicate }: {
  rule: Rule; index: number; onChange: (rule: Rule) => void; onRemove: () => void; onDuplicate: () => void;
}) {
  return <div className="rule-row replace-rule">
    <div className="replace-rule-main">
      <div className="replace-block">
        <strong>把这个方块</strong>
        <BlockSearchSelect value={rule.match.name} onChange={(name) => onChange({ ...rule, match: { name, properties: {} } })} />
      </div>
      <div className="replace-block">
        <strong>替换成</strong>
        <BlockSearchSelect value={rule.replace.name} onChange={(name) => onChange({ ...rule, replace: { name, properties: {} } })} />
      </div>
      <div className="replace-rule-actions">
        <Button variant="secondary" aria-label={`复制规则 ${index + 1}`} onClick={onDuplicate}>复制</Button>
        <Button variant="ghost" aria-label={`删除规则 ${index + 1}`} onClick={onRemove}>删除</Button>
      </div>
    </div>
    <details className="replace-advanced">
      <summary>高级 · 方块状态与手动 ID</summary>
      <div className="replace-rule-main">
        <div className="replace-block">
          <label>原方块 ID（可用通配符）<Input className="input" value={rule.match.name} onChange={(event) => onChange({ ...rule, match: { name: event.target.value, properties: {} } })} /></label>
          <PropertySelector blockId={rule.match.name} value={rule.match.properties} onChange={(properties) => onChange({ ...rule, match: { ...rule.match, properties } })} isTo={false} />
        </div>
        <div className="replace-block">
          <label>目标方块 ID<Input className="input" value={rule.replace.name} onChange={(event) => onChange({ ...rule, replace: { name: event.target.value, properties: {} } })} /></label>
          <PropertySelector blockId={rule.replace.name} value={rule.replace.properties} onChange={(properties) => onChange({ ...rule, replace: { ...rule.replace, properties } })} isTo />
          <Dropdown value={rule.property_mode} options={[
            { label: "保持同名状态", value: "merge" },
            { label: "使用目标默认状态", value: "drop" },
            { label: "仅使用手动指定状态", value: "replace" },
          ]} onChange={(property_mode) => onChange({ ...rule, property_mode })} />
        </div>
      </div>
    </details>
  </div>;
}

export function ReplacePage({ currentFile }: any) {
  const [rules, setRules] = useState<Rule[]>([]);
  const [log, setLog] = useState("");
  const [error, setError] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [outPath, setOutPath] = useState("");
  useUpdateDraft(`replace:${currentFile}`, { rules, outPath });
  const [busy, setBusy] = useState<"preview" | "apply" | "choose" | null>(null);
  const busyRef = useRef(false);
  const sourceRef = useRef(currentFile);
  const recoverySource = useRef<string | null>(null);
  sourceRef.current = currentFile;

  useEffect(() => {
    if (recoverySource.current === currentFile) return;
    recoverySource.current = currentFile;
    setPreview(null); setError(""); setLog("");
    const recovered = takeUpdateDraft<{ rules: Rule[]; outPath: string }>(`replace:${currentFile}`);
    setOutPath(recovered?.outPath || "");
    if (recovered) setRules(recovered.rules);
  }, [currentFile]);

  const changeRules = (next: Rule[]) => {
    if (busyRef.current) return;
    setRules(next); setPreview(null); setError(""); setLog("");
  };
  const handleAdd = () => changeRules([...rules, {
    match: { name: "minecraft:stone", properties: {} },
    replace: { name: "minecraft:stone_bricks", properties: {} }, property_mode: "merge",
  }]);

  const handleDryRun = async () => {
    if (busyRef.current || !rules.length || !currentFile) return;
    const source = currentFile;
    const snapshot: Rule[] = JSON.parse(JSON.stringify(rules));
    busyRef.current = true; setBusy("preview"); setError(""); setLog(""); setPreview(null);
    try {
      const raw = await dryRunReplaceBlocks(source, snapshot);
      if (sourceRef.current !== source) return;
      setPreview({ source, rules: snapshot, raw, summary: parseSummary(raw) });
      setOutPath(source.replace(/\.litematic$/i, ".replaced.litematic"));
    } catch (err) {
      if (sourceRef.current === source) setError(`预览未完成：${String(err)}`);
    } finally { busyRef.current = false; setBusy(null); }
  };

  const handleChooseOutput = async () => {
    if (busyRef.current) return;
    busyRef.current = true; setBusy("choose");
    const source = currentFile;
    try {
      const selected = await selectLitematicSavePath(outPath);
      if (selected && sourceRef.current === source) { setOutPath(selected); setError(""); }
    } catch (err) { setError(`选择保存位置失败：${String(err)}`); }
    finally { busyRef.current = false; setBusy(null); }
  };

  const handleApply = async () => {
    if (busyRef.current || !preview || preview.source !== currentFile) return;
    const samePath = (path: string) => path.replace(/\//g, "\\").toLowerCase();
    if (!outPath || samePath(outPath) === samePath(currentFile)) { setError("请选择一个新的文件名，原投影将保持不变。"); return; }
    const source = preview.source;
    busyRef.current = true; setBusy("apply"); setError("");
    try {
      if (await checkFileExists(outPath)) { setError("这个文件已存在，请选择另一个文件名。"); return; }
      const result = await applyReplaceBlocks(source, outPath, preview.rules);
      if (sourceRef.current !== source) return;
      setPreview(null);
      setLog(`替换完成，原投影未改动。\n新文件：${outPath}\n\n${result}`);
    } catch (err) {
      if (sourceRef.current === source) setError(`替换未完成：${String(err)}`);
    } finally { busyRef.current = false; setBusy(null); }
  };

  if (!currentFile) return <div className="replace-empty"><h2>替换方块</h2><p>先在投影库打开一个投影，再选择要替换的方块。</p></div>;

  return <section className="replace-page" aria-busy={!!busy}>
    <h2 className="replace-title">替换方块</h2>
    <p className="replace-intro">选择原方块和新方块，预览影响后保存为新文件。原投影保持不变。</p>
    {error && !preview && <div className="replace-error" role="alert">{error}</div>}
    <ScrollArea className="replace-scroll-area" aria-label="替换规则">
    <fieldset className="replace-rules" disabled={!!busy}>
      <legend className="replace-legend">替换规则</legend>
      {!rules.length && <div className="replace-empty"><p>还没有替换规则</p><Button variant="secondary" onClick={handleAdd}>选择要替换的方块</Button></div>}
      {rules.map((rule, index) => <RuleRow key={index} rule={rule} index={index}
        onChange={(next) => changeRules(rules.map((item, i) => i === index ? next : item))}
        onRemove={() => changeRules(rules.filter((_, i) => i !== index))}
        onDuplicate={() => changeRules([...rules, JSON.parse(JSON.stringify(rule))])} />)}
    </fieldset>
    {log && <div className="replace-result" role="status"><strong>替换完成，原投影未改动。</strong><details><summary>查看保存位置与详细结果</summary><pre>{log}</pre></details></div>}
    </ScrollArea>
    <div className="replace-toolbar">
      <Button variant="secondary" disabled={!!busy} onClick={handleAdd}>添加规则</Button>
      <Button variant="ghost" disabled={!!busy || !rules.length} onClick={() => changeRules([])}>清空规则</Button>
      <span className="muted">{rules.length} 条规则</span>
      <Button variant="default" disabled={!!busy || !rules.length} onClick={handleDryRun}>{busy === "preview" ? "正在检查影响…" : "预览替换影响"}</Button>
    </div>
    {preview && <Dialog title="确认替换" subtitle="确认影响范围，保存为新的投影文件。" busy={!!busy} onClose={() => setPreview(null)} width="lg" footer={<>
      <Button variant="secondary" disabled={!!busy} onClick={() => setPreview(null)}>返回修改</Button>
      <Button variant="default" disabled={!!busy || !outPath} onClick={handleApply}>{busy === "apply" ? "正在保存…" : "确认替换并另存"}</Button>
    </>}>
      {preview.summary ? <div className="replace-preview">
        <p><strong>{preview.summary.block_positions_affected.toLocaleString()} 个方块</strong> 将发生变化，已检查 {preview.summary.regions_scanned} 个区域。</p>
        {(preview.summary.per_rule || []).map((item) => <div key={item.rule_index}>{translateBlockId(item.match_name)} → {translateBlockId(item.replace_name)}：{item.affected_block_count.toLocaleString()} 个</div>)}
        {(preview.summary.warnings || []).length > 0 && <div role="alert">{preview.summary.warnings.map((warning, index) => <p key={index}>{warning}</p>)}</div>}
        <details><summary>详细检查结果</summary><pre>{preview.raw}</pre></details>
      </div> : <pre className="replace-preview-raw">{preview.raw}</pre>}
      <label className="replace-output">新文件保存位置<Input className="input" value={outPath} readOnly /></label>
      <Button variant="secondary" disabled={!!busy} onClick={handleChooseOutput}>更改保存位置…</Button>
      {error && <p className="replace-error" role="alert">{error}</p>}
    </Dialog>}
  </section>;
}
