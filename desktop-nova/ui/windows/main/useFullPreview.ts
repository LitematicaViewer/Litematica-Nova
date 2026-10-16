import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import {
  cancelApplicationRender, discardApplicationRenderPreflight, ensureApplicationRenderSubscription,
  findRenderOperationForSource, getApplicationRenderState, isApplicationRenderPending,
  isApplicationViewerPrepared, isApplicationPresentationVisible, prepareApplicationRenderLaunch, resourcePreflightAllows,
  startApplicationRender, subscribeApplicationRender, updateApplicationRenderPresentation,
  useBasicVulkanForApplicationRender, windowsPathKey, type ResourcePreflightView,
} from "../../../src/business/facade";
import { useEmbeddedViewer } from "./useEmbeddedViewer";
import { elementToNativeChildPlacement } from "./embeddedViewerGeometry";

// A user-selected VRAM fallback is a product loading choice, not a renderer.
// Keep it across route mounts; changing the checkbox still selects a new contract.
const selectedLoadingByContext = new Map<string, { preload: boolean; buildMode: "prebuilt" | "realtime" }>();

/** The existing exact renderer owns the scene; the mounted route owns presentation only. */
export function useFullPreview(sourcePath: string, preload: boolean, blocked = false, displayMode: "full" | "standard" = "full") {
  const snapshot = useSyncExternalStore(subscribeApplicationRender, getApplicationRenderState, getApplicationRenderState);
  const [selection, setSelection] = useState<{ sourcePath: string; preload: boolean; displayMode: "full" | "standard"; buildMode: "prebuilt" | "realtime" } | null>(null);
  const contextKey = JSON.stringify([windowsPathKey(sourcePath), displayMode]);
  const retainedSelection = selectedLoadingByContext.get(contextKey);
  const selectedBuildMode = selection?.sourcePath === sourcePath && selection.preload === preload && selection.displayMode === displayMode
    ? selection.buildMode : retainedSelection?.preload === preload ? retainedSelection.buildMode : preload ? "prebuilt" : "realtime";
  const operation = findRenderOperationForSource(sourcePath, snapshot, { displayMode, buildMode: selectedBuildMode });
  const [preflight, setPreflight] = useState<ResourcePreflightView | null>(null);
  const [pending, setPending] = useState(false);
  const [launchStage, setLaunchStage] = useState<"preflight" | "starting" | null>(null);
  const [error, setError] = useState("");
  const current = useRef({ sourcePath, preload, displayMode }); current.current = { sourcePath, preload, displayMode };
  const generation = useRef(0);
  const pendingGrant = useRef<ResourcePreflightView | null>(null);
  const viewport = useEmbeddedViewer(operation, blocked || preflight !== null);
  const mounted = useRef(false);
  const clickedAt = useRef<number | null>(null);
  const inFlightLaunches = useRef(new Map<number, string>());
  const cancelledLaunches = useRef(new Set<number>());
  const previousContext = useRef({ sourcePath, preload, blocked, displayMode });
  useEffect(() => {
    if (operation?.error) console.error("[NOVA_RENDER_PHASE]", { stage: "render_failed", operationId: operation.operationId,
      state: operation.state, error: operation.error, displayMode, buildMode: operation.buildMode });
  }, [operation?.operationId, operation?.error]);

  async function discardGrant() {
    const grant = pendingGrant.current; pendingGrant.current = null; setPreflight(null);
    if (grant) await discardApplicationRenderPreflight(grant.preflightId);
  }
  useEffect(() => {
    mounted.current = true;
    void ensureApplicationRenderSubscription().catch(reason => setError(String(reason)));
    return () => {
      console.info("[NOVA_RENDER_PHASE]", { stage: "route_unmount", sourcePath: current.current.sourcePath, displayMode: current.current.displayMode, generation: generation.current + 1 });
      mounted.current = false; generation.current++;
      const grant = pendingGrant.current; pendingGrant.current = null;
      if (grant) void discardApplicationRenderPreflight(grant.preflightId).catch(() => undefined);

    };
  }, []);
  useEffect(() => {
    const previous = previousContext.current;
    previousContext.current = { sourcePath, preload, blocked, displayMode };
    if (previous.sourcePath === sourcePath && previous.preload === preload
      && previous.blocked === blocked && previous.displayMode === displayMode) return;
    console.info("[NOVA_RENDER_PHASE]", { stage: "render_context_changed", previous, current: { sourcePath, preload, blocked, displayMode }, generation: generation.current + 1 });
    generation.current++; setError(""); setPending(false); setLaunchStage(null); setSelection(null);
    void discardGrant().catch(reason => setError(String(reason)));
    viewport.requestPresentationWhenReady(null);
    if (operation) void updateApplicationRenderPresentation(operation.operationId, { mode: "hidden", visible: false, placement: null }).catch(() => undefined);
  }, [sourcePath, preload, blocked, displayMode]);


  async function execute(action: (version: number) => Promise<void>) {
    const version = ++generation.current; setPending(true); setLaunchStage(null); setError("");
    try { await action(version); } catch (reason) { if (mounted.current && generation.current === version) setError(String(reason)); }
    finally { if (mounted.current && generation.current === version) { setPending(false); setLaunchStage(null); } }
  }
  async function launch(buildMode: "prebuilt" | "realtime", version: number, grant?: ResourcePreflightView) {
    if (!mounted.current || generation.current !== version) return;
    if (grant && buildMode === "prebuilt" && !resourcePreflightAllows(grant, "continue_prebuilt")) throw new Error("当前显存门禁不允许预构建");
    setLaunchStage("starting");
    selectedLoadingByContext.set(JSON.stringify([windowsPathKey(current.current.sourcePath), current.current.displayMode]), { preload: current.current.preload, buildMode });
    setSelection({ ...current.current, buildMode });
    inFlightLaunches.current.set(version, JSON.stringify([windowsPathKey(current.current.sourcePath), current.current.displayMode]));
    let result;
    try {
      result = await startApplicationRender({ sourcePath: current.current.sourcePath, buildMode, displayMode: current.current.displayMode,
        preflightId: buildMode === "prebuilt" ? grant?.preflightId : undefined,
        resourceWarningConfirmed: buildMode === "prebuilt" && grant?.severity === "warning",
        realtimeDecisionId: buildMode === "realtime" ? grant?.preflightId : undefined,
        clickStartedAtEpochMs: clickedAt.current });
    } catch (reason) {
      cancelledLaunches.current.delete(version); throw reason;
    } finally { inFlightLaunches.current.delete(version); }
    if (cancelledLaunches.current.delete(version)) {
      await cancelApplicationRender(result.operationId); return;
    }
    // A page/source cancellation does not destroy the Viewer/GPU or let a late result show it.
    if (!mounted.current || generation.current !== version) {
      console.info("[NOVA_RENDER_PHASE]", { stage: "late_start_suppressed", operationId: result.operationId, requestGeneration: version, currentGeneration: generation.current });
      await updateApplicationRenderPresentation(result.operationId, { mode: "hidden", visible: false, placement: null }); return;
    }
    pendingGrant.current = null; setPreflight(null);
    viewport.requestPresentationWhenReady(result.operationId);
  }
  const start = () => execute(async version => {
    clickedAt.current = Date.now();
    console.info("[NOVA_FIRST_BLOCK]", { stage: "user_click", epochMs: clickedAt.current,
      sourcePath: current.current.sourcePath, displayMode: current.current.displayMode, preload: current.current.preload,
      existingOperationReady: isApplicationViewerPrepared(operation) });
    if (blocked) throw new Error("请先选择精确渲染模式");
    setLaunchStage("preflight");
    await discardGrant();
    if (!mounted.current || generation.current !== version) return;
    const expected = { ...current.current };
    const host = viewport.hostRef.current;
    if (!host) throw new Error("预览容器尚未就绪，请在页面可见后重新加载。");
    const placement = await elementToNativeChildPlacement(host);
    if (!Number.isInteger(placement.width) || !Number.isInteger(placement.height)
      || placement.width <= 8 || placement.height <= 8 || placement.width > 16384 || placement.height > 16384) {
      throw new Error("预览容器尺寸不可用，请在页面可见后重新加载。");
    }
    if (!mounted.current || generation.current !== version) return;
    const plan = await prepareApplicationRenderLaunch(expected.sourcePath, expected.preload, expected.displayMode,
      [placement.width, placement.height]);
    if (!mounted.current || generation.current !== version) {
      if (plan.preflight) await discardApplicationRenderPreflight(plan.preflight.preflightId); return;
    }
    const grant = plan.preflight;
    if (plan.directRealtime) { await launch("realtime", version); return; }
    if (!grant) throw new Error("加载计划缺少实际显存预检");
    pendingGrant.current = grant; setPreflight(grant);
    if (!expected.preload && grant.severity !== "invalid" && resourcePreflightAllows(grant, "switch_realtime")) {
      await launch("realtime", version, grant); return;
    }
    if (!resourcePreflightAllows(grant, "continue_prebuilt") || grant.severity === "warning") return;
    if (expected.preload) await launch("prebuilt", version, grant);
    else await launch("realtime", version, grant);
  });
  const choose = (action: "prebuilt" | "realtime" | "basic_vulkan" | "cancel") => execute(async version => {
    const grant = pendingGrant.current;
    if (!grant) throw new Error("显存预检已经结束，请重新加载");
    if (action === "cancel") { await discardGrant(); return; }
    if (action === "prebuilt") { await launch("prebuilt", version, grant); return; }
    if (action === "basic_vulkan") {
      if (!resourcePreflightAllows(grant, "use_basic_vulkan")) throw new Error("预检未允许切换 Vulkan");
      const next = await useBasicVulkanForApplicationRender(grant.preflightId, current.current.sourcePath);
      if (!mounted.current || generation.current !== version) {
        await discardApplicationRenderPreflight(next.preflightId); return;
      }
      pendingGrant.current = next; setPreflight(next); return;
    }
    if (grant.severity !== "normal" && !resourcePreflightAllows(grant, "switch_realtime")) throw new Error("预检未允许实时加载");
    await launch("realtime", version, grant);
  });
  const hide = () => execute(async () => {
    viewport.requestPresentationWhenReady(null);
    if (operation) await updateApplicationRenderPresentation(operation.operationId, { mode: "hidden", visible: false, placement: null });
  });
  const close = () => execute(async () => {
    const closingContext = JSON.stringify([windowsPathKey(current.current.sourcePath), current.current.displayMode]);
    selectedLoadingByContext.delete(closingContext);
    for (const [version, context] of inFlightLaunches.current) {
      if (context === closingContext) cancelledLaunches.current.add(version);
    }
    viewport.requestPresentationWhenReady(null); await discardGrant();
    if (operation) await cancelApplicationRender(operation.operationId);
  });
  const loading = isApplicationRenderPending(operation);
  const closed = operation?.state === "cancelled" || operation?.state === "runtime_released";
  const cancelling = operation?.progress?.phase === "cancelling";
  const progress = loading && !cancelling && !launchStage ? operation?.progress : undefined;
  const phaseLabel: Record<string, string> = { scan_nbt: "正在读取", nbt_scan: "正在读取", normalize: "正在整理", mesh_compile: "正在构建", compile_existing_cells: "正在构建", compile_sections: "正在构建", build_scene: "正在构建", encode: "正在保存", decode: "正在读取", artifact_publishing: "正在准备画面", viewer_preparing: "正在准备画面", cancelling: "正在取消" };
  const stage = phaseLabel[progress?.phase || ""] || "正在加载";
  const ready = isApplicationViewerPrepared(operation);
  const visible = ready && isApplicationPresentationVisible(operation);
  const operationFailure = operation?.state === "failed" && !pending ? "预览加载失败，请重新加载。" : "";
  // CPU completion is not a visible frame. Prepared only grants the native
  // presentation command; the service's post-Present receipt removes the UI placeholder.
  const progressLabel = pending && launchStage === "preflight" ? "正在预估显存…"
    : pending && launchStage === "starting" ? "正在启动渲染…"
    : closed ? "预览已关闭，可重新加载。"
    : operationFailure ? operationFailure
    : cancelling ? "正在取消预览…"
    : !operation && !pending ? "尚未加载预览"
    : ready && !visible
    ? operation?.presentation.visible ? "正在显示画面…" : "预览已隐藏"
    : !ready && progress?.completed != null && progress.total != null && progress.total > 0 && progress.completed >= progress.total
    ? "正在准备画面…"
    : progress?.completed != null && progress.total != null && progress.total > 0
    ? `${stage} ${Math.min(100, Math.max(0, Math.round(progress.completed / progress.total * 100)))}%（${progress.completed.toLocaleString()} / ${progress.total.toLocaleString()}）`
    : `${stage}…`;
  return { ...viewport, operation, pending, progress, progressLabel, error: error || viewport.error || operationFailure, preflight, start, choose, hide, close,
    ready, visible, loading };
}
