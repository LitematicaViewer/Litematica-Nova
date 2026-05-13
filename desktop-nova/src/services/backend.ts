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
export type { PathInfo, ProjectionPreviewImageOutput, UserConfig, UserConfigInfo } from "../platform";

export {
  checkFileExists,
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