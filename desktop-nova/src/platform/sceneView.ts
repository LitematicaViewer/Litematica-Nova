import { invoke } from "./tauri";

export interface SceneUiRegion { x: number; y: number; width: number; height: number; }
export interface SceneUiPointer { kind: "down" | "up" | "move" | "wheel"; x: number; y: number; button?: number; delta?: number; }
export interface SceneBlockState { id: string; properties: Record<string, string>; }
export interface SceneInteractionControls {
  mode?: "observe" | "edit"; menuOpen?: boolean; activeSlot?: number;
  hotbar?: (SceneBlockState | null)[];
  setSlot?: { index: number; state: SceneBlockState | null };
  command?: { kind: "break" | "place" | "pick" | "undo" | "redo" | "save"; requestId: string; sceneGeneration: string; expectedRevision: number; savePath?: string; };
}
export interface SceneInteractionSnapshot {
  cameraInputMode?: "drag" | "pointer_lock";
  detailsRequest?: { sequence: number; hit: NonNullable<SceneInteractionSnapshot["hit"]> } | null;
  pointer?: [number, number] | null; pointerLocked?: boolean;
  dataVersion: number | null; gameVersion: string | null; contentId: string | null;
  sceneGeneration: string; revision: number; mode: "observe" | "edit"; menuOpen: boolean;
  activeSlot: number; hotbar: (SceneBlockState | null)[]; dirty: boolean; canUndo: boolean; canRedo: boolean;
  hit: { displayName?: string | null; sourceName?: string | null; instanceId: string; position: [number, number, number]; face: string; distance: number; state: SceneBlockState;
    container: { status: "known" | "unknown"; capacity: number; items: { displayName?: string | null; slot: number; id: string; count: number; variant: string | null }[]; error: string | null } | null;
  } | null; error: string | null;
}
export interface SceneViewControls {
  uiInputBlocked?: boolean;
  uiBackdropOpacity?: number;
  uiRegions?: SceneUiRegion[];
  hotbarRegions?: SceneUiRegion[];
  uiPointer?: SceneUiPointer;
  interaction?: SceneInteractionControls;
  compositionEditor?: import("./composition").CompositionEditorScene;
  layerView?: boolean;
  layerMinY?: number | null;
  layerMaxY?: number | null;
  cameraMode?: "orbit" | "free";
  cameraInputMode?: "drag" | "pointer_lock";
  cameraSpeed?: number;
  cameraSensitivity?: number;
  releaseCameraInput?: boolean;
  resetCamera?: boolean;
  reloadShaderOptions?: boolean;
  worldTime?: number;
  weather?: "auto" | "clear" | "rain" | "thunder" | "snow";
}
export interface SceneViewAck {
  interaction?: SceneInteractionSnapshot;
  sourcePath: string;
  displayMode: "fast_experimental" | "standard" | "full";
  worldTime?: number;
  worldTimeLabel?: string;
  weather?: "auto" | "clear" | "rain" | "thunder" | "snow";
  cameraLocked: boolean;
  error?: string;
}
export async function updateSceneView(input: { sourcePath: string; operationId?: string; overlayRevision?: number; displayMode?: "fast_experimental" | "standard" | "full"; controls: SceneViewControls }) {
  const applyLayerRange = Object.prototype.hasOwnProperty.call(input.controls, "layerMinY") || Object.prototype.hasOwnProperty.call(input.controls, "layerMaxY");
  return invoke<SceneViewAck>("application_set_scene_view", { sourcePath: input.sourcePath, displayMode: input.displayMode ?? null,
    operationId: input.operationId ?? null, overlayRevision: input.overlayRevision ?? null,
    controls: { ...input.controls, applyLayerRange } });
}

export function getSceneViewState(input: { sourcePath: string; displayMode?: "fast_experimental" | "standard" | "full" }) {
  return invoke<SceneViewAck>("application_get_scene_view", { sourcePath: input.sourcePath, displayMode: input.displayMode ?? null });
}

export function releaseCameraInput(input: { sourcePath: string; displayMode?: "fast_experimental" | "standard" | "full" }) {
  return updateSceneView({ ...input, controls: { releaseCameraInput: true } });
}
