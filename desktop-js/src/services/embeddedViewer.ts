import { invoke } from "@tauri-apps/api/core";

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

export function elementToPhysicalRect(element: HTMLElement): EmbeddedRect {
  const rect = element.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  return {
    x: Math.round(rect.left * dpr),
    y: Math.round(rect.top * dpr),
    width: Math.round(rect.width * dpr),
    height: Math.round(rect.height * dpr),
  };
}

export function elementToCssRect(element: HTMLElement): EmbeddedRect {
  const rect = element.getBoundingClientRect();
  return {
    x: Math.round(rect.left),
    y: Math.round(rect.top),
    width: Math.round(rect.width),
    height: Math.round(rect.height),
  };
}

export function isUsableEmbeddedRect(rect: EmbeddedRect): boolean {
  return rect.width > 8 && rect.height > 8;
}

export async function startEmbeddedViewer(
  filePath: string,
  displayMode: string,
  rect: EmbeddedRect,
  purpose: EmbeddedViewerPurpose,
): Promise<EmbeddedViewerStatus> {
  return await invoke("start_embedded_viewer", { filePath, displayMode, rect, purpose });
}

export async function updateEmbeddedViewerBounds(rect: EmbeddedRect): Promise<EmbeddedViewerStatus> {
  return await invoke("update_embedded_viewer_bounds", { rect });
}

export async function stopEmbeddedViewer(): Promise<EmbeddedViewerStatus> {
  return await invoke("stop_embedded_viewer");
}

export async function hideEmbeddedViewer(): Promise<EmbeddedViewerStatus> {
  return await invoke("hide_embedded_viewer");
}

export async function showEmbeddedViewer(): Promise<EmbeddedViewerStatus> {
  return await invoke("show_embedded_viewer");
}

export async function getEmbeddedViewerStatus(): Promise<EmbeddedViewerStatus> {
  return await invoke("get_embedded_viewer_status");
}
