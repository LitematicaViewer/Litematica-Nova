import { t } from "../../../../../src/services/i18n";
import { useEffect, useRef, useState } from "react";
import { FolderOpen } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { Dialog } from "../../../../components/Dialog";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { displayFilePath, saveDialog } from "../../../../../src/business/facade";

/** Export selection produced by the dialog and executed by the container (E.6). */
export interface ExportOptions {
  includePreview: boolean;
  includeMaterialList: boolean;
  includeManifest: boolean;
  outputPath: string;
}

function defaultExportName(): string {
  const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
  return `nova_export_${stamp}.zip`;
}

/**
 * Packaged-export dialog (E.6, 5.9.1.5 / 5.9.1.7 / 裁决 16).
 *
 * The projection body is always included and cannot be unchecked, so the "empty
 * set" submit is impossible; preview / material list / manifest are free. The user
 * picks the destination `.zip` path. The material list is exported as `txt` by
 * default (裁决 4).
 */
export function ExportDialog({
  entryCount,
  busy,
  error,
  onClose,
  onSubmit,
}: {
  entryCount: number;
  busy: boolean;
  error: string;
  onClose: () => void;
  onSubmit: (options: ExportOptions) => void;
}) {
  const [includePreview, setIncludePreview] = useState(true);
  const [includeMaterialList, setIncludeMaterialList] = useState(true);
  const [includeManifest, setIncludeManifest] = useState(true);
  const [outputPath, setOutputPath] = useState("");
  const [pickError, setPickError] = useState("");
  const [picking, setPicking] = useState(false);
  const mounted = useRef(true);
  const pickPending = useRef(false);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);

  const handlePickPath = async () => {
    if (pickPending.current) return;
    pickPending.current = true; setPicking(true); setPickError("");
    try {
      const picked = await saveDialog({ defaultPath: defaultExportName(), filters: [{ name: "ZIP", extensions: ["zip"] }] });
      if (mounted.current && picked) setOutputPath(picked);
    } catch (reason) { if (mounted.current) setPickError(t("library.ux.pickExportFailed", { value0: String(reason) })); }
    finally { pickPending.current = false; if (mounted.current) setPicking(false); }
  };

  const canSubmit = !busy && !picking && !!outputPath.trim() && entryCount > 0;

  return (
    <Dialog
      title={t("library.ux.exportZip")}
      subtitle={t("library.ux.zipSummary", { value0: entryCount })}
      width="md"
      busy={busy}
      onClose={onClose}
      footer={(
        <>
          <Button type="button" onClick={onClose} disabled={busy}>{t("common.cancel")}</Button>
          <Button type="button" variant="default" disabled={!canSubmit} aria-busy={busy} onClick={() => onSubmit({ includePreview, includeMaterialList, includeManifest, outputPath: outputPath.trim() })}>
            {busy ? t("statistics.exporting") : t("library.ux.startExport")}
          </Button>
        </>
      )}
    >
      <div className="library-export-dialog">
        <fieldset className="library-export-content">
          <legend>{t("stockpile.exportContent")}</legend>
          <label className="setting-row library-export-option">
            <Checkbox checked disabled aria-label={t("library.ux.bodyRequired")} /> {t("library.ux.bodyRequired")}
          </label>
          <label className="setting-row library-export-option">
            <Checkbox checked={includePreview} disabled={busy} aria-label={t("library.preview")} onCheckedChange={(value) => setIncludePreview(value === true)} /> {t("library.preview")}
          </label>
          <label className="setting-row library-export-option">
            <Checkbox checked={includeMaterialList} disabled={busy} aria-label={t("stockpile.materialCatalog")} onCheckedChange={(value) => setIncludeMaterialList(value === true)} /> {t("library.ux.materialTxt")}
          </label>
          <label className="setting-row library-export-option">
            <Checkbox checked={includeManifest} disabled={busy} aria-label="manifest.json" onCheckedChange={(value) => setIncludeManifest(value === true)} /> manifest.json
          </label>
        </fieldset>
        <div className="library-export-target">
          <span className="dialog-label">{t("library.ux.exportTarget")}</span>
          <div className="library-export-target-row">
            <Button type="button" disabled={busy || picking} aria-busy={picking} onClick={() => void handlePickPath()}>
              <ThemeIcon name="explorer" size={16} fallback={FolderOpen} />{t("library.ux.chooseTarget")}
            </Button>
            <span className="library-export-path" title={outputPath ? displayFilePath(outputPath) : ""}>{outputPath ? displayFilePath(outputPath) : t("library.ux.noTarget")}</span>
          </div>
        </div>
        <p className="muted library-export-note">{t("library.ux.flatZip")}</p>
      </div>
      {(error || pickError) && <div className="dialog-message dialog-message-error" role="alert">{error || pickError}</div>}
    </Dialog>
  );
}
