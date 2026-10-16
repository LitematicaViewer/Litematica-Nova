import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { invoke } from "./tauri";

export type RenderOperationState =
  | "requested"
  | "running"
  | "build_completed"
  | "artifact_ready"
  | "viewer_preparing"
  | "realtime_ready"
  | "gpu_accepted"
  | "gpu_resident"
  | "viewer_prepared"
  | "runtime_released"
  | "cancelled"
  | "failed";

// Native child presentation is shared by the existing quick and exact viewers.
export type PresentationMode = "hidden" | "popup" | "overlay" | "embedded";
export type PresentationState = "hidden" | "visible";

export interface ProjectSource {
  path: string;
}

export interface PresentationPlacement {
  x: number;
  y: number;
  width: number;
  height: number;
  dpiScale: number;
  focused: boolean;
}

export interface PresentationIntent {
  mode: PresentationMode;
  visible: boolean;
  placement: PresentationPlacement | null;
}

export interface RenderOperationProgress {
  phase: string;
  activity: string | null;
  completed: number | null;
  total: number | null;
  elapsedMs: number;
}

export interface IntegerVector {
  x: number;
  y: number;
  z: number;
}

export interface ProjectMetadata {
  name: string;
  author: string;
  description: string;
  time_created: number;
  time_modified: number;
  total_blocks: number;
  total_volume: number;
  region_count: number;
  enclosing_size: IntegerVector;
  litematic_version: number;
  litematic_subversion: number;
  minecraft_data_version: number;
}

export interface AnalysisScope {
  kind: string;
  region: string | null;
  layer: number | null;
  world_y: number | null;
}

export interface StructureStatsSummary {
  scope: AnalysisScope;
  total_non_air_blocks: number;
  unique_block_types: number;
  total_regions: number;
  total_layers: number;
  total_entities: number;
  total_tile_entities: number;
}

export interface RegionCountSummary {
  region: string;
  count: number;
}

export interface LayerCountSummary {
  layer: number;
  world_y: number;
  count: number;
}

export interface MaterialAnalysisItem {
  block_id: string;
  display_name: string;
  count: number;
  block_count: number;
  container_item_count: number;
  total_count: number;
  icon_hint: string;
  region_counts: RegionCountSummary[] | null;
  layer_counts: LayerCountSummary[] | null;
}

export interface RegionBounds {
  min_x: number;
  max_x: number;
  min_y: number;
  max_y: number;
  min_z: number;
  max_z: number;
}

export interface RegionAnalysisSummary {
  name: string;
  position: IntegerVector;
  size: IntegerVector;
  bounds: RegionBounds;
  total_non_air_blocks: number;
  unique_block_types: number;
  total_entities: number;
  total_tile_entities: number;
}

export interface LayerAnalysisSummary {
  layer: number;
  world_y: number;
  total_non_air_blocks: number;
  unique_block_types: number;
}

export interface EntityAnalysisItem {
  entity_id: string;
  count: number;
}

export interface EntityAnalysisSummary {
  total_entities: number;
  items: EntityAnalysisItem[];
}

export interface ContainerScanAnalysisSummary {
  enabled: boolean;
  containers_scanned: number;
  item_stacks_scanned: number;
  warnings: string[];
}

export interface ProjectAnalysis {
  material_semantics?: string;
  non_item_blocks?: Array<{ block_id:string; display_name:string; count:number; reason:string }>;
  metadata: ProjectMetadata;
  structure_stats: StructureStatsSummary;
  material_items: MaterialAnalysisItem[];
  structural_block_counts?: Array<{ block_id: string; count: number; scope_counts?: Array<{ region: string; layer: number; count: number }> }>;
  regions: RegionAnalysisSummary[];
  layers_summary: LayerAnalysisSummary[];
  entity_summary: EntityAnalysisSummary;
  container_scan: ContainerScanAnalysisSummary;
  derived?: {
    building?: {
      building_type?: string;
      density?: number;
      redstone_ratio?: number;
      fluid_ratio?: number;
    };
  };
}

/** Product-facing render state. Renderer correlation IDs and process details never cross this boundary. */
export interface RenderOperationView {
  operationId: string;
  source: ProjectSource;
  displayMode: "normal" | "standard" | "fast_experimental" | "full";
  buildMode: "prebuilt" | "realtime";
  state: RenderOperationState;
  progress: RenderOperationProgress;
  presentation: PresentationIntent;
  presentationState: PresentationState;
  analysis: ProjectAnalysis | null;
  error: string | null;
}

export interface ApplicationRenderSnapshot {
  operations: RenderOperationView[];
}

export interface StartApplicationRenderInput {
  sourcePath: string;
  buildMode: "prebuilt" | "realtime";
  displayMode?: "full" | "standard";
  preflightId?: string | null;
  resourceWarningConfirmed?: boolean;
  realtimeDecisionId?: string | null;
  clickStartedAtEpochMs?: number | null;
}

export interface RenderLaunchPlan {
  sourceBytes: number;
  directRealtime: boolean;
  preflight: ResourcePreflightView | null;
}

export function prepareApplicationRenderLaunch(sourcePath: string, preload: boolean, displayMode: "full" | "standard" = "full", viewport?: [number, number]): Promise<RenderLaunchPlan> {
  return invoke("application_render_launch_plan", { sourcePath, preload, displayMode, viewport });
}

export type ResourcePreflightSeverity = "unmeasured" | "normal" | "warning" | "hard" | "invalid";
export type ResourcePreflightAction =
  | "continue_prebuilt"
  | "switch_realtime"
  | "use_basic_vulkan"
  | "cancel";

export interface ResourcePreflightComponents {
  exactGeometryBytes: number;
  drawMetadataBytes: number;
  blockEntityBytes: number;
  textureAndShaderBytes: number;
  renderTargetBytes: number;
  runtimeReserveBytes: number;
}

/** Product-level assessment. Renderer admission tokens and adapter identity never enter the WebView. */
export interface ResourcePreflightView {
  preflightId: string;
  severity: ResourcePreflightSeverity;
  conservativeVramBytes: number | null;
  safeAvailableVramBytes: number | null;
  pressurePercent: number | null;
  components: ResourcePreflightComponents | null;
  invalidReason: string | null;
  allowedActions: ResourcePreflightAction[];
}

export interface PreflightApplicationRenderInput {
  sourcePath: string;
  buildMode: "prebuilt";
}

export type ResourcePreflightDisposition = "start_prebuilt" | "confirm_prebuilt" | "block_prebuilt";

export function resourcePreflightAllows(
  preflight: ResourcePreflightView,
  action: ResourcePreflightAction,
): boolean {
  if (preflight.severity === "invalid" && action !== "switch_realtime" && action !== "cancel") {
    return false;
  }
  return preflight.allowedActions.includes(action);
}

/** Identity/capability admission remains mandatory; memory is informational. */
export function resourcePreflightDisposition(
  preflight: ResourcePreflightView,
): ResourcePreflightDisposition {
  if (
    resourcePreflightAllows(preflight, "continue_prebuilt")
  ) {
    return "start_prebuilt";
  }
  return "block_prebuilt";
}

export const APPLICATION_RENDER_STATE_EVENT = "nova://application/render-state";

interface PresentationCommandQueue {
  tail: Promise<void>;
}

interface RoutePresentationSuspension {
  token: number;
  restoreIntent: PresentationIntent;
}

const presentationQueues = new Map<string, PresentationCommandQueue>();
const requestedPresentationByOperation = new Map<string, PresentationIntent>();
const routePresentationSuspensions = new Map<string, RoutePresentationSuspension>();
let nextRouteSuspensionToken = 1;

const PENDING_RENDER_STATES = new Set<RenderOperationState>([
  "requested",
  "running",
  "build_completed",
  "artifact_ready",
  "viewer_preparing",
  "realtime_ready",
  "gpu_accepted",
  "gpu_resident",
]);

export function isApplicationRenderPending(operation: RenderOperationView | null | undefined): boolean {
  return !!operation && PENDING_RENDER_STATES.has(operation.state);
}

export function isApplicationViewerPrepared(operation: RenderOperationView | null | undefined): boolean {
  // Ready permits presentation; the pending set still tracks background work.
  return operation?.state === "viewer_prepared" || operation?.state === "realtime_ready";
}

export function isApplicationPresentationVisible(operation: RenderOperationView | null | undefined): boolean {
  return operation?.presentationState === "visible";
}

function copyPresentationIntent(intent: PresentationIntent): PresentationIntent {
  return {
    ...intent,
    placement: intent.placement ? { ...intent.placement } : null,
  };
}

/** A route may temporarily hide only an already-visible embedded overlay. */
export function isRouteSuspendablePresentation(intent: PresentationIntent): boolean {
  return (intent.mode === "overlay" || intent.mode === "embedded") && intent.visible;
}

function enqueuePresentationCommand(
  operationId: string,
  intent: PresentationIntent,
): Promise<void> {
  const queue = presentationQueues.get(operationId) ?? { tail: Promise.resolve() };
  presentationQueues.set(operationId, queue);
  const immutableIntent = copyPresentationIntent(intent);
  const command = queue.tail.then(async () => {
    // The event stream is the UI's authoritative state source. Deliberately discard
    // this command's snapshot so an older response cannot replace a newer intent.
    await invoke<RenderOperationView>("application_render_update_presentation", {
      operationId,
      intent: immutableIntent,
    });
  });
  queue.tail = command.catch(() => undefined);
  return command;
}

export function preflightApplicationRender(
  input: PreflightApplicationRenderInput,
): Promise<ResourcePreflightView> {
  return invoke("application_render_preflight", { input });
}

/** Re-estimates the frozen request with basic Vulkan for this operation only. */
export function useBasicVulkanForApplicationRender(
  preflightId: string,
  sourcePath: string,
): Promise<ResourcePreflightView> {
  return invoke("application_render_use_basic_vulkan", { preflightId, sourcePath });
}

export function discardApplicationRenderPreflight(preflightId: string): Promise<void> {
  return invoke("application_render_discard_preflight", { preflightId });
}

export function startApplicationRender(input: StartApplicationRenderInput): Promise<RenderOperationView> {
  return invoke("application_render_start", { input });
}

export function cancelApplicationRender(operationId: string): Promise<void> {
  return invoke("application_render_cancel", { operationId });
}

export function updateApplicationRenderPresentation(
  operationId: string,
  intent: PresentationIntent,
): Promise<void> {
  const requestedIntent = copyPresentationIntent(intent);
  // An explicit presentation command supersedes a pending route restoration.
  routePresentationSuspensions.delete(operationId);
  requestedPresentationByOperation.set(operationId, requestedIntent);
  return enqueuePresentationCommand(operationId, requestedIntent).catch((error) => {
    if (requestedPresentationByOperation.get(operationId) === requestedIntent) {
      requestedPresentationByOperation.delete(operationId);
    }
    throw error;
  });
}

/**
 * Updates overlay geometry only while overlay remains the latest user intent.
 * This prevents a slow geometry read from undoing a newer Popup or Hide command.
 */
export function updateApplicationRenderOverlayPlacement(
  operationId: string,
  placement: PresentationPlacement,
  observedIntent: PresentationIntent,
): Promise<boolean> {
  const desiredIntent = requestedPresentationByOperation.get(operationId) ?? observedIntent;
  if (!isRouteSuspendablePresentation(desiredIntent)) return Promise.resolve(false);
  const nextIntent: PresentationIntent = {
    mode: desiredIntent.mode,
    visible: true,
    placement: { ...placement },
  };
  requestedPresentationByOperation.set(operationId, nextIntent);
  return enqueuePresentationCommand(operationId, nextIntent).then(
    () => true,
    (error) => {
      if (requestedPresentationByOperation.get(operationId) === nextIntent) {
        requestedPresentationByOperation.delete(operationId);
      }
      throw error;
    },
  );
}

/**
 * Route cleanup hides only presentation. The remembered user intent stays visible
 * so a later route mount can restore it without recreating the Viewer runtime.
 */
export function suspendApplicationRenderPresentationForRoute(
  operationId: string,
  observedIntent: PresentationIntent,
): Promise<boolean> {
  const desiredIntent = requestedPresentationByOperation.get(operationId) ?? observedIntent;
  if (!isRouteSuspendablePresentation(desiredIntent)) {
    routePresentationSuspensions.delete(operationId);
    return Promise.resolve(false);
  }

  const suspension: RoutePresentationSuspension = {
    token: nextRouteSuspensionToken++,
    restoreIntent: copyPresentationIntent(desiredIntent),
  };
  routePresentationSuspensions.set(operationId, suspension);
  return enqueuePresentationCommand(operationId, {
    ...desiredIntent,
    visible: false,
  }).then(() => true);
}

/**
 * Restores a route-hidden overlay at its current host rect. Explicit Hide/Popup
 * clears the suspension, so neither one is mistaken for route-owned visibility.
 */
export function restoreApplicationRenderPresentationForRoute(
  operationId: string,
  placement: PresentationPlacement,
): Promise<boolean> {
  const suspension = routePresentationSuspensions.get(operationId);
  if (!suspension) return Promise.resolve(false);

  const restoreIntent: PresentationIntent = {
    ...suspension.restoreIntent,
    visible: true,
    placement: { ...placement },
  };
  requestedPresentationByOperation.set(operationId, restoreIntent);
  return enqueuePresentationCommand(operationId, restoreIntent).then(
    () => {
      if (routePresentationSuspensions.get(operationId)?.token === suspension.token) {
        routePresentationSuspensions.delete(operationId);
      }
      return true;
    },
    (error) => {
      if (requestedPresentationByOperation.get(operationId) === restoreIntent) {
        requestedPresentationByOperation.delete(operationId);
      }
      throw error;
    },
  );
}

export function getApplicationRenderSnapshot(): Promise<ApplicationRenderSnapshot> {
  return invoke("application_render_get_snapshot");
}

export function subscribeApplicationRenderState(
  listener: (snapshot: ApplicationRenderSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<ApplicationRenderSnapshot>(APPLICATION_RENDER_STATE_EVENT, ({ payload }) => listener(payload));
}
