import { invoke } from "./tauri";
import { getDataOperationStatus } from "./applicationOperations";
export interface ProductPreviewImage {
  preview_path: string; data_url: string; draft_id?: string | null; source_identity?: string | null;
  display_mode?: string | null; can_embed: boolean; persisted: boolean;
}
export interface EmbeddedLibraryPreview { preview_path: string; data_url: string; width: number; height: number; source_identity: string; origin: "embedded" }
export interface PreviewReadResult { sourcePath: string; image: EmbeddedLibraryPreview | null; error?: string }
export interface PreviewBatchItem { file_path: string; image: ProductPreviewImage | null; error: string | null }
export interface PreviewBatchOutput { items: PreviewBatchItem[]; cancelled: boolean }
export const readVisiblePreviews = (sourcePaths: string[], operationId: string): Promise<PreviewReadResult[]> => invoke("application_data_query", { input: { operationId, kind: "thumbnail", input: { action: "readEmbeddedBatch", sourcePaths } } });
export const cancelPreviewRead = (operationId: string) => getDataOperationStatus(operationId, true);
export const renderPreviewImage = (filePath: string, displayMode: string, cacheInput?: string, operationId?: string): Promise<ProductPreviewImage> => invoke("render_preview_image", { filePath, displayMode, cacheInput: cacheInput || null, operationId: operationId || null });
export const generatePreviewImage = (filePath: string, displayMode?: string, requestId?: string): Promise<ProductPreviewImage> => invoke("generate_preview_image", { filePath, displayMode: displayMode || null, requestId: requestId || null });
export const generatePreviewImages = (filePaths: string[], requestId: string, displayMode?: string): Promise<PreviewBatchOutput> => invoke("generate_preview_images", { filePaths, requestId, displayMode: displayMode || null });
export const saveGeneratedPreview = (filePath: string, draftId: string): Promise<ProductPreviewImage> => invoke("save_generated_preview", { filePath, draftId });
export const discardGeneratedPreview = (draftId: string): Promise<void> => invoke("discard_generated_preview", { draftId });
export const cancelPreviewGeneration = (requestId: string, action: "pause" | "resume" | "cancel" = "cancel"): Promise<void> => invoke("cancel_preview_generation", { requestId, action });
