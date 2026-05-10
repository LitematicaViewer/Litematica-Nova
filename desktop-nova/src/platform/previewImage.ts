import { invoke } from "./tauri";

export interface RenderPreviewOutput {
  preview_path: string;
  data_url: string;
  stdout: string;
  stderr: string;
  exit_code: number | null;
}

export function renderPreviewImage(filePath: string, displayMode: string): Promise<RenderPreviewOutput> {
  return invoke("render_preview_image", { filePath, displayMode });
}

export function generatePreviewImage(filePath: string, displayMode: string): Promise<RenderPreviewOutput> {
  return invoke("generate_preview_image", { filePath, displayMode });
}
