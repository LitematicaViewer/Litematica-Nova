import { invoke } from "./tauri";

export interface LayerViewMeta {
  size_x: number; size_y: number; size_z: number;
  min_x: number; min_y: number; min_z: number;
  minecraft_data_version: number; version: string; name: string;
}
export interface LayerScene {
  sceneHandle: string; generation: number; sourceIdentity: string; resourceIdentity: string;
  visualGeneration: number;
  meta: LayerViewMeta; origin: { x: number; y: number; z: number } | null;
}
export interface LayerViewRect { x: number; z: number; width: number; height: number }
export interface LayerVisualIssue {
  state: string; kind: string; resourceKey: string | null; message: string; retryable: boolean; blockCount: number;
}
export interface LayerVisualDiagnostics {
  readyStates: number; transparentStates: number; missingStates: number; failedStates: number; retryableStates: number;
  issues: LayerVisualIssue[];
}
export interface LayerViewImage {
  sceneHandle: string; generation: number; requestId: number; y: number; worldY: number;
  viewRect: LayerViewRect; imageUrl: string; pixelWidth: number; pixelHeight: number;
  blockCount: number; missingVisuals: number;
  resourceIdentity: string; visualGeneration: number; visualDiagnostics: LayerVisualDiagnostics;
}
export interface LayerHit {
  x: number; y: number; z: number; worldX: number; worldY: number; worldZ: number;
  paletteId: number; id: string; name: string; states: string;
  stateRecord: Record<string, string>; originalStateRecord: Record<string, string>;
  hasStates: boolean; minecraftVersion: string; region: string; propertyOptions: Record<string, string[]>;
}
export interface LayerViewRequest {
  requestId: number; y: number; onionDepth: number; viewRect: LayerViewRect; scale: number; stateHints: boolean;
  overrides: Record<string, Record<string, string>>;
}
let operationSequence = 0;
let requestSequence = 0;
const operationEpoch = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
const query = <T>(input: object): Promise<T> => invoke("application_data_query", {
  input: { operationId: `layer-${operationEpoch}-${++operationSequence}`, kind: "layerView", input },
});
const identity = (scene: LayerScene) => ({ sceneHandle: scene.sceneHandle, generation: scene.generation });
export const nextLayerRequestId = () => ++requestSequence;
export const openLayerScene = (sourcePath: string): Promise<LayerScene> => query({ action: "openScene", sourcePath });
export const getLayerMeta = (scene: LayerScene): Promise<LayerScene> => query({ action: "getLayerMeta", ...identity(scene) });
export const requestLayerView = (scene: LayerScene, request: LayerViewRequest): Promise<LayerViewImage> => query({ action: "requestLayerView", ...identity(scene), ...request });
export const hitTestLayer = (scene: LayerScene, request: Pick<LayerViewRequest, "requestId" | "y" | "onionDepth" | "overrides"> & { uiLocale?: string }, x: number, z: number): Promise<{ sceneHandle: string; generation: number; requestId: number; hit: LayerHit | null }> => query({ action: "hitTest", ...identity(scene), ...request, x, z });
export const cancelLayerView = (scene: LayerScene, requestId: number): Promise<unknown> => query({ action: "cancel", ...identity(scene), requestId });
export const invalidateLayerVisuals = (scene: LayerScene): Promise<{ sceneHandle: string; generation: number; resourceIdentity: string; visualGeneration: number }> => query({ action: "invalidateVisuals", ...identity(scene) });
export const releaseLayerScene = (scene: LayerScene): Promise<unknown> => query({ action: "release", ...identity(scene) });
