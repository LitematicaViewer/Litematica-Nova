import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  type PresentationIntent,
  type PresentationPlacement,
} from "../../../src/business/facade";
import {
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
  inlineMode?: "embedded";
  ensureSubscription(): Promise<void>;
  restore(id: string, placement: PresentationPlacement): Promise<boolean>;
  suspend(id: string, observed: PresentationIntent): Promise<boolean>;
  updatePlacement(id: string, placement: PresentationPlacement, observed: PresentationIntent): Promise<boolean>;
  present(id: string, intent: PresentationIntent): Promise<void>;
}
/** Existing quick Viewer Presentation; no Full runtime controller or overlay implementation. */
export function usePresentationViewport(operation: PresentationViewportOperation | null, blocked: boolean, controller: PresentationViewportController) {
  const inlineMode = "embedded" as const;
  const resolvePlacement = elementToNativeChildPlacement;
  const useRouteEffect = useLayoutEffect;
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [error, setError] = useState("");
  const current = useRef({ operation, blocked });
  current.current = { operation, blocked };
  const routeLease = useRef(0);
  const presentationRevision = useRef(0);
  const lastPlacement = useRef("");
  const geometrySuspended = useRef(false);
  const pageHidden = useRef(false);
  const [visibilityRevision, setVisibilityRevision] = useState(0);
  const [requestedOperationId, requestPresentationWhenReady] = useState<string | null>(null);
  const ready = Boolean(operation?.prepared);

  useEffect(() => {
    let active = true;
    void controller.ensureSubscription().catch((reason) => {
      if (active) setError(`连接预览服务失败：${String(reason)}`);
    });
    return () => { active = false; };
  }, []);

  // Only route ownership controls the lifetime of this lease. Readiness events
  // and modal changes must not recreate a Viewer or release an exact artifact.
  useRouteEffect(() => {
    ++routeLease.current;
    lastPlacement.current = "";
    geometrySuspended.current = false;
    setError("");
    const operationId = operation?.operationId;
    const observedIntent = operation?.presentation;
    return () => {
      ++routeLease.current;
      ++presentationRevision.current;
      if (!operationId || !observedIntent) return;
      const latest = current.current.operation;
      const intent = latest?.operationId === operationId ? latest.presentation : observedIntent;
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
      pageHidden.current = true;
      geometrySuspended.current = true;
      ++presentationRevision.current;
      const target = current.current.operation;
      if (target) void controller.suspend(target.operationId, target.presentation).catch(() => undefined);
    };
    const restorePage = () => {
      if (document.visibilityState === "hidden") return;
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
      if (!cancelled && !pageHidden.current) setVisibilityRevision((revision) => revision + 1);
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
    if (!target || !target.prepared || suspended || pageHidden.current || !host) return;
    requestPresentationWhenReady(null);
    const lease = routeLease.current;
    const revision = ++presentationRevision.current;
    const placement = await resolvePlacement(host);
    if (routeLease.current !== lease || presentationRevision.current !== revision || current.current.blocked || pageHidden.current
      || current.current.operation?.operationId !== target.operationId || !isUsableEmbeddedRect(placement)) return;
    const intent: PresentationIntent = { mode: inlineMode, visible: true, placement };
    await controller.present(target.operationId, intent);
    lastPlacement.current = placementKey(intent);
    setError("");
  }

  async function setPresentation(intent: PresentationIntent) {
    const target = current.current.operation;
    if (!target) return;
    requestPresentationWhenReady(null);
    ++presentationRevision.current;
    await controller.present(target.operationId, intent);
    setError("");
  }

  // A modal temporarily hides the native overlay (CSS z-index cannot occlude a
  // native window). Popup and an explicit Hide remain the user's own intent.
  useEffect(() => {
    if (!operation || !ready) return;
    let cancelled = false;
    const lease = routeLease.current;
    const revision = presentationRevision.current;
    const operationId = operation.operationId;
    if (blocked) {
      void controller.suspend(operationId, operation.presentation)
        .catch((reason) => { if (!cancelled) setError(`暂停页内预览失败：${String(reason)}`); });
      return () => { cancelled = true; };
    }
    const restore = async () => {
      const host = hostRef.current;
      if (!host) return;
      const placement = await resolvePlacement(host);
      if (cancelled || current.current.blocked || pageHidden.current || routeLease.current !== lease || presentationRevision.current !== revision || !isUsableEmbeddedRect(placement)) return;
      const restored = await controller.restore(operationId, placement);
      if (cancelled || current.current.blocked || pageHidden.current || routeLease.current !== lease || presentationRevision.current !== revision) return;
      if (restored) lastPlacement.current = placementKey({ mode: inlineMode, visible: true, placement });
      // Hidden is also a valid explicit user intent. Only a start action on
      // this mounted page grants automatic presentation after the ready gate.
      if (!restored && requestedOperationId === operationId && operation.presentation.mode === "hidden") {
        requestPresentationWhenReady(null);
        await showOverlay();
      }
    };
    void restore().catch((reason) => { if (!cancelled) setError(`显示页内预览失败：${String(reason)}`); });
    return () => { cancelled = true; };
  }, [operation?.operationId, ready, blocked, requestedOperationId, visibilityRevision]);

  useEffect(() => {
    if (!operation || !ready || blocked || operation.presentation.mode !== inlineMode || (!operation.presentation.visible && !geometrySuspended.current)) return;
    const operationId = operation.operationId;
    const observedIntent = operation.presentation;
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
        if (await controller.updatePlacement(operationId, placement, observedIntent)) lastPlacement.current = key;
      } catch (reason) {
        if (!cancelled) setError(`更新预览位置失败：${String(reason)}`);
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
  }, [operation?.operationId, ready, blocked, operation?.presentation.mode, operation?.presentation.visible, visibilityRevision]);

  return { hostRef, error, showOverlay, setPresentation, requestPresentationWhenReady };
}
