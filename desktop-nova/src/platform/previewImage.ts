import { invoke } from "./tauri";

export interface ProductPreviewImage {
  preview_path: string;
  data_url: string;
}

export async function renderPreviewImage(filePath: string, displayMode: string, cacheInput?: string, operationId?: string): Promise<ProductPreviewImage> {
  const output = await invoke<ProductPreviewImage>("render_preview_image", { filePath, displayMode, cacheInput: cacheInput || null, operationId: operationId || null });
  return { preview_path: output.preview_path, data_url: output.data_url };
}

export async function generatePreviewImage(filePath: string, displayMode: string): Promise<ProductPreviewImage> {
  const output = await invoke<ProductPreviewImage>("generate_preview_image", { filePath, displayMode });
  return { preview_path: output.preview_path, data_url: output.data_url };
}
