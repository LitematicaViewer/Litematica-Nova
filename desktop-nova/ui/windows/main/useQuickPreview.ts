import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import {
  closeQuickPreview, startQuickPreview, getQuickPreviewState, quickPreviewForSource, refreshQuickPreview, restoreQuickPreview,
  subscribeQuickPreview, suspendQuickPreview, updateQuickPreviewPlacement, presentQuickPreview,
} from "../../../src/business/facade";
import { usePresentationViewport, type PresentationViewportController } from "./useEmbeddedViewer";

const quickController: PresentationViewportController = {
  inlineMode: "embedded",
  ensureSubscription: refreshQuickPreview,
  restore: restoreQuickPreview,
  suspend: suspendQuickPreview,
  updatePlacement: updateQuickPreviewPlacement,
  present: presentQuickPreview,
};

/** One historical fast_experimental entry for Properties and Render. Full is not a fallback. */
export function useQuickPreview(sourcePath: string, blocked: boolean) {
  const snapshot = useSyncExternalStore(subscribeQuickPreview, getQuickPreviewState, getQuickPreviewState);
  const operation = quickPreviewForSource(sourcePath, snapshot);
  const viewport = usePresentationViewport(operation ? {
    operationId: operation.previewId, prepared: operation.state === "ready", presentation: operation.presentation,
  } : null, blocked, quickController);
  const [pending, setPending] = useState(false);
  const [commandError, setCommandError] = useState("");
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
    catch (reason) { if (source.current === expectedSource) setCommandError(`快速预览失败：${String(reason)}`); }
    finally { inFlight.current = false; setPending(false); }
  }
  const start = () => execute(async () => {
    if (operation?.state === "ready") { await viewport.showOverlay(); return; }
    if (operation?.state === "starting" && !operation.error) return;
    const started = await startQuickPreview(sourcePath);
    if (source.current === sourcePath) viewport.requestPresentationWhenReady(started.previewId);
  });
  const close = () => execute(async () => { if (operation) await closeQuickPreview(operation.previewId); });
  const hide = () => execute(async () => { if (operation) await viewport.setPresentation({ ...operation.presentation, visible: false }); });
  return { ...viewport, operation, pending, error: commandError || viewport.error, start, close, hide };
}
