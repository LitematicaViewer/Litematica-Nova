import { invoke } from "./tauri";

export interface EmbeddedRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface EmbeddedViewerStatus {
  supported: boolean;
  running: boolean;
  visible: boolean;
  parent_hwnd?: string | null;
  child_hwnd?: string | null;
  file?: string | null;
  mode?: string | null;
  rect_physical?: EmbeddedRect | null;
  process_id?: number | null;
  status: string;
  error?: string | null;
  stdout_tail?: string;
  stderr_tail?: string;
}

export type EmbeddedViewerPurpose = "properties_preview" | "render_interactive";

export function startEmbeddedViewer(
  filePath: string,
  displayMode: string,
  rect: EmbeddedRect,
  purpose: EmbeddedViewerPurpose,
  cacheInput?: string,
): Promise<EmbeddedViewerStatus> {
  return invoke("start_embedded_viewer", { filePath, displayMode, rect, purpose, cacheInput: cacheInput || null });
}

export function updateEmbeddedViewerBounds(rect: EmbeddedRect): Promise<EmbeddedViewerStatus> {
  return invoke("update_embedded_viewer_bounds", { rect });
}

export function stopEmbeddedViewer(): Promise<EmbeddedViewerStatus> {
  return invoke("stop_embedded_viewer");
}

export function hideEmbeddedViewer(): Promise<EmbeddedViewerStatus> {
  return invoke("hide_embedded_viewer");
}

export function showEmbeddedViewer(): Promise<EmbeddedViewerStatus> {
  return invoke("show_embedded_viewer");
}

export function getEmbeddedViewerStatus(): Promise<EmbeddedViewerStatus> {
  return invoke("get_embedded_viewer_status");
}
