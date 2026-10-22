import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import {
  closeQuickPreview, startQuickPreview, getQuickPreviewState, quickPreviewForSource, refreshQuickPreview, restoreQuickPreview,
  subscribeQuickPreview, suspendQuickPreview, updateQuickPreviewPlacement, presentQuickPreview,
} from "../../../src/business/facade";
import { usePresentationViewport, type PresentationViewportController } from "./useEmbeddedViewer";

const quickController: PresentationViewportController = {
  inlineMode: "embedded",
  deferPresentationUntilReady: true,
  ensureSubscription: refreshQuickPreview,
  restore: restoreQuickPreview,
  suspend: suspendQuickPreview,
  updatePlacement: updateQuickPreviewPlacement,
  present: presentQuickPreview,
};

/** One historical fast_experimental entry for Properties and Render. Full is not a fallback. */
export function useQuickPreview(sourcePath: string, blocked: boolean, preload = false) {
  useI18n();
  const snapshot = useSyncExternalStore(subscribeQuickPreview, getQuickPreviewState, getQuickPreviewState);
  const operation = quickPreviewForSource(sourcePath, snapshot);
  const viewport = usePresentationViewport(operation ? {
    operationId: operation.previewId, prepared: operation.state === "ready", presentation: operation.presentation,
  } : null, blocked, quickController);
  const [pending, setPending] = useState(false);
  const [commandError, setCommandError] = useState<UiMessage>("");
  const inFlight = useRef(false);
  const source = useRef(sourcePath);
  source.current = sourcePath;
  useEffect(() => { setCommandError(""); }, [sourcePath]);
  async function execute(action: () => Promise<void>) {
    if (inFlight.current || !sourcePath) return;
    const expectedSource = sourcePath;
    inFlight.current = true;
    setPending(true);
    setCommandError("");
    try { await action(); }
    catch (reason) { if (source.current === expectedSource) setCommandError(message("render.quickFailed", { error: String(reason) })); }
    finally { inFlight.current = false; setPending(false); }
  }
  const start = () => execute(async () => {
    const clickStartedAtEpochMs = Date.now();
    console.info("[NOVA_FIRST_BLOCK]", { stage: "user_click", mode: "fast", epochMs: clickStartedAtEpochMs, sourcePath, preload,
      existingGpuAndSceneReady: operation?.state === "ready" && (operation.preload ?? false) === preload });
    if (operation?.state === "ready" && (operation.preload ?? false) === preload) {
      // Host dedupe retains the same Viewer/GPU and forwards only this click's
      // clock to the next native presentation command.
      await startQuickPreview(sourcePath, preload, clickStartedAtEpochMs);
      await viewport.showOverlay(); return;
    }
    if (operation?.state === "starting" && !operation.error) return;
    const started = await startQuickPreview(sourcePath, preload, clickStartedAtEpochMs);
    if (source.current === sourcePath) viewport.requestPresentationWhenReady(started.previewId);
  });
  const close = () => execute(async () => { if (operation) await closeQuickPreview(operation.previewId); });
  const hide = () => execute(async () => { if (operation) await viewport.setPresentation({ ...operation.presentation, visible: false }); });
  return { ...viewport, operation, pending, error: localizeMessage(commandError || viewport.error || (operation?.error ? t("render.loadingFailed") : "")), start, close, hide };
}
