import { invoke } from "../platform/tauri";

export {
  chooseUserConfigDir,
  cleanupLocalTempFiles,
  getPathInfo,
  getUserConfig,
  getWorkspaceRoot,
  openFileParentDir,
  openUserConfigDir,
  openWorkspacePath,
  resetUserConfigDir,
  saveUserConfig,
  setUserConfigDir,
} from "../platform";
export type { CopyFileToDirectoryOutput, DirectoryEntryInfo, PathInfo, ProjectionPreviewImageOutput, UserConfig, UserConfigInfo } from "../platform";
export type { BuiltinIconExtractOutput, VaultBlockIconDownloadOutput, WikiEnumCatalogDownloadOutput } from "../platform/files";

export function executeBackend(binaryName: string, args: string[]): Promise<string> {
  return invoke("execute_backend", { binaryName, args });
}

/**
 * Runs the isolated public core helper (`litematica_core`) and returns its stdout.
 */
export function executeCoreBackend(args: string[]): Promise<string> {
  return executeBackend("litematica_core", args);
}

/**
 * Runs the isolated public fast-preview helper and returns its stdout.
 */
export function executeNativeViewerBackend(args: string[]): Promise<string> {
  return executeBackend("litematica_native_viewer", args);
}

export {
  checkFileExists,
  copyFileToDirectory,
  listDirectoryEntries,
  listLitematicFileEntriesInDirectory,
  getAppDataFilePath as getUserConfigFilePath,
  listLitematicFilesInDirectory,
  readAppDataFile as readUserConfigFile,
  readImageBase64,
  readProjectionPreviewImage,
  readWorkspaceFile,
  writeAppDataFile as writeUserConfigFile,
  writeTextFileAbsolute,
  writeWorkspaceFile,
  // Game-resource manager channels (L1). Builtin extraction works offline; the
  // remote download sources currently reject with an explicit message.
  deleteAppDataFile as deleteUserConfigFile,
  ensureBuiltinBlockIconsExtracted as ensureBuiltinBlockIconsExtractedNative,
  ensureBuiltinItemIconsExtracted as ensureBuiltinItemIconsExtractedNative,
  downloadVaultBlockIconsFromVault,
  downloadVaultItemIconsFromVault,
  downloadWikiBlockIconsFromMinecraftWiki,
  downloadWikiEnumCatalogsFromMinecraftWiki,
  pickLitematicFile,
} from "../platform/files";

export {
  openAssetManagerWindow,
  openLocalLibraryFoldersWindow,
  openMaterialListWindow,
  openRedenLibraryWindow,
  openUiDemoWindow,
} from "../platform/nativeViewer";
export { generatePreviewImage, renderPreviewImage } from "../platform/previewImage";
export type { ProductPreviewImage } from "../platform/previewImage";

export {
  aiChatCompletion,
  aiClearKey,
  aiGetConfig,
  aiSaveConfig,
  aiTestConnection,
} from "../platform/keyStorage";
export type {
  AiChatCompletionInput,
  AiChatCompletionOutput,
  AiChatMessageWire,
  AiPublicConfig,
  AiSaveConfigInput,
  AiTestResult,
} from "../platform/keyStorage";
