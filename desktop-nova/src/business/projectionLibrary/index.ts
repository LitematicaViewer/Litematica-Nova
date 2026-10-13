export type { LibraryCategory, LibraryCategoryIcon, LibraryState, LocalLibraryFolder, ProjectionRecord, SyncLibraryFoldersResult } from "../../services/libraryStore";
export {
  categoryDescendants,
  categoryPath,
  createLibraryCategory,
  deleteLibraryCategory,
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
export { LIBRARY_LUCIDE_ICON_NAMES, sanitizeLibrarySvg, validateLibraryIconDataUrl } from "../../services/libraryCategoryIcons";
