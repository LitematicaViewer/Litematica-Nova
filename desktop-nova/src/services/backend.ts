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
export type {
  BuiltinIconExtractOutput,
  CopyFileToDirectoryOutput,
  DirectoryEntryInfo,
  PathInfo,
  ProjectionPreviewImageOutput,
  UserConfig,
  UserConfigInfo,
  WikiEnumCatalogDownloadOutput,
} from "../platform";

export {
  checkFileExists,
  copyFileToDirectory,
  downloadVaultBlockIconsFromVault,
  downloadVaultItemIconsFromVault,
  ensureBuiltinBlockIconsExtracted as ensureBuiltinBlockIconsExtractedNative,
  ensureBuiltinItemIconsExtracted as ensureBuiltinItemIconsExtractedNative,
  downloadWikiEnumCatalogsFromMinecraftWiki,
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
} from "../platform/files";

export {
  executeBackend,
  executeBackendTrace,
  executeCoreBackend,
  executeCoreBackendTrace,
  executeNativeViewerBackend,
  killCacheBuildTask,
  pollCacheBuildTask,
  startCacheBuildTask,
  CORE_BACKEND_EXE,
  CORE_BACKEND_RELATIVE_PATH,
  NATIVE_VIEWER_EXE,
  NATIVE_VIEWER_RELATIVE_PATH,
} from "../platform/backendProcess";
export type { BackendTrace, CacheBuildLaunch, CacheBuildSnapshot } from "../platform/backendProcess";

export {
  openLocalLibraryFoldersWindow,
  openAssetManagerWindow,
  openEnumeratorWindow,
  openMaterialListWindow,
  openRedenLibraryWindow,
  openUiDemoWindow,
  startNativeViewer,
} from "../platform/nativeViewer";
export { generatePreviewImage, renderPreviewImage } from "../platform/previewImage";
export type { RenderPreviewOutput } from "../platform/previewImage";

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
