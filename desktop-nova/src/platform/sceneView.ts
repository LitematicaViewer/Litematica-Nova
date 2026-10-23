import { invoke } from "./tauri";

export interface SceneViewControls {
  layerMinY?: number | null;
  layerMaxY?: number | null;
  cameraMode?: "orbit" | "free";
  cameraInputMode?: "drag" | "pointer_lock";
  cameraSpeed?: number;
  resetCamera?: boolean;
  reloadShaderOptions?: boolean;
}
export async function updateSceneView(input: { sourcePath: string; displayMode?: "fast_experimental" | "standard" | "full"; controls: SceneViewControls }) {
  const applyLayerRange = Object.prototype.hasOwnProperty.call(input.controls, "layerMinY") || Object.prototype.hasOwnProperty.call(input.controls, "layerMaxY");
  return invoke<void>("application_set_scene_view", { sourcePath: input.sourcePath, displayMode: input.displayMode ?? null,
    controls: { ...input.controls, applyLayerRange } });
}
