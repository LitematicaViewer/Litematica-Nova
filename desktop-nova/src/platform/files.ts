import { invoke } from "./tauri";
import { readEmbeddedPreview } from "./applicationOperations";

export interface LogExportResult {
  path: string;
  sessionIds: string[];
  fileCount: number;
  snapshotBytes: number;
}

export function exportSessionLogs(path: string): Promise<LogExportResult> {
  return invoke<LogExportResult>("application_export_logs_zip", { path });
}

export interface PathInfo {
  raw: string;
  normalized: string;
  parent_dir: string;
  parent_exists: boolean;
  exists: boolean;
  is_dir: boolean;
  is_file: boolean;
  is_absolute: boolean;
  has_litematic_ext: boolean;
}

export interface ProjectionPreviewImageOutput {
  width: number;
  height: number;
  data_url: string;
}

export interface CopyFileToDirectoryOutput {
  target_path: string;
  overwritten: boolean;
  bytes_copied: number;
}

export interface DirectoryEntryInfo {
  path: string;
  name: string;
  is_dir: boolean;
  is_file: boolean;
  file_size: number;
  mtime_ms: number;
  extension: string;
}

/** Result of unpacking a bundled icon archive into its app-data icon root. */
export interface BuiltinIconExtractOutput {
  target_dir: string;
  root_relpath: string;
  extracted: number;
}

/** Result of an icon download into an app-data icon root. */
export interface VaultBlockIconDownloadOutput {
  total: number;
  downloaded: number;
  target_dir: string;
  root_relpath: string;
}

/** Result of downloading the Minecraft Wiki enum catalogs. */
export interface WikiEnumCatalogDownloadOutput {
  target_dir: string;
  root_relpath: string;
  blocks: number;
  items: number;
  enchantments: number;
  entities: number;
}

export function readWorkspaceFile(path: string): Promise<string> {
  return invoke("read_file_string", { path });
}

export function writeWorkspaceFile(path: string, content: string): Promise<void> {
  return invoke("write_file_string", { path, content });
}

export function writeTextFileAbsolute(path: string, content: string): Promise<void> {
  return invoke("write_text_file_absolute", { path, content });
}

export function readAppDataFile(relativePath: string): Promise<string> {
  return invoke("read_user_config_file", { relativePath });
}

export function writeAppDataFile(relativePath: string, content: string): Promise<void> {
  return invoke("write_user_config_file", { relativePath, content });
}

export function getAppDataFilePath(relativePath: string): Promise<string> {
  return invoke("get_user_config_file_path", { relativePath });
}

export function checkFileExists(path: string): Promise<boolean> {
  return invoke("check_file_exists", { path });
}

export function listLitematicFilesInDirectory(path: string, recursive = true): Promise<string[]> {
  return invoke("list_litematic_files_in_directory", { path, recursive });
}

/**
 * Lists the immediate children of a directory with lightweight file metadata.
 */
export function listDirectoryEntries(path: string): Promise<DirectoryEntryInfo[]> {
  return invoke("list_directory_entries", { path });
}

/**
 * Lists .litematic files under a directory with lightweight file metadata.
 */
export function listLitematicFileEntriesInDirectory(path: string, recursive = true): Promise<DirectoryEntryInfo[]> {
  return invoke("list_litematic_file_entries_in_directory", { path, recursive });
}

export function getWorkspaceRoot(): Promise<string> {
  return invoke("get_workspace_root");
}

export function getPathInfo(path: string): Promise<PathInfo> {
  return invoke("get_path_info", { path });
}

export function openWorkspacePath(path: string): Promise<void> {
  return invoke("open_workspace_path", { path });
}

export function openFileParentDir(filePath: string): Promise<void> {
  return invoke("open_file_parent_dir", { filePath });
}

export function copyFileToDirectory(sourcePath: string, targetDirectory: string, targetFileName: string, overwrite = false): Promise<CopyFileToDirectoryOutput> {
  return invoke("copy_file_to_directory", { sourcePath, targetDirectory, targetFileName, overwrite });
}

export function readImageBase64(path: string): Promise<string> {
  return invoke("read_image_base64", { path });
}

export function readProjectionPreviewImage(path: string): Promise<ProjectionPreviewImageOutput | null> {
  return readEmbeddedPreview(path);
}

export function cleanupLocalTempFiles(): Promise<string> {
  return invoke("cleanup_local_temp_files");
}
/**
 * Opens the native picker for a `.litematic` file.
 *
 * @returns The chosen absolute path, or `null` when the user cancels.
 */
export function pickLitematicFile(): Promise<string | null> {
  return invoke("pick_litematic_file");
}

/** Deletes a file inside the app-data (user config) directory. */
export function deleteAppDataFile(relativePath: string): Promise<void> {
  return invoke("delete_user_config_file", { relativePath });
}

/**
 * Unpacks the bundled block-icon archive into the app-data block icon root.
 *
 * The archives live in `pack-in/game-icons/` and are listed in the distributable
 * resource set, so the ~940 Mojang PNGs stay out of the repository.
 */
export function ensureBuiltinBlockIconsExtracted(force = false): Promise<BuiltinIconExtractOutput> {
  return invoke("ensure_builtin_block_icons_extracted", { force });
}

/** Unpacks the bundled item-icon archive into the app-data item icon root. */
export function ensureBuiltinItemIconsExtracted(force = false): Promise<BuiltinIconExtractOutput> {
  return invoke("ensure_builtin_item_icons_extracted", { force });
}

/** Downloads block icons from CCVault (`https://ccvaults.com/`). */
export function downloadVaultBlockIconsFromVault(): Promise<VaultBlockIconDownloadOutput> {
  return invoke("download_vault_block_icons");
}

/** Downloads block icons scraped from Minecraft Wiki (English or Chinese). */
export function downloadWikiBlockIconsFromMinecraftWiki(
  slot: "material_list" | "layering",
  source: "wiki" | "wiki_zh" = "wiki",
): Promise<VaultBlockIconDownloadOutput> {
  return invoke("download_minecraft_wiki_block_icons", { slot, source });
}

/** Downloads item icons from CCVault (`https://ccvaults.com/`). */
export function downloadVaultItemIconsFromVault(): Promise<VaultBlockIconDownloadOutput> {
  return invoke("download_vault_item_icons");
}

/** Downloads the Minecraft Wiki enum catalogs (blocks/items/enchantments/entities). */
export function downloadWikiEnumCatalogsFromMinecraftWiki(): Promise<WikiEnumCatalogDownloadOutput> {
  return invoke("download_minecraft_wiki_enum_catalogs");
}

/** Reads the original GitHub language/catalog source through the Host request limiter. */
export function readGameResourceRemoteText(url: string): Promise<string> {
  return invoke("read_game_resource_remote_text", { url });
}

// ---------------------------------------------------------------------------
// Projection-library commands (C.12, design 2.2.2)
// ---------------------------------------------------------------------------

/** Backend task kinds accepted by the single serial task queue. */
export type LibraryTaskKind =
  | "scan"
  | "checksum"
  | "batch-copy"
  | "batch-rename"
  | "batch-trash"
  | "restore"
  | "export-zip"
  | "index";

/** Folder reference handed to the scanner; only `tracked` participates (includes built-in import). */
export interface LibraryFolderRef {
  path: string;
  type: "tracked" | "non_archive";
}

/** Recheck granularity (four levels, smallest is a single replica). */
export type RecheckScope =
  | { kind: "all" }
  | { kind: "folder"; folderPath: string }
  | { kind: "entry"; entryId: string }
  | { kind: "replica"; replicaId: string };

export interface ScanTaskParams {
  folders: LibraryFolderRef[];
  useCache?: boolean;
  forceRecheck?: boolean;
  scope?: RecheckScope;
}

export interface ChecksumTaskParams {
  paths: string[];
}

export interface LibraryTaskSubmitOutput {
  taskId: string;
}

export interface LibraryTaskCancelOutput {
  cancelled: boolean;
}

export interface TaskDescriptor {
  taskId: string;
  kind: LibraryTaskKind;
}

export interface TaskQueueSnapshot {
  pending: TaskDescriptor[];
  running: TaskDescriptor | null;
}

export interface LitematicChecksum {
  sha256: string;
  byte_count: number;
}

/** Computes the compressed-bytes SHA-256 of a `.litematic` file (never mutates it). */
export function computeLitematicChecksum(path: string): Promise<LitematicChecksum> {
  return invoke("compute_litematic_checksum", { path });
}

/** Submits a task into the single serial background queue. */
export function libraryTaskSubmit(kind: LibraryTaskKind, params: unknown): Promise<LibraryTaskSubmitOutput> {
  return invoke("library_task_submit", { kind, params });
}

/** Requests cooperative cancellation of a pending/running task. */
export function libraryTaskCancel(taskId: string): Promise<LibraryTaskCancelOutput> {
  return invoke("library_task_cancel", { taskId });
}

/** Reads the current queue snapshot (pending + running). */
export function libraryTaskStatus(): Promise<TaskQueueSnapshot> {
  return invoke("library_task_status");
}

export interface TrashMoveItem {
  sourcePath: string;
  trashName: string;
}

export interface TrashMoveResult {
  sourcePath: string;
  trashPath: string;
  ok: boolean;
  reason?: string;
}

export interface TrashRestoreItem {
  trashPath: string;
  targetPath: string;
  renameTo?: string | null;
}

export interface TrashRestoreResult {
  trashPath: string;
  targetPath: string;
  ok: boolean;
  reason?: string;
}

export interface TrashPurgeItem {
  trashPath: string;
}

export interface TrashPurgeResult {
  trashPath: string;
  ok: boolean;
  reason?: string;
}

/** Moves library-managed files into `projection-library/trash`, one result per item. */
export function libraryTrashMove(items: TrashMoveItem[]): Promise<TrashMoveResult[]> {
  return invoke("library_trash_move", { items });
}

/** Restores trashed files; an occupied target reports `ok=false, reason="occupied"`. */
export function libraryTrashRestore(items: TrashRestoreItem[]): Promise<TrashRestoreResult[]> {
  return invoke("library_trash_restore", { items });
}

/** Physically deletes trashed files; unavailable items are skipped and reported. */
export function libraryTrashPurge(items: TrashPurgeItem[]): Promise<TrashPurgeResult[]> {
  return invoke("library_trash_purge", { items });
}

// ---------------------------------------------------------------------------
// Projection-library index commands (D.1/D.2, design 2.2.2(C))
// ---------------------------------------------------------------------------

/** Searchable index fields; matches the Rust `IndexField` enum. */
export type LibraryIndexField =
  | "displayName"
  | "fileName"
  | "author"
  | "description"
  | "tags"
  | "categoryPath"
  | "version";

export interface LibraryIndexQuery {
  text: string;
  /** Restrict matches to these fields; omitted means all fields. */
  fields?: LibraryIndexField[];
}

export interface LibraryIndexQueryResult {
  entryIds: string[];
  /** `false` while the index is still building; results are the ready part. */
  ready: boolean;
}

export interface LibraryIndexStatus {
  ready: boolean;
  indexedCount: number;
  building: boolean;
}

/** One entry's indexable projection handed to the `index` task. */
export interface LibraryIndexEntry {
  entryId: string;
  displayName: string;
  fileName: string;
  author: string;
  description: string;
  tags: string[];
  categoryPath: string;
  version: string;
}

/** Read-only inverted-index lookup; never mutates library data or the index. */
export function libraryIndexQuery(query: LibraryIndexQuery): Promise<LibraryIndexQueryResult> {
  return invoke("library_index_query", { query });
}

/** Reads index readiness/count for the "building, showing partial results" UI. */
export function libraryIndexStatus(): Promise<LibraryIndexStatus> {
  return invoke("library_index_status");
}

// ---------------------------------------------------------------------------
// Packaged export (E.5, design 2.5.4)
// ---------------------------------------------------------------------------

/** One entry of a packaged export; `include.projection` is always true. */
export interface LibraryExportEntry {
  /** Flat zip stem (no extension). */
  archiveName: string;
  sourcePath: string;
  fileName: string;
  displayName: string;
  author: string;
  tags: string[];
  checksum: string | null;
  previewPath?: string;
  materialListText?: string;
}

export interface LibraryExportZipParams {
  outputPath: string;
  exportedAt: string;
  includePreview: boolean;
  includeMaterialList: boolean;
  includeManifest: boolean;
  entries: LibraryExportEntry[];
}

/** Submits the packaged-export task; the caller awaits the `-task-finished` event. */
export function libraryExportZip(params: LibraryExportZipParams): Promise<LibraryTaskSubmitOutput> {
  return invoke("library_export_zip", { params });
}

export function readResourcePackLanguage(language: "zh_cn" | "en_us"): Promise<Record<string, string>> {
  return invoke("read_resource_pack_language", { language });
}

export interface GameDataContext {
  schema_version: number;
  id: string;
  game_version: string;
  content_id: string;
  root: string;
  resource_jar: string;
  source: string;
}
export interface GameDataList { active: GameDataContext | null; entries: GameDataContext[] }
export const listGameData = (): Promise<GameDataList> => invoke("game_data_list");
export const getGameDataContext = (): Promise<GameDataContext> => invoke("game_data_context");
export const readGameDataLanguage = (language: "zh_cn" | "en_us"): Promise<{values: Record<string, string>}> => invoke("game_data_language", { language });
export const readGameDataIcon = (id: string): Promise<string> => invoke("read_game_data_icon", { id });
export const importGameDataJar = (jarPath: string, assetsDir?: string): Promise<GameDataContext> => invoke("game_data_import", { jarPath, assetsDir });
export const selectGameData = (id: string): Promise<GameDataContext> => invoke("game_data_select", { id });
export const restoreDefaultGameData = (): Promise<GameDataContext> => invoke("game_data_default");
