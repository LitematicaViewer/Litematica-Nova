import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ensureApplicationRenderSubscription,
  isApplicationViewerPrepared,
  restoreApplicationRenderPresentationForRoute,
  suspendApplicationRenderPresentationForRoute,
  updateApplicationRenderOverlayPlacement,
  updateApplicationRenderPresentation,
  type PresentationIntent,
  type PresentationPlacement,
  type RenderOperationView,
} from "../../../src/business/facade";
import {
  elementToPresentationPlacement,
  elementToNativeChildPlacement,
  isUsableEmbeddedRect,
  subscribeNativeWindowGeometry,
} from "./embeddedViewerGeometry";

function placementKey(intent: PresentationIntent) {
  const p = intent.placement;
  return p ? [intent.mode, intent.visible, p.x, p.y, p.width, p.height, p.dpiScale, p.focused].join(":") : "";
}

export interface PresentationViewportOperation {
  operationId: string;
  prepared: boolean;
  presentation: PresentationIntent;
}
export interface PresentationViewportController {
  inlineMode?: "overlay" | "embedded";
  deferPresentationUntilReady?: boolean;
  ensureSubscription(): Promise<void>;
  restore(id: string, placement: PresentationPlacement): Promise<boolean>;
  suspend(id: string, observed: PresentationIntent): Promise<boolean>;
  updatePlacement(id: string, placement: PresentationPlacement, observed: PresentationIntent): Promise<boolean>;
  present(id: string, intent: PresentationIntent): Promise<void>;
}
const applicationController: PresentationViewportController = {
  inlineMode: "embedded",
  deferPresentationUntilReady: true,
  ensureSubscription: ensureApplicationRenderSubscription,
  restore: restoreApplicationRenderPresentationForRoute,
  suspend: suspendApplicationRenderPresentationForRoute,
  updatePlacement: updateApplicationRenderOverlayPlacement,
  present: updateApplicationRenderPresentation,
};

/** The full Viewer keeps its existing Application operation and admission contract. */
export function useEmbeddedViewer(operation: RenderOperationView | null, blocked = false) {
  useI18n();
  return usePresentationViewport(operation ? {
    operationId: operation.operationId, prepared: isApplicationViewerPrepared(operation), presentation: operation.presentation,
  } : null, blocked, applicationController);
}

/** Presentation only: both existing Viewers share placement/route/focus handling, not render state. */
export function usePresentationViewport(operation: PresentationViewportOperation | null, blocked: boolean, controller: PresentationViewportController) {
  const inlineMode = controller.inlineMode ?? "overlay";
  const resolvePlacement = inlineMode === "embedded" ? elementToNativeChildPlacement : elementToPresentationPlacement;
  const useRouteEffect = inlineMode === "embedded" ? useLayoutEffect : useEffect;
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [error, setError] = useState<UiMessage>("");
  const leasedOperation = useRef(operation);
  if (operation?.operationId === leasedOperation.current?.operationId) leasedOperation.current = operation;
  const current = useRef({ operation, blocked });
  current.current = { operation, blocked };
  const routeLease = useRef(0);
  const presentationRevision = useRef(0);
  const lastPlacement = useRef("");
  // Host can accept the deferred intent before its event stream catches up.
  const queuedInlineIntent = useRef<{ operationId: string; presentation: PresentationIntent } | null>(null);
  const geometrySuspended = useRef(false);
  const pageHidden = useRef(false);
  const [visibilityRevision, setVisibilityRevision] = useState(0);
  const [requestedOperationId, requestPresentationWhenReady] = useState<string | null>(null);
  const ready = Boolean(operation?.prepared);
  const mayQueuePresentation = ready || controller.deferPresentationUntilReady === true;
  if (operation && queuedInlineIntent.current?.operationId === operation.operationId
    && placementKey(operation.presentation) === placementKey(queuedInlineIntent.current.presentation)) {
    queuedInlineIntent.current = null;
  }

  useEffect(() => {
    let active = true;
    void controller.ensureSubscription().catch((reason) => {
      if (active) setError(message("render.connectFailed", { error: String(reason) }));
    });
    return () => { active = false; };
  }, []);

  // Only route ownership controls the lifetime of this lease. Readiness events
  // and modal changes must not recreate a Viewer or release an exact artifact.
  useRouteEffect(() => {
    ++routeLease.current;
    leasedOperation.current = operation;
    console.info("[NOVA_RENDER_PHASE]", { stage: "presentation_route_mounted", operationId: operation?.operationId, lease: routeLease.current, inlineMode });
    lastPlacement.current = "";
    queuedInlineIntent.current = null;
    geometrySuspended.current = false;
    setError("");
    const operationId = operation?.operationId;
    const observedIntent = operation?.presentation;
    return () => {
      ++routeLease.current;
      ++presentationRevision.current;
      if (!operationId || !observedIntent) return;
      const latest = leasedOperation.current;
      const intent = queuedInlineIntent.current?.operationId === operationId ? queuedInlineIntent.current.presentation
        : latest?.operationId === operationId ? latest.presentation : observedIntent;
      console.info("[NOVA_RENDER_PHASE]", { stage: "presentation_route_suspended", operationId, lease: routeLease.current, previousMode: intent.mode });
      void controller.suspend(operationId, intent).catch(() => undefined);
    };
  }, [operation?.operationId]);

  // Listen even while Ready is pending or the viewport is currently suspended.
  // A document/page hide invalidates every outstanding geometry read immediately.
  useEffect(() => {
    if (inlineMode !== "embedded") return;
    let cancelled = false;
    let unsubscribeNativeWindow: (() => void) | null = null;
    const suspendPage = () => {
      console.info("[NOVA_RENDER_PHASE]", { stage: "presentation_page_hidden", operationId: current.current.operation?.operationId, lease: routeLease.current });
      pageHidden.current = true;
      geometrySuspended.current = true;
      ++presentationRevision.current;
      const target = current.current.operation;
      if (target) {
        const intent = queuedInlineIntent.current?.operationId === target.operationId ? queuedInlineIntent.current.presentation : target.presentation;
        void controller.suspend(target.operationId, intent).catch(() => undefined);
      }
    };
    const restorePage = () => {
      if (document.visibilityState === "hidden") return;
      console.info("[NOVA_RENDER_PHASE]", { stage: "presentation_page_restored", operationId: current.current.operation?.operationId, lease: routeLease.current });
      pageHidden.current = false;
      setVisibilityRevision((revision) => revision + 1);
    };
    const visibilityChanged = () => document.visibilityState === "hidden" ? suspendPage() : restorePage();
    document.addEventListener("visibilitychange", visibilityChanged);
    window.addEventListener("pagehide", suspendPage);
    window.addEventListener("pageshow", restorePage);
    // Native restore notifications are needed even if Ready arrived while the
    // parent was minimized and no visible geometry effect has been mounted yet.
    void subscribeNativeWindowGeometry(() => {
      if (!cancelled && !pageHidden.current) {
        // Invalidate pending native geometry reads in this callback, not after
        // React commits the state update. A resize/minimize can otherwise let
        // the previous visible rect publish once after the native transition.
        ++presentationRevision.current;
        setVisibilityRevision((revision) => revision + 1);
      }
    }).then((unsubscribe) => {
      if (cancelled) unsubscribe(); else unsubscribeNativeWindow = unsubscribe;
    }).catch((reason) => console.warn("[NOVA_PRESENTATION] subscribe window geometry failed", reason));
    if (document.visibilityState === "hidden") suspendPage();
    return () => {
      cancelled = true;
      unsubscribeNativeWindow?.();
      document.removeEventListener("visibilitychange", visibilityChanged);
      window.removeEventListener("pagehide", suspendPage);
      window.removeEventListener("pageshow", restorePage);
    };
  }, []);

  async function showOverlay() {
    const { operation: target, blocked: suspended } = current.current;
    const host = hostRef.current;
    if (!target || (!target.prepared && !controller.deferPresentationUntilReady) || suspended || pageHidden.current || !host) return;
    requestPresentationWhenReady(null);
    const lease = routeLease.current;
    const revision = ++presentationRevision.current;
    const placement = await resolvePlacement(host);
    if (routeLease.current !== lease || presentationRevision.current !== revision || current.current.blocked || pageHidden.current
      || current.current.operation?.operationId !== target.operationId || !isUsableEmbeddedRect(placement)) return;
    const intent: PresentationIntent = { mode: inlineMode, visible: true, placement };
    if (controller.deferPresentationUntilReady) queuedInlineIntent.current = { operationId: target.operationId, presentation: intent };
    await controller.present(target.operationId, intent);
    lastPlacement.current = placementKey(intent);
    setError("");
  }

  async function setPresentation(intent: PresentationIntent) {
    const target = current.current.operation;
    if (!target) return;
    requestPresentationWhenReady(null);
    queuedInlineIntent.current = null;
    ++presentationRevision.current;
    await controller.present(target.operationId, intent);
    setError("");
  }

  // The Service keeps native visibility hidden until real GPU readiness. The
  // route can update or suspend its pending desired placement before that gate.
  useEffect(() => {
    if (!operation || !mayQueuePresentation) return;
    let cancelled = false;
    const lease = routeLease.current;
    const revision = presentationRevision.current;
    const operationId = operation.operationId;
    if (blocked) {
      ++presentationRevision.current;
      const intent = queuedInlineIntent.current?.operationId === operationId ? queuedInlineIntent.current.presentation : operation.presentation;
      void controller.suspend(operationId, intent)
        .catch((reason) => { if (!cancelled) setError(message("render.pauseFailed", { error: String(reason) })); });
      return () => { cancelled = true; };
    }
    const restore = async () => {
      const host = hostRef.current;
      if (!host) return;
      const placement = await resolvePlacement(host);
      if (cancelled || current.current.blocked || pageHidden.current || routeLease.current !== lease || presentationRevision.current !== revision
        || current.current.operation?.operationId !== operationId || !isUsableEmbeddedRect(placement)) return;
      const restored = await controller.restore(operationId, placement);
      if (cancelled || current.current.blocked || pageHidden.current || routeLease.current !== lease || presentationRevision.current !== revision) return;
      const intent: PresentationIntent = { mode: inlineMode, visible: true, placement };
      if (restored) {
        if (controller.deferPresentationUntilReady) queuedInlineIntent.current = { operationId, presentation: intent };
        lastPlacement.current = placementKey(intent);
      }
      // Only an explicit start on this mounted page owns a deferred Show.
      if (!restored && requestedOperationId === operationId && operation.presentation.mode === "hidden") {
        if (controller.deferPresentationUntilReady) queuedInlineIntent.current = { operationId, presentation: intent };
        requestPresentationWhenReady(null);
        await controller.present(operationId, intent);
        if (!cancelled && routeLease.current === lease && presentationRevision.current === revision
          && current.current.operation?.operationId === operationId && !current.current.blocked && !pageHidden.current) {
          lastPlacement.current = placementKey(intent);
        }
      }
    };
    void restore().catch((reason) => { if (!cancelled) setError(message("render.showFailed", { error: String(reason) })); });
    return () => { cancelled = true; };
  }, [operation?.operationId, mayQueuePresentation, blocked, requestedOperationId, visibilityRevision]);

  useEffect(() => {
    if (!operation || !mayQueuePresentation || blocked) return;
    const operationId = operation.operationId;
    const observedIntent = queuedInlineIntent.current?.operationId === operationId ? queuedInlineIntent.current.presentation : operation.presentation;
    if (observedIntent.mode !== inlineMode || (!observedIntent.visible && !geometrySuspended.current)) return;
    let cancelled = false;
    let scheduled = 0;
    let inFlight = false;
    let queued = false;
    let unsubscribeNativeWindow: (() => void) | null = null;
    const lease = routeLease.current;
    const isCurrent = () => !cancelled && routeLease.current === lease && !current.current.blocked
      && current.current.operation?.operationId === operationId;
    const update = async () => {
      if (!isCurrent() || pageHidden.current) return;
      if (inFlight) { queued = true; return; }
      const host = hostRef.current;
      if (!host) return;
      inFlight = true;
      const revision = presentationRevision.current;
      try {
        const placement = await resolvePlacement(host);
        if (!isCurrent() || pageHidden.current || presentationRevision.current !== revision) return;
        if (!isUsableEmbeddedRect(placement)) {
          if (!geometrySuspended.current) {
            geometrySuspended.current = true;
            await controller.suspend(operationId, observedIntent);
          }
          return;
        }
        if (geometrySuspended.current) {
          geometrySuspended.current = false;
          await controller.restore(operationId, placement);
          if (!isCurrent() || pageHidden.current || presentationRevision.current !== revision) return;
          lastPlacement.current = "";
        }
        const intent: PresentationIntent = { mode: inlineMode, visible: true, placement };
        const key = placementKey(intent);
        if (key === lastPlacement.current) return;
        if (await controller.updatePlacement(operationId, placement, observedIntent)
          && isCurrent() && !pageHidden.current && presentationRevision.current === revision) {
          if (controller.deferPresentationUntilReady) queuedInlineIntent.current = { operationId, presentation: intent };
          lastPlacement.current = key;
        }
      } catch (reason) {
        if (!cancelled) setError(message("render.moveFailed", { error: String(reason) }));
      } finally {
        inFlight = false;
        if (!cancelled && queued) { queued = false; schedule(); }
      }
    };
    function schedule() {
      window.clearTimeout(scheduled);
      scheduled = window.setTimeout(() => void update(), 40);
    }
    const observer = new ResizeObserver(schedule);
    if (hostRef.current) observer.observe(hostRef.current);
    window.addEventListener("resize", schedule);
    window.addEventListener("scroll", schedule, true);
    window.addEventListener("focus", schedule);
    window.addEventListener("blur", schedule);
    if (inlineMode !== "embedded") void subscribeNativeWindowGeometry(schedule).then((unsubscribe) => {
      if (cancelled) unsubscribe(); else unsubscribeNativeWindow = unsubscribe;
    }).catch((reason) => console.warn("[NOVA_PRESENTATION] subscribe window geometry failed", reason));
    void update();
    return () => {
      cancelled = true;
      window.clearTimeout(scheduled);
      observer.disconnect();
      unsubscribeNativeWindow?.();
      window.removeEventListener("resize", schedule);
      window.removeEventListener("scroll", schedule, true);
      window.removeEventListener("focus", schedule);
      window.removeEventListener("blur", schedule);
    };
  }, [operation?.operationId, mayQueuePresentation, blocked, operation?.presentation.mode, operation?.presentation.visible, requestedOperationId, visibilityRevision]);

  return { hostRef, error: localizeMessage(error), showOverlay, setPresentation, requestPresentationWhenReady };
}
