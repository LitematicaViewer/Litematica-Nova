import React, { useEffect, useState } from "react";
import { Dialog } from "../../components/Dialog";
import { Button } from "../../components/ui/button";
import { saveDialog } from "../../../src/platform/dialogs";
import { querySchematicFormat, type FormatResult, type SchematicFormat } from "../../../src/platform/schematicFormats";
import { useI18n } from "../../i18n";

export function FormatExportDialog({ sourcePath, onClose, onWorldExport }: { sourcePath: string; onClose: () => void; onWorldExport: () => void }) {
  const { locale } = useI18n();
  const en = locale === "en-US";
  const [view, setView] = useState<FormatResult>();
  const [format, setFormat] = useState<SchematicFormat>("litematic");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState("");
  useEffect(() => {
    let active = true;
    void querySchematicFormat({ action: "inspect", input: sourcePath }).then(result => { if (active) setView(result); }).catch(reason => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, [sourcePath]);
  const capability = view?.capabilities?.find(item => item.format === format);
  const exportFile = async () => {
    const extension = format === "litematic" ? "litematic" : format === "buildingGadgetsLegacy" || format === "buildingGadgets2" ? "json" : "nbt";
    const output = await saveDialog({ filters: [{ name: format, extensions: [extension] }] });
    if (!output) return;
    setBusy(true); setError(""); setSaved("");
    try { await querySchematicFormat({ action: "export", input: sourcePath, output, format }); setSaved(output); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };
  const copyTemplate = async () => {
    setBusy(true); setError(""); setSaved("");
    try {
      const result = await querySchematicFormat({ action: "encodeText", input: sourcePath, format });
      if (!result.clipboardText) throw new Error(en ? "No template text was returned." : "没有返回模板共享文本。");
      await navigator.clipboard.writeText(result.clipboardText);
      setSaved(en ? "Template copied." : "模板共享文本已复制。");
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };
  return <Dialog title={en ? "Export schematic" : "导出投影"} busy={busy} onClose={onClose} footer={<>
    <Button variant="ghost" disabled={busy} onClick={onWorldExport}>{en ? "Export world…" : "导出世界…"}</Button>
    {(format === "buildingGadgetsLegacy" || format === "buildingGadgets2") && <Button disabled={busy || !capability?.canExport} onClick={() => void copyTemplate()}>{en ? "Copy template" : "复制模板共享文本"}</Button>}
    <Button disabled={busy || !capability?.canExport} onClick={() => void exportFile()}>{en ? "Save as…" : "另存为…"}</Button>
  </>}>
    <label>{en ? "Target format" : "目标格式"}<select className="input" value={format} onChange={event => setFormat(event.target.value as SchematicFormat)}>
      {(view?.capabilities ?? []).map(item => <option key={item.format} value={item.format} disabled={!item.canExport}>{item.format}{item.canExport ? "" : (en ? " (unavailable)" : "（不支持）")}</option>)}
    </select></label>
    {capability?.reason && <p role="status">{capability.reason}</p>}
    <p>{en ? "Format limitations are checked by the backend. Non-equivalent data is never silently discarded." : "格式限制由后端核对，不会静默丢弃无法等价转换的数据。"}</p>
    {error && <p role="alert">{error}</p>}{saved && <p role="status">{saved}</p>}
  </Dialog>;
}
