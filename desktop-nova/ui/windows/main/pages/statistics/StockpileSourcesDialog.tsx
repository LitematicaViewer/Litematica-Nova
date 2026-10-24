import React, { useEffect, useRef, useState } from "react";
import { FolderOpen, Plus, Trash2 } from "lucide-react";
import type { StockpileSourceInput, StockpileTaskView } from "../../../../../src/platform/applicationOperations";
import { openDialog } from "../../../../../src/platform/dialogs";
import { libraryRecordViews, loadLibrary, type LibraryRecordView } from "../../../../../src/services/libraryStore";
import { t } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Input } from "../../../../components/ui/input";
import { Dropdown } from "../../../../components/Dropdown";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../../components/ui/table";
import "./materials.css";

export interface StockpileSourcesSubmission { name: string; sources: StockpileSourceInput[]; requestId: string }
export interface StockpileSourcesDialogProps {
  mode: "create" | "manage" | "add"; initialSources?: StockpileSourceInput[]; task?: StockpileTaskView; defaultName?: string;
  onSubmit: (input: StockpileSourcesSubmission) => Promise<void>; onClose: () => void;
}
type DraftSource = { key: string; source: StockpileSourceInput; copies: string };
const draft = (source: StockpileSourceInput): DraftSource => ({ key: crypto.randomUUID(), source: {
  sourceId: source.sourceId, sourcePath: source.sourcePath, projectId: source.projectId, scope: { ...source.scope },
  includeContainerItems: source.includeContainerItems, copies: source.copies,
}, copies: String(source.copies) });

/** Source selection and input only. Rust owns deduplication, counts and commits. */
export function StockpileSourcesDialog({ mode, initialSources = [], task, defaultName = "", onSubmit, onClose }: StockpileSourcesDialogProps) {
  useI18n();
  const [name, setName] = useState(defaultName || task?.name || "");
  const [rows, setRows] = useState(() => initialSources.map(draft));
  const [records, setRecords] = useState<LibraryRecordView[]>([]);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [search, setSearch] = useState("");
  const [reading, setReading] = useState(true);
  const [libraryError, setLibraryError] = useState("");
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const running = useRef(false);
  const mounted = useRef(true);
  const requestId = useRef(crypto.randomUUID());
  const dirty = () => { requestId.current = crypto.randomUUID(); setError(""); };
  const readLibrary = async () => {
    setReading(true); setLibraryError("");
    try { const state = await loadLibrary(); if (mounted.current) setRecords(libraryRecordViews(state)); }
    catch (reason) { if (mounted.current) setLibraryError(String(reason)); }
    finally { if (mounted.current) setReading(false); }
  };
  useEffect(() => { mounted.current = true; void readLibrary(); return () => { mounted.current = false; }; }, []);
  const append = (sources: StockpileSourceInput[]) => { dirty(); setRows(previous => [...previous, ...sources.map(draft)]); };
  const update = (key: string, edit: (row: DraftSource) => DraftSource) => { dirty(); setRows(previous => previous.map(row => row.key === key ? edit(row) : row)); };
  const files = async () => {
    try {
      const chosen = await openDialog({ multiple: true, filters: [{ name: "Minecraft schematics", extensions: ["litematic", "schem", "schematic", "nbt"] }] });
      if (mounted.current && chosen) append((Array.isArray(chosen) ? chosen : [chosen]).map(sourcePath => ({ sourcePath, scope: { kind: "all", value: null }, includeContainerItems: false, copies: 1 })));
    } catch (reason) { if (mounted.current) setError(String(reason)); }
  };
  const submit = async () => {
    if (running.current) return;
    if ((mode === "create" && !name.trim()) || (mode !== "manage" && !rows.length)) { setError(t("stockpile.sources.required")); return; }
    const sources: StockpileSourceInput[] = [];
    for (const row of rows) {
      const copies = /^\d+$/.test(row.copies) ? Number(row.copies) : NaN;
      if (!Number.isSafeInteger(copies) || copies < 1) { setError(t("stockpile.sources.invalidCopies")); return; }
      if (!row.source.sourcePath || (row.source.scope.kind === "region" && !row.source.scope.value.trim())
        || (row.source.scope.kind === "layer" && !/^-?\d+$/.test(row.source.scope.value))) { setError(t("stockpile.invalidScope")); return; }
      sources.push({ ...row.source, copies });
    }
    running.current = true; setPending(true); setError("");
    try { await onSubmit({ name: name.trim(), sources, requestId: requestId.current }); if (mounted.current) onClose(); }
    catch (reason) { if (mounted.current) setError(String(reason)); }
    finally { running.current = false; if (mounted.current) setPending(false); }
  };
  const visible = records.filter(record => `${record.displayName} ${record.fileName} ${record.path}`.toLocaleLowerCase().includes(search.toLocaleLowerCase()));
  return <Dialog title={t(mode === "create" ? "stockpile.sources.create" : mode === "add" ? "stockpile.sources.add" : "stockpile.sources.manage")}
    subtitle={t("stockpile.sources.confirmOnly")} className="stockpile-sources-dialog" width="lg" busy={pending} onClose={onClose}
    footer={<><Button variant="ghost" disabled={pending} onClick={onClose}>{t("editor.cancel")}</Button>
      <Button disabled={pending || (mode !== "manage" && !rows.length)} onClick={() => void submit()}>{pending ? t("editor.saving") : t(mode === "create" ? "stockpile.sources.createConfirm" : "stockpile.sources.saveConfirm")}</Button></>}>
    {mode === "create" ? <label className="stockpile-source-name">{t("stockpile.taskName")}<Input autoFocus value={name} disabled={pending} onChange={event => { dirty(); setName(event.target.value); }} /></label> : <strong>{task?.name}</strong>}
    <div className="stockpile-source-picker-actions"><Button variant="outline" disabled={pending} onClick={() => void files()}><FolderOpen size={16} />{t("stockpile.sources.localFiles")}</Button>
      <Button variant="outline" disabled={pending || !selected.size} onClick={() => { append(records.filter(record => selected.has(record.path)).map(record => ({ sourcePath: record.path, scope: { kind: "all", value: null }, includeContainerItems: false, copies: 1 }))); setSelected(new Set()); }}><Plus size={16} />{t("stockpile.sources.addSelected")}</Button></div>
    <details className="stockpile-source-library" open={!rows.length}>
      <summary>{t("stockpile.sources.library")}</summary>
      <Input value={search} disabled={pending} aria-label={t("stockpile.sources.search")} placeholder={t("stockpile.sources.search")} onChange={event => setSearch(event.target.value)} />
      {reading && <p role="status">{t("editor.loading")}</p>}
      {libraryError && <div role="alert"><span>{libraryError}</span><Button size="sm" onClick={() => void readLibrary()}>{t("stockpile.retry")}</Button></div>}
      <div className="stockpile-source-library-list">{visible.map(record => <label key={record.id} className="stockpile-source-library-row">
        <Checkbox checked={selected.has(record.path)} disabled={pending || record.status !== "ok"} onCheckedChange={checked => setSelected(previous => { const next = new Set(previous); if (checked === true) next.add(record.path); else next.delete(record.path); return next; })} />
        <span><strong>{record.displayName || record.fileName}</strong><span>{record.path}</span>{record.status !== "ok" && <span>{t("stockpile.sources.unavailable")}</span>}</span>
      </label>)}</div>
      {!reading && !libraryError && !visible.length && <p>{t("statistics.noMatches")}</p>}
    </details>
    <div className="stockpile-source-table-scroll"><Table className="stockpile-source-table"><TableHeader><TableRow>
      <TableHead>{t("stockpile.sources.projection")}</TableHead><TableHead>{t("statistics.scope")}</TableHead><TableHead>{t("statistics.copies")}</TableHead><TableHead>{t("stockpile.sources.remove")}</TableHead>
    </TableRow></TableHeader><TableBody>{rows.map(row => <TableRow key={row.key}>
      <TableCell><strong>{row.source.sourcePath.split(/[\\/]/).pop()}</strong><span className="stockpile-source-path">{row.source.sourcePath}</span></TableCell>
      <TableCell><div className="stockpile-source-scope"><Dropdown disabled={pending} value={row.source.scope.kind} ariaLabel={t("statistics.scope")}
        options={[{ value: "all", label: t("statistics.all") }, { value: "region", label: t("stockpile.region") }, { value: "layer", label: t("stockpile.layer") }]}
        onChange={kind => update(row.key, value => ({ ...value, source: { ...value.source, scope: kind === "region" ? { kind, value: "" } : kind === "layer" ? { kind, value: "0" } : { kind: "all", value: null } } }))} />
        {row.source.scope.kind !== "all" && <Input disabled={pending} aria-label={t(row.source.scope.kind === "region" ? "stockpile.region" : "stockpile.layer")} type={row.source.scope.kind === "layer" ? "number" : "text"} step={1} value={row.source.scope.value} onChange={event => update(row.key, value => ({ ...value, source: { ...value.source, scope: { ...value.source.scope, value: event.target.value } as StockpileSourceInput["scope"] } }))} />}
        <label className="beta-material-check"><Checkbox disabled={pending} checked={row.source.includeContainerItems} onCheckedChange={checked => update(row.key, value => ({ ...value, source: { ...value.source, includeContainerItems: checked === true } }))} />{t("statistics.includeContainers")}</label>
      </div></TableCell>
      <TableCell><Input disabled={pending} className="stockpile-source-copies" type="number" min={1} step={1} value={row.copies} aria-label={t("statistics.copies")} onChange={event => update(row.key, value => ({ ...value, copies: event.target.value }))} /></TableCell>
      <TableCell><Button size="icon-sm" variant="ghost" disabled={pending} aria-label={t("stockpile.sources.remove")} onClick={() => { dirty(); setRows(previous => previous.filter(value => value.key !== row.key)); }}><Trash2 size={16} /></Button></TableCell>
    </TableRow>)}</TableBody></Table></div>
    {!rows.length && <p>{t(mode === "manage" ? "stockpile.sources.clearEmpty" : "stockpile.sources.empty")}</p>}
    <p className="muted">{t("stockpile.sources.backendMerge")}</p>
    {error && <p role="alert" className="subwindow-error">{error}</p>}
  </Dialog>;
}
