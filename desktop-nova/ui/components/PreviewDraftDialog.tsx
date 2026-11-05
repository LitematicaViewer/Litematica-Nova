import { useLayoutEffect, useRef, useState } from "react";
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
  const ownership = useRef<{ id: string; alive: boolean; working: boolean; settled: boolean } | null>(null);
  // An accepted draft belongs to this dialog. Wait for an in-flight save before
  // releasing it: Rust may still be reading its frame for the source transaction.
  const release = (owner: NonNullable<typeof ownership.current>) => {
    if (owner.working || owner.settled) return;
    owner.settled = true;
    queueMicrotask(() => {
      // React's effect replay may immediately reacquire this same draft.
      if (ownership.current !== owner && ownership.current?.alive && ownership.current.id === owner.id) return;
      void discardGeneratedPreview(owner.id).catch(reason => console.warn("[NOVA_PREVIEW] discard departed draft failed", reason));
    });
  };
  useLayoutEffect(() => {
    setBusy(false); setError("");
    const owner = draft?.draft_id ? { id: draft.draft_id, alive: true, working: false, settled: false } : null;
    ownership.current = owner;
    return () => { if (owner) { owner.alive = false; release(owner); } };
  }, [draft?.draft_id]);
  if (!draft) return null;
  const discard = async () => {
    const owner = ownership.current;
    if (owner?.working || owner?.settled || busy) return;
    if (owner) owner.working = true;
    setBusy(true); setError("");
    try {
      if (owner) { await discardGeneratedPreview(owner.id); owner.settled = true; }
      if (!owner || owner.alive) onClose();
    } catch (reason) { if (!owner || owner.alive) setError(String(reason)); }
    finally { if (owner) owner.working = false; if (!owner || owner.alive) setBusy(false); else release(owner); }
  };
  const save = async () => {
    const owner = ownership.current;
    if (!owner || !draft.can_embed || owner.working || owner.settled) return;
    owner.working = true;
    setBusy(true); setError("");
    try {
      const saved = await saveGeneratedPreview(sourcePath, owner.id);
      if (!saved.persisted) throw new Error(t("library.previewSaveUnconfirmed"));
      owner.settled = true;
      if (owner.alive) { onSaved(saved); onClose(); }
    } catch (reason) { if (owner.alive) setError(String(reason)); }
    finally { owner.working = false; if (owner.alive) setBusy(false); else release(owner); }
  };
  return <Dialog title={t("library.previewDraftTitle")} subtitle={sourcePath} width="md" busy={busy} onClose={() => { void discard(); }}
    footer={<><Button disabled={busy} onClick={() => { void discard(); }}>{t("library.previewDiscard")}</Button><Button variant="default" disabled={busy || !draft.can_embed || !draft.draft_id} onClick={() => { void save(); }}>{busy ? t("library.previewSaving") : t("library.previewSaveEmbedded")}</Button></>}>
    <img className="preview-draft-image" src={draft.data_url} alt={t("library.previewDraftAlt")} />
    <p>{t("library.previewDraftDescription")}</p>
    {draft.display_mode && <p className="nova-muted">{t("settings.previewGeneration")} · {draft.display_mode === "fast_experimental" ? t("render.mode.fast") : draft.display_mode === "standard" ? t("render.mode.standard") : t("native.mode.full")}</p>}
    {!draft.can_embed && <p role="status">{t("library.previewFormatUnsupported")}</p>}
    {error && <p className="nova-error" role="alert">{error}</p>}
  </Dialog>;
}
