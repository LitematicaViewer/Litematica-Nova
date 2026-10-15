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
export type { CopyFileToDirectoryOutput, DirectoryEntryInfo, PathInfo, ProjectionPreviewImageOutput, UserConfig, UserConfigInfo, V5CompatibilityConfig, ViewerControlConfig } from "../platform";

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
  deleteAppDataFile as deleteUserConfigFile,
  ensureBuiltinBlockIconsExtracted as ensureBuiltinBlockIconsExtractedNative,
  ensureBuiltinItemIconsExtracted as ensureBuiltinItemIconsExtractedNative,
  downloadVaultBlockIconsFromVault,
  downloadVaultItemIconsFromVault,
  downloadWikiBlockIconsFromMinecraftWiki,
  downloadWikiEnumCatalogsFromMinecraftWiki,
  pickLitematicFile,
  readGameResourceRemoteText,
} from "../platform/files";

export {
  openAssetManagerWindow,
  openEnumeratorWindow,
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
