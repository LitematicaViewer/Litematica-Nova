import React, { useEffect, useMemo, useRef, useState } from "react";
import { ArrowRight } from "lucide-react";
import { convertProjectVersion, queryBlockDatabase, minecraftVersionLabel, type ConversionPreview, type BlockDatabaseBlock, type BlockDatabaseVersion } from "../../../../../src/business/projectionLibrary";
import { saveDialog, loadDatabases, initI18n, translateBlockId } from "../../../../../src/business/facade";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { BlockSearchSelect, BlockPropertySelector } from "../../../../components/BlockSearchSelect";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../../components/ui/table";

function parseState(state: string) {
  const match = state.match(/^([^[]+)(?:\[(.*)\])?$/);
  return { id: match?.[1] || state, properties: Object.fromEntries((match?.[2] || "").split(",").filter(Boolean).map((pair) => pair.split("="))) };
}
function stateString(id: string, properties: Record<string, string>) {
  const entries = Object.entries(properties).sort(([a], [b]) => a.localeCompare(b));
  return entries.length ? `${id}[${entries.map(([key, value]) => `${key}=${value}`).join(",")}]` : id;
}
function targetState(block: BlockDatabaseBlock, original: Record<string, string>) {
  const properties = parseState(block.defaultState).properties;
  for (const [key, value] of Object.entries(original)) if (block.properties[key]?.includes(value)) properties[key] = value;
  return stateString(block.id, properties);
}
export function VersionConversionDialog({ sourcePath, onClose, onSaved }: {
  sourcePath: string; onClose: () => void; onSaved: (path: string) => void;
}) {
  const [versions, setVersions] = useState<BlockDatabaseVersion[]>([]);
  const [targetVersion, setTargetVersion] = useState("26.3");
  const [blocks, setBlocks] = useState<BlockDatabaseBlock[]>([]);
  const [preview, setPreview] = useState<ConversionPreview | null>(null);
  const [replacements, setReplacements] = useState<Record<string, string>>({});
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [overwriteConfirm, setOverwriteConfirm] = useState(false);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const alive = useRef(true);
  const byId = useMemo(() => new Map(blocks.map((block) => [block.id, block])), [blocks]);
  const allowedBlocks = useMemo(() => blocks.map((block) => block.id), [blocks]);
  const blockNames = useMemo(() => Object.fromEntries(blocks.map((block) => [block.id, block.name])), [blocks]);
  const target = versions.find((version) => version.version === targetVersion);
  useEffect(() => {
    void queryBlockDatabase({}).then((result) => { if (alive.current) setVersions((result.versions || []).filter((version) => version.stateCount > 0)); }).catch((error) => { if (alive.current) setError(String(error)); });
    void Promise.all([loadDatabases(), initI18n()]).catch((error) => console.warn("Conversion names:", error));
    return () => { alive.current = false; generation.current++; };
  }, []);
  async function inspect(overrides: Record<string, string> = {}, loadBlocks = false) {
    const token = ++generation.current;
    setBusy(true); setError(""); setDirty(true);
    try {
      const [result, catalog] = await Promise.all([convertProjectVersion({ sourcePath, targetVersion, replacements: overrides }),
        loadBlocks ? queryBlockDatabase({ version: targetVersion }) : Promise.resolve(null)]);
      if (!alive.current || token !== generation.current) return;
      if (catalog) setBlocks(catalog.blocks || []);
      setPreview(result);
      setReplacements(Object.fromEntries(result.entries.map((entry) => [entry.sourceState, entry.targetState || ""])));
      setDirty(false);
    } catch (error) { if (alive.current && token === generation.current) setError(String(error)); }
    finally { if (alive.current && token === generation.current) setBusy(false); }
  }
  useEffect(() => { setPreview(null); setBlocks([]); setReplacements({}); setOverwriteConfirm(false); void inspect({}, true); }, [sourcePath, targetVersion]);
  function changeBlock(source: string, id: string, bulk = false) {
    const block = byId.get(id); if (!block || !preview) return;
    const selected = preview.entries.find((entry) => entry.sourceState === source);
    setReplacements((current) => {
      const next = { ...current };
      for (const entry of preview.entries) if (entry.sourceState === source || (bulk && selected?.category && entry.category === selected.category))
        next[entry.sourceState] = targetState(block, parseState(entry.sourceState).properties);
      return next;
    }); setDirty(true);
  }
  async function save(overwrite = false) {
    if (!preview?.writable || dirty || busy) return;
    const token = generation.current;
    setBusy(true); setError("");
    try {
      const extension = target?.outputExtension || ".litematic";
      const path = overwrite ? sourcePath : await saveDialog({ title: "保存转换后的投影", defaultPath: sourcePath.replace(/\.[^.\\/]+$/, `-${targetVersion}${extension}`), filters: [{ name: "Minecraft 投影", extensions: [extension.slice(1)] }] });
      if (!path || !alive.current || token !== generation.current) return;
      const result = await convertProjectVersion({ sourcePath, targetVersion, replacements, overwrite,
        expectedSourceSha256: preview.sourceSha256, outputPath: path });
      if (alive.current && token === generation.current && result.outputPath) onSaved(result.outputPath);
    } catch (error) { if (alive.current && token === generation.current) setError(String(error)); }
    finally { if (alive.current && token === generation.current) setBusy(false); }
  }
  return <Dialog title="转换 Minecraft 版本" width="lg" busy={busy} onClose={onClose}
    footer={<><Button variant="ghost" disabled={busy} onClick={onClose}>取消</Button>
      <Button disabled={busy || !preview || !dirty} onClick={() => void inspect(replacements)}>检查差异</Button>
      <Button variant="ghost" disabled={busy || dirty || !preview?.writable || target?.legacy} onClick={() => setOverwriteConfirm(true)}>覆盖原件…</Button>
      <Button disabled={busy || dirty || !preview?.writable} onClick={() => void save()}>另存为…</Button></>}>
    <label className="properties-conversion-target">目标 Minecraft 版本
      <select value={targetVersion} disabled={busy || !versions.length} onChange={(event) => { setDirty(true); setTargetVersion(event.target.value); }}>
        {versions.map((version) => <option key={version.version} value={version.version}>Minecraft {version.version}</option>)}
      </select>
    </label>
    {busy && <p role="status">正在检查投影数据…</p>}
    {error && <p role="alert" className="properties-error">{error}</p>}
    {overwriteConfirm ? <div className="properties-overwrite-confirm"><p>覆盖原投影文件？此操作会替换原有版本数据。</p>
      <Button disabled={busy} onClick={() => setOverwriteConfirm(false)}>取消覆盖</Button><Button variant="destructive" disabled={busy} onClick={() => void save(true)}>确认覆盖</Button>
    </div> : preview && <>
      <p>{minecraftVersionLabel(preview.sourceDataVersion)} → Minecraft {preview.targetVersion} · 影响 {preview.changedBlockCount.toLocaleString()} 个方块</p>
      {!!preview.blockedPayloads.length && <div role="alert" className="properties-error">
        {preview.payloadIssues?.length ? preview.payloadIssues.map((issue) => <p key={`${issue.kind}:${issue.typeName}`}>
          <strong>{issue.typeName} · {issue.count.toLocaleString()} 处</strong>：{issue.message}
        </p>) : <p>附加数据需要处理，请选择其他目标版本。</p>}
      </div>}
      {preview.entries.length ? <Table className="properties-conversion-table"><TableHeader><TableRow>
        <TableHead>原方块</TableHead><TableHead>替代方块</TableHead><TableHead>影响数量</TableHead>
      </TableRow></TableHeader><TableBody>{preview.entries.map((entry) => {
        const source = parseState(entry.sourceIconState || entry.sourceState); const state = parseState(replacements[entry.sourceState] || ""); const block = byId.get(state.id);
        return <TableRow key={entry.sourceState}>
          <TableCell><div className="properties-conversion-source"><BlockIcon blockId={source.id} /><strong>{entry.sourceName || translateBlockId(source.id)}</strong><ArrowRight size={16} aria-hidden="true" /></div><small>{entry.reason}</small></TableCell>
          <TableCell><BlockSearchSelect value={state.id} allowedBlocks={allowedBlocks} blockNames={blockNames} disabled={busy || !blocks.length} onChange={(id) => changeBlock(entry.sourceState, id)} />
            {block && <BlockPropertySelector blockId={block.id} value={state.properties} properties={block.properties} isTo exact onChange={(properties) => { setReplacements((current) => ({ ...current, [entry.sourceState]: stateString(block.id, properties) })); setDirty(true); }} />}
            {entry.category && block && <Button size="sm" variant="ghost" disabled={busy} onClick={() => changeBlock(entry.sourceState, block.id, true)}>应用到同类</Button>}
            {!entry.valid && <span className="properties-error">请选择目标版本支持的方块状态</span>}</TableCell>
          <TableCell>{entry.count.toLocaleString()}</TableCell>
        </TableRow>;
      })}</TableBody></Table> : <p>方块与状态兼容。</p>}
      {dirty && <p role="status">替代项已修改，请检查差异。</p>}
    </>}
  </Dialog>;
}
