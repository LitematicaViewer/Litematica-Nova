import { open as openShell } from "@tauri-apps/plugin-shell";

import { invoke } from "./tauri";

export function startNativeViewer(filePath: string, displayMode = "full", cacheInput?: string): Promise<void> {
  return invoke("start_native_viewer", { filePath, displayMode, cacheInput: cacheInput || null });
}

export function openMaterialListWindow(activeFile?: string | null): Promise<void> {
  return invoke("open_material_list_window", { activeFile: activeFile || null });
}

export function openPath(path: string): Promise<void> {
  return openShell(path);
}
