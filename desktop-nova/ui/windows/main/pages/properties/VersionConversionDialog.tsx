import { t, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { ArrowRight } from "lucide-react";
import { convertProjectVersion, queryBlockDatabase, type ConversionPreview, type BlockDatabaseBlock, type BlockDatabaseVersion } from "../../../../../src/business/projectionLibrary";
import { saveDialog, getDataOperationStatus, type ApplicationDataOperation } from "../../../../../src/business/facade";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { BlockSearchSelect, BlockPropertySelector } from "../../../../components/BlockSearchSelect";
import { BlockIcon } from "../../../../components/BlockIcon";
import { Dropdown } from "../../../../components/Dropdown";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../../components/ui/table";

export function VersionConversionDialog({ sourcePath, onClose, onSaved }: {
  sourcePath: string; onClose: () => void; onSaved: (path: string) => void;
}) {
  const { locale, revision } = useI18n();
  const [versions, setVersions] = useState<BlockDatabaseVersion[]>([]);
  const [targetVersion, setTargetVersion] = useState("26.3");
  const [blocks, setBlocks] = useState<BlockDatabaseBlock[]>([]);
  const [preview, setPreview] = useState<ConversionPreview | null>(null);
  const [replacements, setReplacements] = useState<Record<string, string>>({});
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [progress, setProgress] = useState<ApplicationDataOperation["progress"]>();
  const [status, setStatus] = useState("");
  const operation = useRef<string | null>(null);
  const [overwriteConfirm, setOverwriteConfirm] = useState(false);
  const [error, setError] = useState<UiMessage>("");
  const generation = useRef(0);
  const alive = useRef(true);
  const allowedBlocks = useMemo(() => blocks.map((block) => block.id), [blocks]);
  const blockNames = useMemo(() => Object.fromEntries(blocks.map((block) => [block.id, block.name])), [blocks, revision]);
  const target = versions.find((version) => version.version === targetVersion);
  useEffect(() => {
    alive.current = true;
    void queryBlockDatabase({}).then((result) => { if (alive.current) setVersions(result.versions || []); }).catch((error) => { if (alive.current) setError(String(error)); });
    return () => { alive.current = false; generation.current++; if (operation.current) void getDataOperationStatus(operation.current, true).catch(() => undefined); };
  }, []);
  async function inspect(overrides: Record<string, string> = {}, loadBlocks = false, selection?: { sourceState: string; targetBlockId: string; bulk: boolean }, propertyEdit?: { sourceState: string; properties: Record<string,string> }) {
    const token = ++generation.current;
    if (operation.current) void getDataOperationStatus(operation.current, true).catch(() => undefined);
    const operationId = crypto.randomUUID(); operation.current = operationId;
    setBusy(true); setError(""); setDirty(true); setProgress(undefined); setStatus("");
    try {
      const [result, catalog] = await Promise.all([convertProjectVersion({ sourcePath, targetVersion, replacements: overrides, selection, propertyEdit }, operationId),
        loadBlocks ? queryBlockDatabase({ version: targetVersion }) : Promise.resolve(null)]);
      if (!alive.current || token !== generation.current) return;
      if (catalog) setBlocks(catalog.blocks || []);
      setPreview(result);
      setReplacements(result.effectiveReplacements);
      setDirty(false);
    } catch (error) { if (alive.current && token === generation.current) setError(String(error)); }
    finally { if (alive.current && token === generation.current) { setBusy(false); operation.current = null; } }
  }
  useEffect(() => { setPreview(null); setBlocks([]); setReplacements({}); setOverwriteConfirm(false); void inspect({}, true); }, [sourcePath, targetVersion]);
  function changeBlock(sourceState: string, targetBlockId: string, bulk = false) {
    void inspect(replacements, false, { sourceState, targetBlockId, bulk });
  }
  const cancelOperation = () => {
    generation.current += 1;
    const operationId = operation.current; operation.current = null;
    if (operationId) void getDataOperationStatus(operationId, true).catch(failure => setError(String(failure)));
    setBusy(false); setSaving(false); setDirty(true); setProgress(undefined); setStatus(t("conversion.cancelled"));
  };
  const close = () => { if (saving) return; cancelOperation(); onClose(); };
  useEffect(() => {
    if (!busy) return;
    let active = true;
    const timer = window.setInterval(() => {
      const operationId = operation.current; if (!operationId) return;
      void getDataOperationStatus(operationId).then(result => { if (active && operation.current === operationId) setProgress(result.progress); }).catch(() => undefined);
    }, 300);
    return () => { active = false; window.clearInterval(timer); };
  }, [busy]);
  async function save(overwrite = false) {
    if (!preview?.writable || dirty || busy) return;
    const token = generation.current;
    setBusy(true); setSaving(true); setError(""); setStatus(""); setProgress(undefined);
    try {
      const extension = target?.outputExtension || ".litematic";
      const path = overwrite ? sourcePath : await saveDialog({ title: t("conversion.saveTitle"), defaultPath: sourcePath.replace(/\.[^.\\/]+$/, `-${targetVersion}${extension}`), filters: [{ name: t("conversion.fileFilter"), extensions: [extension.slice(1)] }] });
      if (!path || !alive.current || token !== generation.current) return;
      const operationId = crypto.randomUUID(); operation.current = operationId;
      const result = await convertProjectVersion({ sourcePath, targetVersion, replacements, overwrite,
        expectedSourceSha256: preview.sourceSha256, outputPath: path }, operationId);
      if (alive.current && token === generation.current && result.outputPath) onSaved(result.outputPath);
    } catch (error) { if (alive.current && token === generation.current) setError(String(error)); }
    finally { if (alive.current && token === generation.current) { setBusy(false); setSaving(false); operation.current = null; } }
  }
  return <Dialog title={t("conversion.title")} width="lg" busy={saving} onClose={close}
    footer={<><Button variant="ghost" onClick={() => busy ? cancelOperation() : close()}>{t(busy ? "conversion.cancelOperation" : "editor.cancel")}</Button>
      <Button disabled={busy || !preview || !dirty} onClick={() => void inspect(replacements)}>{t("conversion.checkDifferences")}</Button>
      <Button variant="ghost" disabled={busy || dirty || !preview?.writable || target?.legacy} onClick={() => setOverwriteConfirm(true)}>{t("conversion.overwrite")}</Button>
      <Button disabled={busy || dirty || !preview?.writable} onClick={() => void save()}>{t("conversion.saveAs")}</Button></>}>
    <label className="properties-conversion-target">{t("conversion.target")}
      <Dropdown
        value={targetVersion}
        disabled={saving || !versions.length}
        ariaLabel={t("conversion.target")}
        onChange={(next) => { setDirty(true); setTargetVersion(next); }}
        options={versions.map((version) => ({ label: `Minecraft ${version.version}`, value: version.version }))}
      />
    </label>
    {busy && <p role="status">{progress?.total !== undefined ? t("conversion.progress", { current: progress.completed ?? progress.current ?? 0, total: progress.total }) : t("conversion.inspecting")}</p>}
    {status && <p role="status">{status}</p>}
    {error && <p role="alert" className="properties-error nova-text-danger">{localizeMessage(error)}</p>}
    {overwriteConfirm ? <div className="properties-overwrite-confirm"><p>{t("conversion.overwritePrompt")}</p>
      <Button disabled={busy} onClick={() => setOverwriteConfirm(false)}>{t("conversion.cancelOverwrite")}</Button><Button variant="destructive" disabled={busy} onClick={() => void save(true)}>{t("conversion.confirmOverwrite")}</Button>
    </div> : preview && <>
      <p>{t("conversion.summary", {source:preview.sourceVersion || String(preview.sourceDataVersion), target:preview.targetVersion, count:preview.changedBlockCount.toLocaleString(locale)})}</p>
      {!!preview.blockedPayloads.length && <div role="alert" className="properties-error nova-text-danger">
        {preview.payloadIssues?.length ? preview.payloadIssues.map((issue) => <p key={`${issue.kind}:${issue.typeName}`}>
          <strong>{t("conversion.issueCount", {type:issue.typeName, count:issue.count.toLocaleString(locale)})}</strong>: {localizeMessage(issue.message)}
        </p>) : <p>{t("conversion.payloadBlocked")}</p>}
      </div>}
      {preview.entries.length ? <Table className="properties-conversion-table"><TableHeader><TableRow>
        <TableHead>{t("conversion.sourceBlock")}</TableHead><TableHead>{t("conversion.targetBlock")}</TableHead><TableHead>{t("conversion.affectedCount")}</TableHead>
      </TableRow></TableHeader><TableBody>{preview.entries.map((entry) => {
        return <TableRow key={entry.sourceState}>
          <TableCell><div className="properties-conversion-source"><BlockIcon blockId={entry.sourceIconId || entry.sourceBlockId} /><strong>{entry.sourceName}</strong><ArrowRight size={16} aria-hidden="true" /></div><small>{localizeMessage(entry.reason)}</small></TableCell>
          <TableCell><BlockSearchSelect value={entry.targetBlockId || ""} allowedBlocks={allowedBlocks} blockNames={blockNames} disabled={busy || !blocks.length} onChange={(id) => changeBlock(entry.sourceState, id)} />
            {entry.targetBlockId && <BlockPropertySelector blockId={entry.targetBlockId} value={entry.targetProperties} properties={entry.targetPropertyOptions || {}} isTo exact onChange={(properties) => { void inspect(replacements, false, undefined, { sourceState: entry.sourceState, properties }); }} />}
            {entry.category && entry.targetBlockId && <Button size="sm" variant="ghost" disabled={busy} onClick={() => changeBlock(entry.sourceState, entry.targetBlockId!, true)}>{t("conversion.applyCategory")}</Button>}
            {!entry.valid && <span className="properties-error nova-text-danger">{t("conversion.chooseState")}</span>}</TableCell>
          <TableCell>{entry.count.toLocaleString(locale)}</TableCell>
        </TableRow>;
      })}</TableBody></Table> : <p>{t("conversion.compatible")}</p>}
      {dirty && <p role="status">{t("conversion.dirty")}</p>}
    </>}
  </Dialog>;
}
