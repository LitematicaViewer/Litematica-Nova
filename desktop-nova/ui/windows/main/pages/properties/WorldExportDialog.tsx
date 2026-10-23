import React, { useState } from "react";
import { t, localizeMessage, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { openDialog } from "../../../../../src/platform/dialogs";
import { exportMinecraftWorld, type WorldExportResult } from "../../../../../src/services/worldExport";
import { Dialog } from "../../../../components/Dialog";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";

function directoryName(value: string): string {
  return value.trim().replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").replace(/[. ]+$/, "");
}
export function WorldExportDialog({ sourcePath, name, onClose }: {
  sourcePath: string; name: string; onClose: () => void;
}) {
  useI18n();
  const [worldName, setWorldName] = useState(name || t("worldExport.defaultName"));
  const [parentFolder, setParentFolder] = useState("");
  const [origin, setOrigin] = useState({ x: 0, y: 0, z: 0 });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiMessage>("");
  const [result, setResult] = useState<WorldExportResult | null>(null);
  const folderName = directoryName(worldName);
  const separator = parentFolder.includes("\\") ? "\\" : "/";
  const outputDirectory = parentFolder && folderName ? `${parentFolder.replace(/[\\/]+$/, "")}${separator}${folderName}` : "";
  const validOrigin = Object.values(origin).every(value => Number.isInteger(value) && value >= -2147483648 && value <= 2147483647);
  async function chooseFolder() {
    try {
      const selected = await openDialog({ title: t("worldExport.chooseTitle"), directory: true, multiple: false });
      if (typeof selected === "string") { setParentFolder(selected); setError(""); setResult(null); }
    } catch (reason) { setError({ key: "worldExport.failed", params: { error: String(reason) } }); }
  }
  async function exportWorld() {
    if (busy || !outputDirectory || !validOrigin) return;
    setBusy(true); setError(""); setResult(null);
    try { setResult(await exportMinecraftWorld({ sourcePath, outputDirectory, worldName: worldName.trim(), origin })); }
    catch (reason) { setError({ key: "worldExport.failed", params: { error: String(reason) } }); }
    finally { setBusy(false); }
  }
  return <Dialog title={t("worldExport.title")} busy={busy} onClose={onClose} footer={<>
    <Button variant="ghost" disabled={busy} onClick={onClose}>{t("library.closeDialog")}</Button>
    <Button disabled={busy || !outputDirectory || !validOrigin || !!result} onClick={() => void exportWorld()}>{busy ? t("worldExport.busy") : t("worldExport.export")}</Button>
  </>}>
    <p>{t("worldExport.target")}</p>
    <label className="properties-field"><span>{t("worldExport.name")}</span><Input value={worldName} disabled={busy} onChange={event => { setWorldName(event.target.value); setResult(null); }} /></label>
    <label className="properties-field"><span>{t("worldExport.folder")}</span><Input value={parentFolder} readOnly /><Button disabled={busy} variant="ghost" onClick={() => void chooseFolder()}>{t("worldExport.chooseFolder")}</Button></label>
    {outputDirectory && <p className="properties-world-export-path"><span>{t("worldExport.output")}: </span>{outputDirectory}</p>}
    <fieldset className="properties-world-export-origin" disabled={busy}><legend>{t("worldExport.origin")}</legend>
      {(["x", "y", "z"] as const).map(axis => <label className="properties-field" key={axis}><span>{axis.toUpperCase()}</span><Input type="number" step="1" value={Number.isNaN(origin[axis]) ? "" : origin[axis]} onChange={event => { setOrigin(current => ({ ...current, [axis]: event.target.value === "" ? NaN : Number(event.target.value) })); setResult(null); }} /></label>)}
    </fieldset>
    {error && <p role="alert" className="properties-error nova-text-danger">{localizeMessage(error)}</p>}
    {result && <p role="status">{t("worldExport.success", { path: result.outputPath })}</p>}
  </Dialog>;
}
