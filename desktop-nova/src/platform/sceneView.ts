import { invoke } from "./tauri";

export interface SceneViewControls {
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
  sourcePath: string;
  displayMode: "fast_experimental" | "standard" | "full";
  worldTime?: number;
  weather?: "auto" | "clear" | "rain" | "thunder" | "snow";
  cameraLocked: boolean;
  error?: string;
}
export async function updateSceneView(input: { sourcePath: string; displayMode?: "fast_experimental" | "standard" | "full"; controls: SceneViewControls }) {
  const applyLayerRange = Object.prototype.hasOwnProperty.call(input.controls, "layerMinY") || Object.prototype.hasOwnProperty.call(input.controls, "layerMaxY");
  return invoke<SceneViewAck>("application_set_scene_view", { sourcePath: input.sourcePath, displayMode: input.displayMode ?? null,
    controls: { ...input.controls, applyLayerRange } });
}

export function getSceneViewState(input: { sourcePath: string; displayMode?: "fast_experimental" | "standard" | "full" }) {
  return invoke<SceneViewAck>("application_get_scene_view", { sourcePath: input.sourcePath, displayMode: input.displayMode ?? null });
}

export function releaseCameraInput(input: { sourcePath: string; displayMode?: "fast_experimental" | "standard" | "full" }) {
  return updateSceneView({ ...input, controls: { releaseCameraInput: true } });
}
