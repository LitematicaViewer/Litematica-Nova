export type { LibraryCategory, LibraryCategoryIcon, LibraryState, LibraryTrashEntry, LocalLibraryFolder, ProjectionRecord, SyncLibraryFoldersResult } from "../../services/libraryStore";
export { minecraftVersionLabel, minecraftVersions } from "../../services/minecraftVersions";
export {
  categoryDescendants,
  categoryPath,
  createLibraryCategory,
  deleteLibraryCategory,
  trashLibraryRecords,
  restoreLibraryTrash,
  discardLibraryTrash,
  moveLibraryRecordsToCategory,
  normalizeLibraryState,
  projectionRecordAnalysis,
  updateLibraryCategory,
  addLocalLibraryFolder,
  addOrUpdateRecord,
  addOrUpdateRecordFromAnalysis,
  analyzeFile,
  loadLibrary,
  removeLocalLibraryFolder,
  reorderLibraryRecords,
  saveLibrary,
  setRecordPreview,
  syncAllLocalLibraryFolders,
  syncLocalLibraryFolder,
  updateLocalLibraryFolder,
} from "../../services/libraryStore";
export { LIBRARY_ICON_PRESETS, LIBRARY_LUCIDE_ICON_NAMES, sanitizeLibrarySvg, validateLibraryIconDataUrl } from "../../services/libraryCategoryIcons";
export { windowsPathKey, displayFilePath } from "../../services/filePathIdentity";
export { queryBlockDatabase, convertProjectVersion, type BlockDatabaseVersion, type BlockDatabaseBlock, type BlockDatabaseState, type BlockDatabaseResult, type ConversionPreview } from "../../platform/applicationOperations";
