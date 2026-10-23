import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
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
export function useFullPreview(sourcePath: string, preload: boolean, blocked = false, displayMode: "full" | "standard" = "full", layerView?: { range: [number, number] | null }) {
  useI18n();
  const snapshot = useSyncExternalStore(subscribeApplicationRender, getApplicationRenderState, getApplicationRenderState);
  const [selection, setSelection] = useState<{ sourcePath: string; preload: boolean; displayMode: "full" | "standard"; buildMode: "prebuilt" | "realtime" } | null>(null);
  const isLayerView = layerView !== undefined;
  const layerViewRef = useRef(layerView); layerViewRef.current = layerView;
  const contextKey = JSON.stringify([windowsPathKey(sourcePath), displayMode, isLayerView]);
  const retainedSelection = selectedLoadingByContext.get(contextKey);
  const selectedBuildMode = selection?.sourcePath === sourcePath && selection.preload === preload && selection.displayMode === displayMode
    ? selection.buildMode : retainedSelection?.preload === preload ? retainedSelection.buildMode : preload ? "prebuilt" : "realtime";
  const operation = findRenderOperationForSource(sourcePath, snapshot, { displayMode, buildMode: selectedBuildMode, layerView: isLayerView });
  const [preflight, setPreflight] = useState<ResourcePreflightView | null>(null);
  const [pending, setPending] = useState(false);
  const [launchStage, setLaunchStage] = useState<"preflight" | "starting" | null>(null);
  const [error, setError] = useState<UiMessage>("");
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
    if (grant && buildMode === "prebuilt" && !resourcePreflightAllows(grant, "continue_prebuilt")) throw new Error(t("render.preloadRejected"));
    setLaunchStage("starting");
    selectedLoadingByContext.set(JSON.stringify([windowsPathKey(current.current.sourcePath), current.current.displayMode, isLayerView]), { preload: current.current.preload, buildMode });
    setSelection({ ...current.current, buildMode });
    inFlightLaunches.current.set(version, JSON.stringify([windowsPathKey(current.current.sourcePath), current.current.displayMode, isLayerView]));
    let result;
    try {
      result = await startApplicationRender({ sourcePath: current.current.sourcePath, buildMode, displayMode: current.current.displayMode,
        preflightId: buildMode === "prebuilt" ? grant?.preflightId : undefined,
        resourceWarningConfirmed: buildMode === "prebuilt" && grant?.severity === "warning",
        realtimeDecisionId: buildMode === "realtime" ? grant?.preflightId : undefined,
        clickStartedAtEpochMs: clickedAt.current, layerView: isLayerView, initialLayerRange: layerViewRef.current?.range });
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
    if (blocked) throw new Error(t("render.selectExactMode"));
    setLaunchStage("preflight");
    await discardGrant();
    if (!mounted.current || generation.current !== version) return;
    const expected = { ...current.current };
    const host = viewport.hostRef.current;
    if (!host) throw new Error(t("render.containerNotReady"));
    const placement = await elementToNativeChildPlacement(host);
    if (!Number.isInteger(placement.width) || !Number.isInteger(placement.height)
      || placement.width <= 8 || placement.height <= 8 || placement.width > 16384 || placement.height > 16384) {
      throw new Error(t("render.containerSizeUnavailable"));
    }
    if (!mounted.current || generation.current !== version) return;
    // No prediction, warning or preflight task belongs to the layer route.
    // Runtime GPU allocation/residency safety remains in the existing Viewer.
    if (isLayerView) {
      if (operation && !["failed", "cancelled", "runtime_released"].includes(operation.state)) {
        viewport.requestPresentationWhenReady(operation.operationId);
        return;
      }
      await launch("realtime", version); return;
    }
    const plan = await prepareApplicationRenderLaunch(expected.sourcePath, expected.preload, expected.displayMode,
      [placement.width, placement.height]);
    if (!mounted.current || generation.current !== version) {
      if (plan.preflight) await discardApplicationRenderPreflight(plan.preflight.preflightId); return;
    }
    const grant = plan.preflight;
    if (plan.directRealtime) { await launch("realtime", version); return; }
    if (!grant) throw new Error(t("render.missingPreflight"));
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
    if (!grant) throw new Error(t("render.expiredPreflight"));
    if (action === "cancel") { await discardGrant(); return; }
    if (action === "prebuilt") { await launch("prebuilt", version, grant); return; }
    if (action === "basic_vulkan") {
      if (!resourcePreflightAllows(grant, "use_basic_vulkan")) throw new Error(t("render.vulkanNotApproved"));
      const next = await useBasicVulkanForApplicationRender(grant.preflightId, current.current.sourcePath);
      if (!mounted.current || generation.current !== version) {
        await discardApplicationRenderPreflight(next.preflightId); return;
      }
      pendingGrant.current = next; setPreflight(next); return;
    }
    if (grant.severity !== "normal" && !resourcePreflightAllows(grant, "switch_realtime")) throw new Error(t("render.realtimeNotApproved"));
    await launch("realtime", version, grant);
  });
  const hide = () => execute(async () => {
    viewport.requestPresentationWhenReady(null);
    if (operation) await updateApplicationRenderPresentation(operation.operationId, { mode: "hidden", visible: false, placement: null });
  });
  const close = () => execute(async () => {
    const closingContext = JSON.stringify([windowsPathKey(current.current.sourcePath), current.current.displayMode, isLayerView]);
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
  const phaseLabel: Record<string, string> = {
    starting: t("render.phase.starting"), scan_nbt: t("render.phase.reading"), nbt_scan: t("render.phase.reading"), load_litematic: t("render.phase.reading"),
    identity: t("render.phase.verifying"), native_cache_identity: t("render.phase.verifying"), normalize: t("render.phase.normalizing"), normalize_canonical_ir: t("render.phase.normalizing"),
    load_mc26_catalog: t("render.phase.blockCatalog"), build_mc26_schemas: t("render.phase.blockStates"), load_resource_packs: t("render.phase.resources"),
    bind_world: t("render.phase.sceneResources"), load_biome_tint: t("render.phase.biomeColors"),
    prepare_shared_lighting: t("render.phase.sharedLighting"),
    mesh_compile: t("render.phase.building"), compile_existing_cells: t("render.phase.building"), compile_sections: t("render.phase.building"), build_scene: t("render.phase.building"),
    encode: t("render.phase.saving"), container: t("render.phase.saving"), checksum: t("render.phase.checkingCache"), decode: t("render.phase.readingCache"), install: t("render.phase.savingCache"),
    artifact_publishing: t("render.phase.preparingFrame"), viewer_preparing: t("render.phase.preparingFrame"), cancelling: t("render.phase.cancelling"),
  };
  const stage = phaseLabel[progress?.phase || ""] || t("render.phase.loading");
  const lightingProgress = progress?.phase === "prepare_shared_lighting";
  const geometryProgress = ["mesh_compile", "compile_existing_cells", "compile_sections", "build_scene"].includes(progress?.phase || "");
  // A resource stage's 1/1 is not task completion. Only measured units from
  // their own phase may drive a bounded bar; unknown work stays indeterminate.
  const countedProgress = geometryProgress || (lightingProgress && progress?.activity === "light_sections")
    || ["encode", "container", "checksum", "decode"].includes(progress?.phase || "");
  const progressUnits = countedProgress && progress?.completed != null && progress.total != null
    && Number.isSafeInteger(progress.completed) && Number.isSafeInteger(progress.total)
    && progress.completed >= 0 && progress.total > 0 && progress.completed <= progress.total
    ? { completed: progress.completed, total: progress.total } : null;
  const progressRange = lightingProgress && progressUnits?.completed === 0 ? null : progressUnits;
  const lightingPercent = progressUnits ? Math.floor(progressUnits.completed / progressUnits.total * 100) : 0;
  const ready = isApplicationViewerPrepared(operation);
  const visible = ready && isApplicationPresentationVisible(operation);
  const operationFailure = operation?.state === "failed" && !pending
    ? operation.progress.phase === "viewer_failed"
      ? /PPLL|ZeroCaptureEpoch|CaptureStatusEpochMismatch|ResolveParameters/i.test(operation.error || "")
        ? t("render.transparencyFailed") : t("render.renderFailed")
      : t("render.loadingFailed")
    : "";
  // CPU completion is not a visible frame. Prepared only grants the native
  // presentation command; the service's post-Present receipt removes the UI placeholder.
  const progressLabel = pending && launchStage === "preflight" ? t("render.estimatingMemory")
    : pending && launchStage === "starting" ? t("render.starting")
    : closed ? t("render.closed")
    : operationFailure ? operationFailure
    : cancelling ? t("render.cancelling")
    : !operation && !pending ? t("render.notLoaded")
    : ready && !visible
    ? operation?.presentation.visible ? t("render.presenting") : t("render.hidden")
    : !ready && geometryProgress && progressRange && progressRange.completed === progressRange.total
    ? t("render.preparingFrame")
    : lightingProgress && progressUnits
    ? t("render.lightingProgress", { phase: stage, completed: progressUnits.completed, total: progressUnits.total, detail: progressUnits.completed > 0 ? `${getUiLocale() === "zh-CN" ? "，" : ", "}${lightingPercent === 0 ? "<1" : lightingPercent}%` : "", suffix: progressUnits.completed === 0 ? "…" : "" })
    : progressRange
    ? `${stage} ${Math.round(progressRange.completed / progressRange.total * 100)}%（${progressRange.completed.toLocaleString()} / ${progressRange.total.toLocaleString()}）`
    : `${stage}…`;
  return { ...viewport, operation, pending, progress, progressRange, progressLabel, error: localizeMessage(operationFailure || error || viewport.error), preflight, start, choose, hide, close,
    ready, visible, loading };
}
