import { useEffect, useState } from "react";
import { Dialog } from "./Dialog";
import { Button } from "./ui/button";
import { useI18n } from "../i18n";
import { t } from "../../src/services/i18n";
import { discardGeneratedPreview, saveGeneratedPreview, type ProductPreviewImage } from "../../src/platform/previewImage";
import "../styles/preview-draft.css";

export interface PreviewDraftDialogProps {
  draft: ProductPreviewImage | null;
  sourcePath: string;
  onSaved: (image: ProductPreviewImage) => void;
  onClose: () => void;
}
/** Generation is derived-only. This is the shared, explicit source-write boundary. */
export function PreviewDraftDialog({ draft, sourcePath, onSaved, onClose }: PreviewDraftDialogProps) {
  useI18n();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => { setBusy(false); setError(""); }, [draft?.draft_id]);
  if (!draft) return null;
  const discard = async () => {
    if (busy) return;
    setBusy(true); setError("");
    try { if (draft.draft_id) await discardGeneratedPreview(draft.draft_id); onClose(); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };
  const save = async () => {
    if (!draft.draft_id || !draft.can_embed || busy) return;
    setBusy(true); setError("");
    try {
      const saved = await saveGeneratedPreview(sourcePath, draft.draft_id);
      if (!saved.persisted) throw new Error(t("library.previewSaveUnconfirmed"));
      onSaved(saved); onClose();
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };
  return <Dialog title={t("library.previewDraftTitle")} subtitle={sourcePath} width="md" onClose={() => { void discard(); }}
    footer={<><Button disabled={busy} onClick={() => { void discard(); }}>{t("library.previewDiscard")}</Button><Button variant="default" disabled={busy || !draft.can_embed || !draft.draft_id} onClick={() => { void save(); }}>{busy ? t("library.previewSaving") : t("library.previewSaveEmbedded")}</Button></>}>
    <img className="preview-draft-image" src={draft.data_url} alt={t("library.previewDraftAlt")} />
    <p>{t("library.previewDraftDescription")}</p>
    {draft.display_mode && <p className="nova-muted">{t("settings.previewGeneration")} · {draft.display_mode === "fast_experimental" ? t("render.mode.fast") : draft.display_mode === "standard" ? t("render.mode.standard") : t("native.mode.full")}</p>}
    {!draft.can_embed && <p role="status">{t("library.previewFormatUnsupported")}</p>}
    {error && <p className="nova-error" role="alert">{error}</p>}
  </Dialog>;
}
