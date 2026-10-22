import { t, formatDate, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
import React, { useEffect, useRef, useState } from "react";
import { Button } from "../../components/ui/button";
import { Dialog } from "../../components/Dialog";
import { NavIcon } from "../../shell/NavIcon";
import { cancelUpdate, checkForUpdate, downloadUpdate, installUpdate, onUpdateState, readUpdateState, restartAfterUpdateFailure, skipUpdate, type UpdateView } from "../../../src/platform/updater";
import { clearCurrentUpdateDrafts, preserveUpdateDrafts, releaseNoteSections } from "./updateRecovery";
import "../../styles/updater.css";

let startupCheck: Promise<UpdateView> | undefined;

export function UpdateDialog({ theme }: { theme: string }) {
  useI18n();
  const [view, setView] = useState<UpdateView | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<UiMessage>("");
  const action = useRef(false);
  const [running, setRunning] = useState(false);
  useEffect(() => {
    let live = true;
    const accept = (next: UpdateView) => {
      if (!live) return;
      setView(next);
      if (next.phase === "available") setOpen(true);
    };
    const listening = onUpdateState(accept);
    // Subscribe first: the automatic check is asynchronous and shared across StrictMode mounts.
    void listening.then(async () => {
      accept(await readUpdateState());
      startupCheck ??= checkForUpdate(false);
      accept(await startupCheck);
    }).catch(() => undefined);
    return () => { live = false; void listening.then((stop) => stop()).catch(() => undefined); };
  }, []);

  const manualCheck = async () => {
    if (action.current) return;
    action.current = true; setRunning(true); setError(""); setOpen(true);
    try { setView(await checkForUpdate(true)); } catch (e) { setError(String(e)); }
    finally { action.current = false; setRunning(false); }
  };
  const apply = async () => {
    if (action.current) return;
    action.current = true; setRunning(true); setError("");
    try {
      preserveUpdateDrafts();
      const current = await readUpdateState();
      const next = current.phase === "downloaded" ? current : await downloadUpdate(); setView(next);
      if (next.phase !== "downloaded") return;
      preserveUpdateDrafts();
      await installUpdate();
    } catch (e) { setError(String(e)); }
    finally {
      // A cancelled/failed update must not later resurrect old edits during normal Save.
      try { if ((await readUpdateState()).phase !== "restartRequired") clearCurrentUpdateDrafts(); } catch (e) { setError(String(e)); }
      action.current = false; setRunning(false);
    }
  };
  const cancel = async () => {
    if (view?.phase === "installing" || view?.phase === "restartRequired") return;
    try { setView(await cancelUpdate()); setOpen(false); } catch (e) { setError(String(e)); }
  };
  const skip = async () => {
    try { setView(await skipUpdate()); setOpen(false); } catch (e) { setError(String(e)); }
  };
  const installing = view?.phase === "installing" || view?.phase === "restartRequired";
  const canUpdate = !!view?.version && !running && ["available", "error", "cancelled", "downloaded"].includes(view.phase);
  const phaseText = { idle: t("update.notChecked"), disabled: t("update.notConfigured"), checking: t("update.checking"), available: t("update.available"), current: t("update.current"), skipped: t("update.skipped"), downloading: t("update.downloading"), downloaded: t("update.verified"), waiting: t("update.waiting"), installing: t("update.installing"), cancelled: t("update.cancelled"), error: t("update.incomplete"), restartRequired: t("update.recoveryRequired") };
  return <>
    <Button variant="ghost" className="sidebar-item nav-item updater-check" onClick={manualCheck} disabled={running} title={t("update.check")} aria-label={t("update.check")}>
      <NavIcon name="update" theme={theme} /><span className="nav-label">{t("update.check")}</span>
    </Button>
    {open && <Dialog title={view?.version ? t("update.toVersion", { version: view.version }) : t("update.title")} subtitle={t("update.restartHint")} onClose={() => void cancel()} busy={installing} width="md"
      footer={<>
        <Button variant="ghost" onClick={() => void skip()} disabled={!view?.version || running || installing}>{t("update.skip")}</Button>
        <Button variant="secondary" onClick={() => void cancel()} disabled={installing}>{t("common.cancel")}</Button>
        {view?.phase === "restartRequired"
          ? <Button onClick={() => void restartAfterUpdateFailure().catch((e) => setError(String(e)))}>{t("update.restartCurrent")}</Button>
          : <Button onClick={() => void apply()} disabled={!canUpdate}>{t("update.installRestart")}</Button>}
      </>}>
      <p role="status" aria-live="polite">{view ? phaseText[view.phase] : t("update.checking")}</p>
      <dl className="update-version"><dt>{t("update.currentVersion")}</dt><dd>{view?.currentVersion || "—"}</dd><dt>{t("update.version")}</dt><dd>{view?.version || "—"}</dd><dt>{t("update.date")}</dt><dd>{view?.date ? formatDate(view.date, {dateStyle:"medium"}) : t("common.notProvided")}</dd></dl>
      {view?.source && <p className="muted">{t("update.source")}{view.source}</p>}
      {view?.phase === "downloading" && <div className="update-download"><progress aria-label={t("update.downloadProgress")} max={view.total || undefined} value={view.total ? Math.min(view.downloaded, view.total) : undefined} /><span>{(view.downloaded / 1048576).toFixed(1)} MB{view.total ? ` / ${(view.total / 1048576).toFixed(1)} MB` : ""}</span></div>}
      {view?.version && releaseNoteSections(view.notes).map(({ label, lines }) => <section className="update-notes" key={label}><h4>{localizeMessage(label)}</h4><ul>{lines.map((line, i) => <li key={i}>{line}</li>)}</ul></section>)}
      {(error || view?.message) && <p role="alert">{localizeMessage(error || view?.message)}</p>}
    </Dialog>}
  </>;
}
