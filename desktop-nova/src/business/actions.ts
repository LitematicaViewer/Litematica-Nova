import { openDialog, saveDialog } from "../platform/dialogs";
import { openPath, openExternalUrl as openUrl } from "../platform/nativeViewer";
import {
  getPathInfo,
  getWorkspaceRoot,
  type PathInfo,
} from "../services/backend";
import {
  editProjectMetadata,
  getApplicationRuntimeHealth,
  inspectProject,
  replaceProjectBlocks,
  type ApplicationRuntimeHealth,
} from "../platform/applicationOperations";
import { sanitizeFileName } from "../services/generateService";
import { exportSessionLogs, type LogExportResult } from "../platform/files";

const BLOCKSTATE_DB_PATH = "data/minecraft_blockstates/26.1.json";
const BLOCKSTATE_ZH_PATH = "data/minecraft_blockstates/26.1.zh_cn.json";
const BLOCK_ICON_DIR_PATH = "block";

export interface RuntimePathInfo {
  workspaceRoot: string;
  dbInfo: PathInfo;
  zhInfo: PathInfo;
  iconDirInfo: PathInfo;
}

export type ApplicationHealthResult = ApplicationRuntimeHealth;

export interface MetadataPatchInput {
  currentFile: string;
  patch: unknown;
  outputPath?: string;
}

export async function selectLitematicFile(): Promise<string | null> {
  const selected = await openDialog({ filters: [{ name: "Litematic", extensions: ["litematic"] }] });
  return typeof selected === "string" ? selected : null;
}

export async function selectLitematicSavePath(defaultPath: string): Promise<string | null> {
  const selected = await saveDialog({
    defaultPath,
    filters: [{ name: "Litematic", extensions: ["litematic"] }],
  });
  return typeof selected === "string" ? selected : null;
}

export async function selectJsonSavePath(defaultPath: string): Promise<string | null> {
  const selected = await saveDialog({
    defaultPath,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  return typeof selected === "string" ? selected : null;
}

export async function exportDiagnosticLogs(): Promise<LogExportResult | null> {
  const selected = await saveDialog({
    title: "导出日志 ZIP…",
    defaultPath: `Litematica-Nova-日志-${new Date().toISOString().slice(0, 10)}.zip`,
    filters: [{ name: "日志 ZIP", extensions: ["zip"] }],
  });
  if (!selected) return null;
  return exportSessionLogs(/\.zip$/i.test(selected) ? selected : `${selected}.zip`);
}

export async function openExternalUrl(url: string): Promise<void> {
  await openUrl(url);
}

export async function openExternalPath(path: string): Promise<void> {
  await openPath(path);
}

export async function getDefaultProjectionOutputPath(currentFile: string | undefined, projectionName: string): Promise<string> {
  const fileName = `${sanitizeFileName(projectionName || "Generated Projection")}.litematic`;
  if (currentFile) {
    const index = Math.max(currentFile.lastIndexOf("\\"), currentFile.lastIndexOf("/"));
    if (index > 0) return `${currentFile.slice(0, index + 1)}${fileName}`;
  }
  const root = await getWorkspaceRoot();
  return `${root}\\${fileName}`;
}

export async function analyzeProjectionFile(filePath: string): Promise<any> {
  return inspectProject({ sourcePath: filePath });
}

export async function saveProjectionMetadataPatch({ currentFile, patch, outputPath }: MetadataPatchInput): Promise<string> {
  const result = await editProjectMetadata({
    sourcePath: currentFile,
    patch,
    outputPath: outputPath || null,
  });
  return result.message;
}

export async function selectPreviewImageFile(): Promise<string | null> {
  const selected = await openDialog({ filters: [{ name: "预览图片", extensions: ["png", "jpg", "jpeg", "webp", "bmp"] }] });
  return typeof selected === "string" ? selected : null;
}

export function importProjectionPreviewImage(currentFile: string, imagePath: string): Promise<string> {
  return saveProjectionMetadataPatch({ currentFile, patch: { preview_image_path: imagePath } });
}

export function clearProjectionPreviewImage(currentFile: string): Promise<string> {
  return saveProjectionMetadataPatch({ currentFile, patch: { preview_image_data: [] } });
}

export async function dryRunReplaceBlocks(currentFile: string, rules: any[]): Promise<string> {
  const cleanRules = rules.map((rule) => {
    const next = JSON.parse(JSON.stringify(rule));
    if (Object.keys(next.match.properties || {}).length === 0) delete next.match.properties;
    return next;
  });
  const result = await replaceProjectBlocks({
    sourcePath: currentFile,
    outputPath: null,
    rules: cleanRules,
    dryRun: true,
  });
  return result.message;
}

export async function applyReplaceBlocks(currentFile: string, outputPath: string, rules: any[]): Promise<string> {
  const cleanRules = rules.map((rule) => {
    const next = JSON.parse(JSON.stringify(rule));
    if (Object.keys(next.match.properties || {}).length === 0) delete next.match.properties;
    return next;
  });
  const result = await replaceProjectBlocks({
    sourcePath: currentFile,
    outputPath,
    rules: cleanRules,
    dryRun: false,
  });
  return result.message;
}

export async function getNovaRuntimePathInfo(): Promise<RuntimePathInfo> {
  const [workspaceRoot, dbInfo, zhInfo, iconDirInfo] = await Promise.all([
    getWorkspaceRoot(),
    getPathInfo(BLOCKSTATE_DB_PATH),
    getPathInfo(BLOCKSTATE_ZH_PATH),
    getPathInfo(BLOCK_ICON_DIR_PATH),
  ]);
  return { workspaceRoot, dbInfo, zhInfo, iconDirInfo };
}

export async function checkApplicationRuntimeHealth(): Promise<ApplicationHealthResult> {
  return getApplicationRuntimeHealth();
}
