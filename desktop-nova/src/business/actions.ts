import { openDialog, saveDialog } from "../platform/dialogs";
import { pickLitematicFile } from "../platform/files";
import { openPath } from "../platform/nativeViewer";
import {
  CORE_BACKEND_EXE,
  CORE_BACKEND_RELATIVE_PATH,
  NATIVE_VIEWER_EXE,
  NATIVE_VIEWER_RELATIVE_PATH,
  executeCoreBackend,
  executeBackend,
  getPathInfo,
  getUserConfigFilePath,
  getWorkspaceRoot,
  startNativeViewer,
  writeUserConfigFile,
  type PathInfo,
} from "../services/backend";
import { sanitizeFileName } from "../services/generateService";

const BLOCKSTATE_DB_PATH = "data/minecraft_blockstates/26.1.json";
const BLOCKSTATE_ZH_PATH = "data/minecraft_blockstates/26.1.zh_cn.json";
const BLOCK_ICON_DIR_PATH = "block";

export interface RuntimePathInfo {
  workspaceRoot: string;
  coreInfo: PathInfo;
  viewerInfo: PathInfo;
  dbInfo: PathInfo;
  zhInfo: PathInfo;
  iconDirInfo: PathInfo;
}

export interface BackendHealthResult {
  coreInfo: PathInfo;
  viewerInfo: PathInfo;
  coreHelp: string;
  viewerHelp: string;
  ok: boolean;
}

export interface MetadataPatchInput {
  currentFile: string;
  patch: unknown;
  outputPath?: string;
}

export async function selectLitematicFile(): Promise<string | null> {
  return pickLitematicFile();
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

export async function selectPreviewImageFile(): Promise<string | null> {
  const selected = await openDialog({
    filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "webp", "bmp"] }],
  });
  return typeof selected === "string" ? selected : null;
}

export async function clearProjectionPreviewImage(currentFile: string): Promise<string> {
  return saveProjectionMetadataPatch({ currentFile, patch: { preview_image_data: [] } });
}

export async function importProjectionPreviewImage(currentFile: string, imagePath: string): Promise<string> {
  return saveProjectionMetadataPatch({ currentFile, patch: { preview_image_path: imagePath } });
}

export async function openExternalPath(path: string): Promise<void> {
  await openPath(path);
}

export async function openProjectionViewer(filePath: string, displayMode = "full", cacheInput?: string): Promise<void> {
  await startNativeViewer(filePath, displayMode, cacheInput);
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
  return JSON.parse(await executeCoreBackend(["analyze", filePath]));
}

export async function saveProjectionMetadataPatch({ currentFile, patch, outputPath }: MetadataPatchInput): Promise<string> {
  const relative = `render/metadata_patch_${Date.now()}.json`;
  const patchPath = await getUserConfigFilePath(relative);
  await writeUserConfigFile(relative, JSON.stringify(patch, null, 2));
  const args = ["edit-metadata", "--input", currentFile, "--patch", patchPath];
  if (outputPath) args.push("--output", outputPath);
  return executeCoreBackend(args);
}

export async function dryRunReplaceBlocks(currentFile: string, rules: any[]): Promise<string> {
  const cleanRules = rules.map((rule) => {
    const next = JSON.parse(JSON.stringify(rule));
    if (Object.keys(next.match.properties || {}).length === 0) delete next.match.properties;
    return next;
  });
  const rulesRelativePath = "render/replace_rules_js.json";
  const rulesPath = await getUserConfigFilePath(rulesRelativePath);
  await writeUserConfigFile(rulesRelativePath, JSON.stringify({ rules: cleanRules }));
  return executeCoreBackend(["replace-blocks", "--input", currentFile, "--rules", rulesPath, "--dry-run"]);
}

export async function applyReplaceBlocks(currentFile: string, outputPath: string): Promise<string> {
  const rulesPath = await getUserConfigFilePath("render/replace_rules_js.json");
  return executeCoreBackend(["replace-blocks", "--input", currentFile, "--output", outputPath, "--rules", rulesPath]);
}

export async function buildLayerMetaCache(cacheFile: string): Promise<string> {
  return executeCoreBackend(["cache-layer-meta", cacheFile]);
}

export async function getNovaRuntimePathInfo(): Promise<RuntimePathInfo> {
  const [workspaceRoot, coreInfo, viewerInfo, dbInfo, zhInfo, iconDirInfo] = await Promise.all([
    getWorkspaceRoot(),
    getPathInfo(CORE_BACKEND_RELATIVE_PATH),
    getPathInfo(NATIVE_VIEWER_RELATIVE_PATH),
    getPathInfo(BLOCKSTATE_DB_PATH),
    getPathInfo(BLOCKSTATE_ZH_PATH),
    getPathInfo(BLOCK_ICON_DIR_PATH),
  ]);
  return { workspaceRoot, coreInfo, viewerInfo, dbInfo, zhInfo, iconDirInfo };
}

export async function checkBackendHealth(): Promise<BackendHealthResult> {
  const [coreInfo, viewerInfo] = await Promise.all([
    getPathInfo(CORE_BACKEND_RELATIVE_PATH),
    getPathInfo(NATIVE_VIEWER_RELATIVE_PATH),
  ]);
  const coreHelp = await executeBackend(CORE_BACKEND_EXE, ["--help"]).catch((err) => String(err));
  const viewerHelp = await executeBackend(NATIVE_VIEWER_EXE, ["--help"]).catch((err) => String(err));
  const ok = coreInfo.exists && coreInfo.is_file && viewerInfo.exists && viewerInfo.is_file && coreHelp.length > 0 && viewerHelp.length > 0;
  return { coreInfo, viewerInfo, coreHelp, viewerHelp, ok };
}
