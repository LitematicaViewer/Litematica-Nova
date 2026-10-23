import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { displayPathText } from "../../../../../src/business/facade";
import React, { useEffect, useRef, useState } from "react";
import { Box, EyeOff, Play, X, Image } from "lucide-react";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { loadLibrary, setRecordPreview, renderPreviewImage, readProjectionPreviewImage } from "../../../../../src/business/facade";
import { emitEvent } from "../../../../../src/platform/events";
import { projectionLibraryStateChangedEvent } from "../../../libraryEvents";
import { Button } from "../../../../components/ui/button";
import { Checkbox } from "../../../../components/ui/checkbox";
import { useQuickPreview } from "../../useQuickPreview";
import { useFullPreview } from "../../useFullPreview";
import { formatResourceBytes } from "../../../../../src/business/resourcePreflightUnits";
import { displayFilePath } from "../../../../../src/business/facade";
import { FreeCameraControls } from "./FreeCameraControls";
import "./render.css";

/** Separate historical Fast and real private Full service paths; no algorithm substitution. */
export function RenderPage({ currentFile, presentationBlocked = false }: { currentFile: string; presentationBlocked?: boolean }) {
  useI18n();
  const [mode, setMode] = useState<"fast" | "standard" | "full">("fast");
  const [preloads, setPreloads] = useState({ full: false, standard: false, fast: false });
  const exactMode = mode === "standard" ? "standard" : "full";
  const preload = preloads[mode];
  const preview = useQuickPreview(currentFile || "", mode !== "fast" || presentationBlocked, preload);
  const full = useFullPreview(currentFile || "", preload, mode === "fast" || presentationBlocked, exactMode);
  const [capturing, setCapturing] = useState(false);
  const [captureMessage, setCaptureMessage] = useState<UiMessage>("");
  const captureInFlight = useRef(false);
  const captureSource = useRef(currentFile); captureSource.current = currentFile;
  useEffect(() => { setCaptureMessage(""); }, [currentFile]);
  const capture = async () => {
    if (captureInFlight.current) return;
    const source = currentFile; captureInFlight.current = true;
    setCapturing(true); setCaptureMessage("");
    try {
      const image = await renderPreviewImage(source, mode === "fast" ? "fast_experimental" : mode, undefined,
        mode === "fast" ? preview.operation?.previewId : full.operation?.operationId);
      const embedded = await readProjectionPreviewImage(source);
      if (!embedded?.data_url) throw new Error(t("render.previewReadbackFailed"));
      await setRecordPreview(await loadLibrary(), source, image.preview_path);
      await emitEvent(projectionLibraryStateChangedEvent, { reason: "embedded-preview", path: source }).catch(() => undefined);
      if (captureSource.current === source) setCaptureMessage(message("render.thumbnailSaved"));
    } catch (error) { if (captureSource.current === source) setCaptureMessage(String(error)); }
    finally { captureInFlight.current = false; setCapturing(false); }
  };
  const fullProgress = full.progress;
  const operation = preview.operation;
  const ready = operation?.state === "ready";
  const loading = operation?.state === "starting" && !operation.error;
  const error = preview.error || operation?.error;
  const status = !currentFile ? t("render.openFirst")
    : loading ? t("render.preparingFast")
      : ready ? t("render.controlsHint")
        : operation?.state === "failed" ? t("render.fastLoadingFailed")
          : operation?.state === "closed" ? t("render.fastClosed") : t("render.notLoaded");
  const name = currentFile?.split(/[\\/]/).pop() || t("render.noSchematic");
  return <section className="render-page">
    <header className="render-page-header">
      <h2 className="render-page-title">{mode === "full" ? t("render.title.full") : mode === "standard" ? t("render.title.standard") : t("render.title.fast")}</h2>
      <div className="render-page-source" title={displayFilePath(currentFile)}>{name}</div>
    </header>
    <div className="render-page-actions" aria-label={t("render.modeLabel")}>
      <Button variant={mode === "fast" ? "default" : "ghost"} aria-pressed={mode === "fast"} onClick={() => setMode("fast")}>{t("render.mode.fast")}</Button>
      <Button variant={mode === "standard" ? "default" : "ghost"} aria-pressed={mode === "standard"} onClick={() => setMode("standard")}>{t("render.mode.standard")}</Button>
      <Button variant={mode === "full" ? "default" : "ghost"} aria-pressed={mode === "full"} onClick={() => setMode("full")}>{t("render.mode.full")}</Button>
    </div>
    <FreeCameraControls sourcePath={currentFile} displayMode={mode === "fast" ? "fast_experimental" : mode}
      ready={mode === "fast" ? ready && !!operation?.presentation.visible : full.ready && full.visible} />
    {mode !== "fast" ? <>
      <div className="render-page-actions">
        <label><Checkbox checked={preload} disabled={full.pending || full.loading} onCheckedChange={checked => setPreloads(previous => ({ ...previous, [exactMode]: checked === true }))} />{t("render.preload")}</label>
        <Button disabled={!currentFile || full.pending || full.loading} onClick={() => void full.start()}><ThemeIcon name={mode === "full" ? "build" : "run"} size={16} fallback={mode === "full" ? Box : Play} />{t(mode === "standard" ? "render.title.standard" : "render.title.full")}</Button>
        <Button variant="ghost" disabled={!full.operation || full.pending} onClick={() => void full.hide()}><ThemeIcon name="hide" size={16} fallback={EyeOff} />{t("render.hide")}</Button>
        <Button variant="ghost" disabled={!full.operation && !full.pending && !full.preflight} onClick={() => void full.close()}><ThemeIcon name="close" size={16} fallback={X} />{t("render.closeCancel")}</Button>
        <Button variant="ghost" disabled={!full.ready || full.pending || capturing} onClick={() => void capture()}><ThemeIcon name="picture_trans" size={16} fallback={Image} />{capturing ? t("render.saving") : t("render.setThumbnail")}</Button>
      </div>
      {full.preflight && <section className="render-preview-help" role="status">
        <p>{t("render.memoryRequired")}{full.preflight.conservativeVramBytes == null ? t("common.unknown") : formatResourceBytes(full.preflight.conservativeVramBytes)}</p>
        {full.preflight.severity === "warning" && <p>{t("render.pressure80")}</p>}
        {full.preflight.severity === "hard" && <p>{t("render.pressure95")}</p>}
        {full.preflight.severity === "unmeasured" && <p role="alert">{full.preflight.invalidReason ? displayPathText(localizeMessage(full.preflight.invalidReason)) : t("render.pressureUnknown")}</p>}
        <div className="render-page-actions">
          <Button disabled={full.pending || !full.preflight.allowedActions.includes("continue_prebuilt")} onClick={() => void full.choose("prebuilt")}>{t("render.confirmPrebuild")}</Button>
          <Button disabled={full.pending || (full.preflight.severity !== "normal" && !full.preflight.allowedActions.includes("switch_realtime"))} onClick={() => void full.choose("realtime")}>{t("render.openRealtime")}</Button>
          <Button variant="ghost" disabled={full.pending || !full.preflight.allowedActions.includes("use_basic_vulkan")} onClick={() => void full.choose("basic_vulkan")}>{t("render.reduceQuality")}</Button>
          <Button variant="ghost" onClick={() => void full.close()}>{t("common.cancel")}</Button>
        </div>
      </section>}
      {full.error && <pre className="render-error" role="alert">{displayPathText(localizeMessage(full.error))}</pre>}
      {fullProgress && !full.visible && !full.error && <div className="render-preview-help" role="status" aria-live="polite">
        {full.progressLabel}
        <progress aria-label={t("render.progress")} max={full.progressRange?.total} value={full.progressRange?.completed} />
      </div>}
      <div className="render-preview-group"><div ref={full.hostRef} className="render-preview-host" aria-label={t("render.viewport", { mode: mode === "standard" ? t("render.standard") : t("render.full") })}>
        {!full.visible && <div className="render-preview-placeholder"><ThemeIcon name="empty" size={40} fallback={Box} /><strong>{name}</strong><span>{full.error ? t("render.loadingFailed") : full.progressLabel}</span></div>}
      </div></div>
    </> : <>
    <div className="render-page-actions">
      <label><Checkbox checked={preload} disabled={preview.pending || loading} onCheckedChange={(checked) => setPreloads((current) => ({ ...current, fast: checked === true }))} />{t("render.preload")}</label>
      <Button variant="default" disabled={!currentFile || preview.pending || loading} onClick={() => void preview.start()}>
        <ThemeIcon name="run" size={16} fallback={Play} />{preview.pending ? t("render.connecting") : loading ? t("render.loading") : ready ? t("render.embed") : operation ? t("render.reload") : t("render.title.fast")}
      </Button>
      <Button variant="ghost" disabled={!operation?.presentation.visible || preview.pending} onClick={() => void preview.hide()}><ThemeIcon name="hide" size={16} fallback={EyeOff} />{t("render.hidePreview")}</Button>
      <Button variant="ghost" disabled={!operation || !["starting", "ready"].includes(operation.state) || preview.pending} onClick={() => void preview.close()}><ThemeIcon name="close" size={16} fallback={X} />{loading ? t("render.cancelLoading") : t("render.closePreview")}</Button>
      <Button variant="ghost" disabled={!ready || capturing} onClick={() => void capture()}><ThemeIcon name="picture_trans" size={16} fallback={Image} />{capturing ? t("render.readingFrame") : t("render.setThumbnail")}</Button>
    </div>
    {error && <pre className="render-error" role="alert">{displayPathText(localizeMessage(error))}</pre>}
    {captureMessage && <div role="status">{displayPathText(localizeMessage(captureMessage))}</div>}
    <div className="render-preview-group">
      <div ref={preview.hostRef} className="render-preview-host" aria-label={t("render.fastViewport")}>
        {!ready && <div className="render-preview-placeholder"><ThemeIcon name="empty" size={40} fallback={Box} /><strong>{name}</strong><span>{status}</span></div>}
      </div>
      <div className="render-preview-help" role="status" aria-live="polite">{status}</div>
    </div>
    </>}
    {mode !== "fast" && captureMessage && <div role="status">{displayPathText(localizeMessage(captureMessage))}</div>}
  </section>;
}
