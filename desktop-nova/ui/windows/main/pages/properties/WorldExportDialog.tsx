import React, { useEffect, useRef, useState } from "react";
import { t, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { openDialog } from "../../../../../src/platform/dialogs";
import { openPath } from "../../../../../src/platform/nativeViewer";
import { exportMinecraftWorld, worldExportOptions, worldExportStatus, type WorldExportOptions, type WorldExportProgress, type WorldExportResult } from "../../../../../src/services/worldExport";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../../../components/ui/select";

function directoryName(value: string): string {
  const name = value.trim().replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").replace(/[. ]+$/, "");
  return /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(name) ? `_${name}` : name;
}
export function WorldExportDialog({ sourcePath, name, onClose }: {
  sourcePath: string; name: string; onClose: () => void;
}) {
  useI18n();
  const [worldName, setWorldName] = useState(name || t("worldExport.defaultName"));
  const [parentFolder, setParentFolder] = useState("");
  const [origin, setOrigin] = useState({ x: 0, y: 0, z: 0 });
  const [options, setOptions] = useState<WorldExportOptions | null>(null);
  const [target, setTarget] = useState("");
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [progress, setProgress] = useState<WorldExportProgress | null>(null);
  const [error, setError] = useState<UiMessage>("");
  const [result, setResult] = useState<WorldExportResult | null>(null);
  const task = useRef<string | null>(null);
  const cancelRequested = useRef(false);
  useEffect(() => {
    let current = true;
    void worldExportOptions(sourcePath).then(value => { if (current) { setOptions(value); setTarget(value.defaultVersion); } })
      .catch(reason => { if (current) setError({ key: "worldExport.failed", params: { error: String(reason) } }); });
    return () => { current = false; if (task.current) void worldExportStatus(task.current, true).catch(() => {}); };
  }, [sourcePath]);
  useEffect(() => {
    if (!busy) return;
    let pending = false;
    const timer = window.setInterval(() => {
      const id = task.current;
      if (!id || pending) return;
      pending = true;
      void worldExportStatus(id).then(value => { if (task.current === id && value.progress) setProgress(value.progress); })
        .finally(() => { pending = false; }).catch(() => {});
    }, 300);
    return () => window.clearInterval(timer);
  }, [busy]);
  const folderName = directoryName(worldName);
  const separator = parentFolder.includes("\\") ? "\\" : "/";
  const outputDirectory = parentFolder && folderName ? `${parentFolder.replace(/[\\/]+$/, "")}${separator}${folderName}` : "";
  const validOrigin = Object.values(origin).every(value => Number.isInteger(value) && value >= -2147483648 && value <= 2147483647);
  const profile = options?.profiles.find(value => value.version === target);
  async function chooseFolder() {
    try {
      const selected = await openDialog({ title: t("worldExport.chooseTitle"), directory: true, multiple: false });
      if (typeof selected === "string") { setParentFolder(selected); setError(""); setResult(null); }
    } catch (reason) { setError({ key: "worldExport.failed", params: { error: String(reason) } }); }
  }
  async function cancelExport() {
    if (!task.current || cancelling) return;
    setCancelling(true); cancelRequested.current = true;
    try { await worldExportStatus(task.current, true); }
    catch (reason) { setCancelling(false); cancelRequested.current = false; setError({ key: "worldExport.failed", params: { error: String(reason) } }); }
  }
  async function exportWorld() {
    if (busy || !outputDirectory || !validOrigin || !profile) return;
    const taskId = crypto.randomUUID(); task.current = taskId; cancelRequested.current = false;
    setBusy(true); setCancelling(false); setError(""); setResult(null); setProgress(null);
    try { setResult(await exportMinecraftWorld({ taskId, sourcePath, outputDirectory, worldName: worldName.trim(), targetVersion: target, origin })); }
    catch (reason) { setError({ key: cancelRequested.current ? "worldExport.cancelled" : "worldExport.failed", params: { error: String(reason) } }); }
    finally { task.current = null; setBusy(false); setCancelling(false); }
  }
  return <Dialog title={t("worldExport.title")} busy={busy} onClose={onClose} footer={<>
    {busy ? <Button variant="ghost" disabled={cancelling} onClick={() => void cancelExport()}>{t(cancelling ? "worldExport.cancelling" : "worldExport.cancel")}</Button>
      : <Button variant="ghost" onClick={onClose}>{t("library.closeDialog")}</Button>}
    {!result && <Button disabled={busy || !profile || !outputDirectory || !validOrigin} onClick={() => void exportWorld()}>{busy ? t("worldExport.busy") : t("worldExport.export")}</Button>}
    {result && <Button onClick={() => void openPath(result.outputPath).catch(reason => setError({ key: "worldExport.failed", params: { error: String(reason) } }))}>{t("worldExport.openFolder")}</Button>}
  </>}>
    <label className="properties-field"><span>{t("worldExport.name")}</span><Input value={worldName} disabled={busy} onChange={event => { setWorldName(event.target.value); setResult(null); }} /></label>
    <div className="properties-field"><span id="world-export-version-label">{t("worldExport.version")}</span><Select value={target} disabled={busy || !options} onValueChange={value => { setTarget(value); setResult(null); }}>
      <SelectTrigger aria-labelledby="world-export-version-label"><SelectValue placeholder={t("worldExport.loadingVersions")} /></SelectTrigger>
      <SelectContent>{options?.profiles.map(value => <SelectItem key={value.version} value={value.version}>Minecraft Java {value.version}</SelectItem>)}</SelectContent>
    </Select></div>
    <label className="properties-field"><span>{t("worldExport.folder")}</span><Input value={parentFolder} readOnly /><Button disabled={busy} variant="ghost" onClick={() => void chooseFolder()}>{t("worldExport.chooseFolder")}</Button></label>
    {outputDirectory && <p className="properties-world-export-path"><span>{t("worldExport.output")}: </span>{outputDirectory}</p>}
    <p className="muted">{t(profile?.generator.startsWith("flat_") ? "worldExport.legacyScope" : "worldExport.scope")}</p>
    <details><summary>{t("worldExport.advanced")}</summary>
      {profile && <p>{t("worldExport.height", { min: profile.min_y, max: profile.min_y + profile.height - 1 })}</p>}
      <fieldset className="properties-world-export-origin" disabled={busy}><legend>{t("worldExport.origin")}</legend>
        {(["x", "y", "z"] as const).map(axis => <label className="properties-field" key={axis}><span>{axis.toUpperCase()}</span><Input type="number" step="1" value={Number.isNaN(origin[axis]) ? "" : origin[axis]} onChange={event => { setOrigin(current => ({ ...current, [axis]: event.target.value === "" ? NaN : Number(event.target.value) })); setResult(null); }} /></label>)}
      </fieldset>
    </details>
    {busy && <p role="status">{t(`worldExport.stage.${progress?.stage || "reading"}`)}{progress && progress.total > 0 ? ` ${progress.completed}/${progress.total}` : ""}</p>}
    {error && <p role="alert" className="properties-error nova-text-danger">{localizeMessage(error)}</p>}
    {result && <p role="status">{t("worldExport.success", { path: result.outputPath, version: result.gameVersion })}</p>}
  </Dialog>;
}
