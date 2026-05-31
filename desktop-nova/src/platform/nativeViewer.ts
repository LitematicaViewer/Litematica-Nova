import { open as openShell } from "@tauri-apps/plugin-shell";

import { invoke } from "./tauri";

export function startNativeViewer(filePath: string, displayMode = "full", cacheInput?: string): Promise<void> {
  return invoke("start_native_viewer", { filePath, displayMode, cacheInput: cacheInput || null });
}

/**
 * Opens or focuses the desktop material-list window for the active projection.
 */
export function openMaterialListWindow(activeFile?: string | null): Promise<void> {
  return invoke("open_material_list_window", { activeFile: activeFile || null });
}

/**
 * Opens or focuses the desktop enumerator window for the active projection.
 */
export function openEnumeratorWindow(activeFile?: string | null): Promise<void> {
  return invoke("open_enumerator_window", { activeFile: activeFile || null });
}

/**
 * Opens or focuses the desktop RedenMC online-library window.
 */
export function openRedenLibraryWindow(): Promise<void> {
  return invoke("open_reden_library_window");
}

/**
 * Opens or focuses the desktop local-library-folder manager window.
 */
export function openLocalLibraryFoldersWindow(): Promise<void> {
  return invoke("open_local_library_folders_window");
}

/**
 * Opens or focuses an empty desktop demo window for child-window style checks.
 */
export function openUiDemoWindow(): Promise<void> {
  return invoke("open_ui_demo_window");
}

/**
 * Opens or focuses the desktop game asset manager window.
 */
export function openAssetManagerWindow(): Promise<void> {
  return invoke("open_asset_manager_window");
}

export function openPath(path: string): Promise<void> {
  return openShell(path);
}
