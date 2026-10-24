import { LIBRARY_ICON_PRESETS, normalizeLibraryCategoryIcon } from "./libraryCategoryIcons";
import { windowsPathKey } from "./filePathIdentity";
import {
  readUserConfigFile,
  writeUserConfigFile,
  checkFileExists,
  DirectoryEntryInfo,
  listLitematicFileEntriesInDirectory,
  copyFileToDirectory,
  computeLitematicChecksum,
  getUserConfigFilePath,
  libraryTrashMove,
  libraryTrashRestore,
  libraryTrashPurge,
  libraryExportZip,
  type LibraryFolderRef,
  type LibraryTaskKind,
  type TrashMoveResult,
  type LibraryExportEntry,
  type LibraryExportZipParams,
} from "./backend";
import { inspectProject, editProjectMetadata } from "../platform/applicationOperations";
import { migrateLibraryState, needsLibraryMigration } from "./libraryMigration";
import {
  applyContentDiverge,
  applyContentSync,
  auditRedundancy,
  autoMergeByChecksum,
  buildChecksumIndex,
  entryActiveReplicas,
  reconcileDirtyReplicas,
  resolveEntryId as resolveRedirectChain,
} from "./libraryContent";
import type { DirtyReplicaMove, RedundancyReport } from "./libraryContent";
import { cancelLibraryTask as cancelTask, submitAndAwait, submitLibraryTask as submitTask, subscribeLibraryTask as subscribeTask, awaitLibraryTask } from "./libraryTasks";
import { formatMaterialQuantity, loadStructureStats } from "./statsService";
import { minecraftVersionLabel } from "./minecraftVersions";
import {
  queryIndex,
  submitIndexBuild,
  subscribeIndexStatus as subscribeIndexStatusImpl,
} from "./libraryIndexClient";
import type {
  LibraryIndexEntry,
  LibraryIndexQuery,
  LibraryIndexQueryResult,
  LibraryIndexStatus,
  UnsubscribeIndexStatus,
} from "./libraryIndexClient";

/** Library-level status of a stable entry identity. */
export type EntryStatus = "active" | "retired" | "merged";
/** Physical status of one replica file of an entry. */
export type ReplicaStatus = "active" | "missing" | "parse_error" | "trashed";
/** Folder participation class. `builtin_import` is a tracked folder. */
export type LibraryFolderType = "tracked" | "non_archived" | "builtin_import";
/** Trash lifecycle status. `legacy` records came from the pre-v5 single-layer model. */
export type TrashEntryStatus = "in_trash" | "restored" | "purged" | "legacy";

/** Library-level metadata and the stable identity of one logical projection. */
export interface LibraryEntry {
  id: string;
  createdAt: number;
  /** Current content fingerprint (compressed file bytes SHA-256); `null` until computed. */
  checksum: string | null;
  status: EntryStatus;
  categoryIds: string[];
  displayName: string;
  /** NBT `Name` metadata, kept separate from the file's display name. */
  internalName?: string;
  author: string;
  description: string;
  /** Authoritative in-library mirror of the file's `Metadata.NovaTags`. */
  tags: string[];
  preview: EntryPreview;
  stats: EntryStats;
  sortIndex: number;
  lastAnalyzedAt: number;
  lastError: string;
}

export interface EntryPreview {
  path?: string;
  preview_image_path?: string;
  updatedAt?: number;
}

export interface EntryStats {
  totalBlocks: number;
  totalVolume: number;
  regionCount: number;
  regionNames?: string[];
  enclosingSize: { x: number; y: number; z: number };
  minecraftDataVersion: number;
}

/** One physical file that carries the content of an entry. */
export interface Replica {
  id: string;
  entryId: string;
  path: string;
  fileName: string;
  folderPath: string;
  checksum: string | null;
  fileSize: number;
  mtime: number;
  status: ReplicaStatus;
  lastAnalyzedAt: number;
  lastError: string;
}

/** Complete old-id → new-id mapping appended on every automatic merge. */
export interface EntryIdRedirect {
  fromId: string;
  toId: string;
  at: number;
}

/** Mixed recent reference: an entry reference or a bare path (non-archived activation). */
export interface RecentRef {
  kind: "entry" | "path";
  entryId?: string;
  path?: string;
  checksum?: string;
}

export interface LocalLibraryFolder {
  path: string;
  type: LibraryFolderType;
  enabled: boolean;
  recursive: boolean;
  /** When on (default), mounting / syncing / refreshing / importing auto-imports this folder's new files (B.1). */
  autoActivate: boolean;
  sort_index: number;
  lastSyncedAt: number;
  lastScanCount: number;
  lastError: string;
}

export interface SyncLibraryFoldersResult {
  importedCount: number;
  scannedFolderCount: number;
  state: LibraryState;
  errors: string[];
}

export type LibraryCategoryIcon =
  | { kind: "lucide"; name: string }
  | { kind: "minecraft"; blockId: string }
  | { kind: "image"; dataUrl: string };

export interface LibraryCategory {
  id: string;
  name: string;
  parentId: string | null;
  sort_index: number;
  icon: LibraryCategoryIcon;
}

/**
 * Flat projection of one replica plus its entry metadata.
 * A-phase transitional read model so existing views keep working on the two-level data;
 * the full view rewrite happens in phase D.
 */
export interface LibraryRecordView {
  id: string;
  entryId: string;
  path: string;
  fileName: string;
  displayName: string;
  internalName?: string;
  author: string;
  description: string;
  tags: string[];
  totalBlocks: number;
  totalVolume: number;
  regionCount: number;
  regionNames?: string[];
  enclosingSize: { x: number; y: number; z: number };
  minecraftDataVersion: number;
  fileSize: number;
  mtime: number;
  checksum: string | null;
  categoryIds: string[];
  /** Legacy three-state status; `trashed` replicas do not produce a view. */
  status: "ok" | "missing" | "parse_error";
  lastAnalyzedAt: number;
  lastError: string;
  previewPath?: string;
  preview_image_path?: string;
  previewUpdatedAt?: number;
  sort_index: number;
}

export interface LibraryTrashEntry {
  id: string;
  replicaSnapshot: Replica | null;
  originalPath: string;
  originalFileName: string;
  trashPath: string;
  movedAt: number;
  status: TrashEntryStatus;
  /** Legacy compatibility fields kept so the current trash view keeps compiling (phase E rewrites it). */
  deletedAt: number;
  name: string;
  kind: "projection" | "category";
  records: LibraryRecordView[];
  categories: LibraryCategory[];
}

export interface LibraryState {
  schemaVersion: 5;
  entries: LibraryEntry[];
  replicas: Replica[];
  folders: LocalLibraryFolder[];
  categories: LibraryCategory[];
  trash: LibraryTrashEntry[];
  redirects: EntryIdRedirect[];
  favorites: string[];
  recent: RecentRef[];
}

/** Loose input accepted by the normalizer/migrator so legacy and custom fields survive. */
export type RawLibraryState = Partial<LibraryState> & Record<string, any>;

const LIB_PATH = "projection-library/js_library.json";
const SCHEMA_VERSION = 5;
const RECENT_LIMIT = 20;

/** Fixed, always-mounted built-in import folder under the user config dir (5.1.1.7, 6.3.3). */
export const BUILTIN_IMPORT_RELATIVE_PATH = "projection-library/import";

const TAGS_SUMMARY_PREFIX = "标签：";
const TAGS_SUMMARY_SEPARATOR = "\n\n";

/**
 * Strips the redundant tag summary that E.8 mirrors into the file's `Description`
 * (裁决 2) so the library's authoritative `description` stays clean when a file is
 * re-analyzed. Mirrors `metadata_edit.rs::strip_tags_summary`.
 */
export function stripTagsSummaryFromDescription(description: string): string {
  const marker = `${TAGS_SUMMARY_SEPARATOR}${TAGS_SUMMARY_PREFIX}`;
  const index = description.lastIndexOf(marker);
  if (index >= 0) {
    const tail = description.slice(index + marker.length);
    if (!tail.includes("\n")) return description.slice(0, index);
  }
  return description;
}

function folderKey(path: string): string {
  return windowsPathKey(path);
}

export function parentDirectory(path: string): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  const lastSeparatorIndex = Math.max(normalized.lastIndexOf("\\"), normalized.lastIndexOf("/"));
  if (lastSeparatorIndex < 0) return "";
  const parent = normalized.slice(0, lastSeparatorIndex);
  return /^(?:\\\\\?\\)?[a-z]:$/i.test(parent) ? `${parent}${normalized[lastSeparatorIndex]}` : parent;
}

export function fileNameFromPath(filePath: string): string {
  return filePath.split(/[\\/]/).pop() || "";
}

function asRecord(value: unknown): Record<string, any> {
  return value && typeof value === "object" ? (value as Record<string, any>) : {};
}

function finiteNumber(value: unknown, fallback: number): number {
  return Number.isFinite(value) ? Number(value) : fallback;
}

function stringValue(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function normalizeSize(value: unknown): { x: number; y: number; z: number } {
  const size = asRecord(value);
  return { x: finiteNumber(size.x, 0), y: finiteNumber(size.y, 0), z: finiteNumber(size.z, 0) };
}

function normalizeEntryStatus(value: unknown): EntryStatus {
  return value === "retired" || value === "merged" || value === "active" ? value : "active";
}

export function normalizeReplicaStatus(value: unknown): ReplicaStatus {
  return value === "missing" || value === "parse_error" || value === "trashed" || value === "active" ? value : "active";
}

function normalizeFolderType(value: unknown): LibraryFolderType {
  return value === "non_archived" || value === "builtin_import" || value === "tracked" ? value : "tracked";
}

function normalizeFolder(folder: Record<string, any>, index: number): LocalLibraryFolder {
  const path = stringValue(folder.path).trim();
  return {
    ...folder,
    path,
    type: normalizeFolderType(folder.type),
    enabled: folder.enabled !== false,
    recursive: folder.recursive !== false,
    // Missing = default true (B.1 normalization/migration rule).
    autoActivate: folder.autoActivate !== false,
    sort_index: finiteNumber(folder.sort_index, index),
    lastSyncedAt: finiteNumber(folder.lastSyncedAt, 0),
    lastScanCount: finiteNumber(folder.lastScanCount, 0),
    lastError: stringValue(folder.lastError),
  };
}

export function normalizeLibraryCategories(raw?: unknown): { categories: LibraryCategory[]; ids: Set<string> } {
  const categories: LibraryCategory[] = [];
  const ids = new Set<string>();
  for (const [index, entry] of (Array.isArray(raw) ? raw : []).entries()) {
    const candidate = asRecord(entry);
    if (typeof candidate.id !== "string" || !candidate.id || ids.has(candidate.id)) continue;
    ids.add(candidate.id);
    categories.push({
      ...candidate,
      id: candidate.id,
      name: typeof candidate.name === "string" && candidate.name.trim() ? candidate.name.trim() : "恢复的分类",
      parentId: typeof candidate.parentId === "string" ? candidate.parentId : null,
      sort_index: finiteNumber(candidate.sort_index, index),
      icon: normalizeLibraryCategoryIcon(candidate.icon),
    });
  }
  for (const entry of categories) {
    if (!entry.parentId || !ids.has(entry.parentId)) { entry.parentId = null; continue; }
    const visited = new Set([entry.id]);
    let parent: LibraryCategory | undefined = entry;
    while (parent?.parentId) {
      if (visited.has(parent.parentId)) { entry.parentId = null; break; }
      visited.add(parent.parentId);
      parent = categories.find((item) => item.id === parent!.parentId);
    }
  }
  return { categories, ids };
}

function normalizeEntry(raw: Record<string, any>, categories: LibraryCategory[], categoryIds: Set<string>, index: number): LibraryEntry | null {
  if (typeof raw.id !== "string" || !raw.id) return null;
  const ids = [...new Set((Array.isArray(raw.categoryIds) ? raw.categoryIds : []).filter((id: unknown): id is string => typeof id === "string" && !!id))];
  for (const id of ids) if (!categoryIds.has(id)) {
    let restored = `restored:${id}`;
    while (categoryIds.has(restored)) restored += ":legacy";
    categories.push({ id: restored, name: "恢复的分类", parentId: null, sort_index: categories.length, icon: { kind: "lucide", name: "Folder" } });
    categoryIds.add(restored);
    ids[ids.indexOf(id)] = restored;
  }
  // `regionNames` lives on `stats` for the two-level model; a legacy top-level
  // `regionNames` is still accepted so an un-migrated alias keeps its data.
  const regionNamesSource = raw.stats?.regionNames ?? raw.regionNames;
  const regionNames = Array.isArray(regionNamesSource) && regionNamesSource.every((name: unknown) => typeof name === "string") ? [...regionNamesSource] : undefined;
  const preview = asRecord(raw.preview);
  return {
    ...raw,
    id: raw.id,
    createdAt: finiteNumber(raw.createdAt, 0),
    checksum: typeof raw.checksum === "string" ? raw.checksum : null,
    status: normalizeEntryStatus(raw.status),
    categoryIds: [...new Set(ids)],
    displayName: typeof raw.displayName === "string" && raw.displayName.trim() ? raw.displayName : "未命名投影",
    internalName: typeof raw.internalName === "string" ? raw.internalName : undefined,
    author: stringValue(raw.author),
    description: stringValue(raw.description),
    tags: (Array.isArray(raw.tags) ? raw.tags : []).filter((tag: unknown): tag is string => typeof tag === "string"),
    preview: {
      path: typeof preview.path === "string" ? preview.path : undefined,
      preview_image_path: typeof preview.preview_image_path === "string" ? preview.preview_image_path : undefined,
      updatedAt: Number.isFinite(preview.updatedAt) ? Number(preview.updatedAt) : undefined,
    },
    stats: {
      totalBlocks: finiteNumber(raw.stats?.totalBlocks, 0),
      totalVolume: finiteNumber(raw.stats?.totalVolume, 0),
      regionCount: finiteNumber(raw.stats?.regionCount, 0),
      regionNames,
      enclosingSize: normalizeSize(raw.stats?.enclosingSize),
      minecraftDataVersion: finiteNumber(raw.stats?.minecraftDataVersion, 0),
    },
    sortIndex: finiteNumber(raw.sortIndex, index),
    lastAnalyzedAt: finiteNumber(raw.lastAnalyzedAt, 0),
    lastError: stringValue(raw.lastError),
  };
}

function normalizeReplica(raw: Record<string, any>, entryIds: Set<string>): Replica | null {
  const path = stringValue(raw.path).trim();
  if (!path || typeof raw.id !== "string" || !raw.id) return null;
  if (typeof raw.entryId !== "string" || !entryIds.has(raw.entryId)) return null;
  const fileName = stringValue(raw.fileName) || fileNameFromPath(path);
  return {
    ...raw,
    id: raw.id,
    entryId: raw.entryId,
    path,
    fileName,
    folderPath: stringValue(raw.folderPath) || parentDirectory(path),
    checksum: typeof raw.checksum === "string" ? raw.checksum : null,
    fileSize: finiteNumber(raw.fileSize, 0),
    mtime: finiteNumber(raw.mtime, 0),
    status: normalizeReplicaStatus(raw.status),
    lastAnalyzedAt: finiteNumber(raw.lastAnalyzedAt, 0),
    lastError: stringValue(raw.lastError),
  };
}

export function toRecordView(entry: LibraryEntry, replica: Replica): LibraryRecordView {
  return {
    id: replica.id,
    entryId: entry.id,
    path: replica.path,
    fileName: replica.fileName,
    displayName: entry.displayName,
    internalName: entry.internalName,
    author: entry.author,
    description: entry.description,
    tags: [...entry.tags],
    totalBlocks: entry.stats.totalBlocks,
    totalVolume: entry.stats.totalVolume,
    regionCount: entry.stats.regionCount,
    regionNames: entry.stats.regionNames,
    enclosingSize: { ...entry.stats.enclosingSize },
    minecraftDataVersion: entry.stats.minecraftDataVersion,
    fileSize: replica.fileSize,
    mtime: replica.mtime,
    checksum: replica.checksum,
    categoryIds: [...entry.categoryIds],
    status: replica.status === "active" ? "ok" : replica.status === "missing" ? "missing" : "parse_error",
    lastAnalyzedAt: replica.lastAnalyzedAt,
    lastError: replica.lastError,
    previewPath: entry.preview.path,
    preview_image_path: entry.preview.preview_image_path,
    previewUpdatedAt: entry.preview.updatedAt,
    sort_index: entry.sortIndex,
  };
}

/** Flat views of every non-trashed replica (A-phase transitional read model). */
export function libraryRecordViews(state: LibraryState): LibraryRecordView[] {
  const entries = new Map(state.entries.map((entry) => [entry.id, entry]));
  const views: LibraryRecordView[] = [];
  for (const replica of state.replicas) {
    if (replica.status === "trashed") continue;
    const entry = entries.get(replica.entryId);
    if (!entry || entry.status === "merged") continue;
    views.push(toRecordView(entry, replica));
  }
  return views;
}

function normalizeRecentRef(raw: Record<string, any>): RecentRef | null {
  if (raw.kind === "path") {
    const path = stringValue(raw.path).trim();
    if (!path) return null;
    return { kind: "path", path, checksum: typeof raw.checksum === "string" ? raw.checksum : undefined };
  }
  if (typeof raw.entryId === "string" && raw.entryId) return { kind: "entry", entryId: raw.entryId };
  return null;
}

function normalizeTrashEntry(raw: Record<string, any>): LibraryTrashEntry {
  const candidates = Array.isArray(raw.records) ? raw.records : [];
  return {
    ...raw,
    id: stringValue(raw.id),
    replicaSnapshot: raw.replicaSnapshot && typeof raw.replicaSnapshot === "object" ? raw.replicaSnapshot as Replica : null,
    originalPath: stringValue(raw.originalPath),
    originalFileName: stringValue(raw.originalFileName),
    trashPath: stringValue(raw.trashPath),
    movedAt: finiteNumber(raw.movedAt, 0),
    status: raw.status === "restored" || raw.status === "purged" || raw.status === "legacy" || raw.status === "in_trash" ? raw.status : "in_trash",
    deletedAt: finiteNumber(raw.deletedAt, 0),
    name: stringValue(raw.name),
    kind: raw.kind === "category" ? "category" : "projection",
    records: candidates,
    categories: normalizeLibraryCategories(raw.categories).categories,
  };
}

/** Pure two-level normalization; idempotent and preserving unknown/custom fields. */
function normalizeTwoLevelState(input: Record<string, any>): LibraryState {
  const categories = normalizeLibraryCategories(input.categories);
  const entries: LibraryEntry[] = [];
  const seenEntryIds = new Set<string>();
  for (const [index, raw] of (Array.isArray(input.entries) ? input.entries : []).entries()) {
    const entry = normalizeEntry(asRecord(raw), categories.categories, categories.ids, index);
    if (!entry || seenEntryIds.has(entry.id)) continue;
    seenEntryIds.add(entry.id);
    entries.push(entry);
  }
  const entryIds = new Set(entries.map((entry) => entry.id));
  const replicas: Replica[] = [];
  const seenReplicaIds = new Set<string>();
  for (const raw of (Array.isArray(input.replicas) ? input.replicas : [])) {
    const replica = normalizeReplica(asRecord(raw), entryIds);
    if (!replica || seenReplicaIds.has(replica.id)) continue;
    seenReplicaIds.add(replica.id);
    replicas.push(replica);
  }
  // Backfill a display name from the first replica when an entry has none yet.
  for (const entry of entries) {
    if (entry.displayName === "未命名投影") {
      const first = replicas.find((replica) => replica.entryId === entry.id);
      if (first && first.fileName) entry.displayName = first.fileName;
    }
  }

  const foldersByKey = new Map<string, LocalLibraryFolder>();
  for (const [index, raw] of (Array.isArray(input.folders) ? input.folders : []).entries()) {
    const normalized = normalizeFolder(asRecord(raw), index);
    if (!normalized.path) continue;
    const key = folderKey(normalized.path);
    const previous = foldersByKey.get(key);
    if (!previous) { foldersByKey.set(key, normalized); continue; }
    const latest = normalized.lastSyncedAt > previous.lastSyncedAt ? normalized : previous;
    // Conservative: a disabled alias stays disabled; keep either alias's scan coverage; smallest sort_index wins.
    foldersByKey.set(key, {
      ...normalized, ...previous,
      type: previous.type || normalized.type,
      enabled: previous.enabled && normalized.enabled,
      recursive: previous.recursive || normalized.recursive,
      autoActivate: previous.autoActivate && normalized.autoActivate,
      sort_index: Math.min(previous.sort_index, normalized.sort_index),
      lastSyncedAt: latest.lastSyncedAt,
      lastScanCount: latest.lastScanCount,
      lastError: latest.lastError,
    });
  }
  const folders = [...foldersByKey.values()];
  folders.sort((left, right) => left.sort_index - right.sort_index || left.path.localeCompare(right.path));
  folders.forEach((folder, index) => { folder.sort_index = index; });

  const trash = (Array.isArray(input.trash) ? input.trash : [])
    .filter((entry: unknown) => entry && typeof asRecord(entry).id === "string")
    .map((entry: unknown) => normalizeTrashEntry(asRecord(entry)));

  const redirects = (Array.isArray(input.redirects) ? input.redirects : []).map((raw: unknown) => asRecord(raw))
    .filter((raw: Record<string, any>) => typeof raw.fromId === "string" && typeof raw.toId === "string")
    .map((raw: Record<string, any>) => ({ fromId: raw.fromId, toId: raw.toId, at: finiteNumber(raw.at, 0) }));

  const favorites = [...new Set((Array.isArray(input.favorites) ? input.favorites : []).filter((id: unknown): id is string => typeof id === "string" && !!id))];
  const recent = (Array.isArray(input.recent) ? input.recent : []).map((raw: unknown) => normalizeRecentRef(asRecord(raw))).filter((ref: RecentRef | null): ref is RecentRef => !!ref);

  const preliminary: LibraryState = {
    ...input,
    schemaVersion: SCHEMA_VERSION,
    entries,
    replicas,
    folders,
    categories: categories.categories,
    trash,
    redirects,
    favorites,
    recent,
  };
  // Invariant convergence (4.2.1 / 裁决 9): a replica whose active checksum
  // disagrees with its entry is migrated out deterministically, never silently
  // dropped. Idempotent: once converged, a second pass is a no-op.
  const reconciled = reconcileDirtyReplicas(preliminary);
  return { ...preliminary, entries: reconciled.state.entries, replicas: reconciled.state.replicas };
}

export function normalizeLibraryState(raw?: RawLibraryState | null): LibraryState {
  const input = asRecord(raw);
  const source = needsLibraryMigration(input) ? migrateLibraryState(input) : input;
  return normalizeTwoLevelState(source);
}

export function categoryDescendants(state: LibraryState, id: string): Set<string> {
  const result = new Set<string>([id]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const entry of state.categories || []) if (entry.parentId && result.has(entry.parentId) && !result.has(entry.id)) { result.add(entry.id); changed = true; }
  }
  return result;
}

export function categoryPath(state: LibraryState, id: string): string {
  const names: string[] = [];
  const seen = new Set<string>();
  let entry = state.categories?.find((item) => item.id === id);
  while (entry && !seen.has(entry.id)) {
    seen.add(entry.id);
    names.unshift(entry.name);
    entry = state.categories?.find((item) => item.id === entry!.parentId);
  }
  return names.join(" / ");
}

function categoryName(name: string): string {
  const value = name.trim();
  if (!value || value.length > 80 || /[\\/\x00-\x1f]/.test(value)) throw new Error("分类名称需为 1–80 个字符，且不包含斜线或控制字符。");
  return value;
}

function assertCategoryParent(state: LibraryState, parentId: string | null): void {
  if (parentId && !state.categories?.some((item) => item.id === parentId)) throw new Error("目标分类已不存在，请刷新后重试。");
}

export function createLibraryCategory(state: LibraryState, name: string, parentId: string | null = null, id = `category:${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`): LibraryState {
  const next = normalizeLibraryState(state);
  assertCategoryParent(next, parentId);
  const value = categoryName(name);
  if (next.categories.some((entry) => entry.id === id || (entry.parentId === parentId && entry.name === value))) throw new Error("同一位置已存在此分类，请换一个名称。");
  return { ...next, categories: [...next.categories, { id, name: value, parentId, sort_index: next.categories.length, icon: { kind: "lucide", name: "Folder" } }] };
}

export function updateLibraryCategory(state: LibraryState, id: string, patch: Partial<Pick<LibraryCategory, "name" | "parentId" | "icon">>): LibraryState {
  const next = normalizeLibraryState(state);
  const entry = next.categories.find((item) => item.id === id);
  if (!entry) throw new Error("分类已不存在，请刷新后重试。");
  const parentId = patch.parentId === undefined ? entry.parentId : patch.parentId;
  assertCategoryParent(next, parentId);
  if (parentId && categoryDescendants(next, id).has(parentId)) throw new Error("分类不能移动到自身或子分类中。");
  const name = patch.name === undefined ? entry.name : categoryName(patch.name);
  if (next.categories.some((item) => item.id !== id && item.parentId === parentId && item.name === name)) throw new Error("目标位置存在同名分类。");
  return { ...next, categories: next.categories.map((item) => item.id === id ? { ...item, name, parentId, icon: patch.icon === undefined ? item.icon : normalizeLibraryCategoryIcon(patch.icon) } : item) };
}

function trashId(): string { return `trash:${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`; }

function snapshotCategories(state: LibraryState, ids: string[]): LibraryCategory[] {
  const included = new Set<string>();
  for (let id of ids) while (id && !included.has(id)) {
    const category = state.categories.find((entry) => entry.id === id);
    if (!category) break;
    included.add(id);
    id = category.parentId || "";
  }
  return state.categories.filter((entry) => included.has(entry.id)).map((entry) => ({ ...entry, icon: { ...entry.icon } }));
}

function replicaView(state: LibraryState, replica: Replica): LibraryRecordView | null {
  const entry = state.entries.find((item) => item.id === replica.entryId);
  return entry ? toRecordView(entry, replica) : null;
}

function buildTrashEntry(state: LibraryState, kind: "projection" | "category", name: string, replicas: Replica[], categories: LibraryCategory[]): LibraryTrashEntry {
  const views = replicas.map((replica) => replicaView(state, replica)).filter((view): view is LibraryRecordView => !!view);
  const first = replicas[0];
  const at = Date.now();
  return {
    id: trashId(),
    replicaSnapshot: first ? { ...first } : null,
    originalPath: first?.path || "",
    originalFileName: first?.fileName || "",
    trashPath: "",
    movedAt: at,
    status: "in_trash",
    deletedAt: at,
    name,
    kind,
    records: views,
    categories,
  };
}

/** Remove library memberships (replica → trashed). Original files and cached previews remain untouched. */
export function trashLibraryRecords(state: LibraryState, records: LibraryRecordView[]): LibraryState {
  const next = normalizeLibraryState(state);
  const ids = new Set(records.map((record) => record.id));
  const targets = next.replicas.filter((replica) => ids.has(replica.id));
  if (!targets.length) return next;
  const targetIds = new Set(targets.map((replica) => replica.id));
  const replicas = next.replicas.map((replica) => targetIds.has(replica.id) ? { ...replica, status: "trashed" as ReplicaStatus } : replica);
  const entries = retiresWithoutActiveReplicas(next.entries, replicas);
  return normalizeLibraryState({
    ...next,
    replicas,
    entries,
    trash: [...next.trash, buildTrashEntry(next, "projection", targets.length === 1 ? (targets[0].fileName || "1 个投影") : `${targets.length} 个投影`, targets, snapshotCategories(next, targets.flatMap((replica) => next.entries.find((entry) => entry.id === replica.entryId)?.categoryIds || [])))],
  });
}

function retiresWithoutActiveReplicas(entries: LibraryEntry[], replicas: Replica[]): LibraryEntry[] {
  const activeByEntry = new Set(replicas.filter((replica) => replica.status === "active").map((replica) => replica.entryId));
  return entries.map((entry) => entry.status === "active" && !activeByEntry.has(entry.id) ? { ...entry, status: "retired" as EntryStatus } : entry);
}

export function restoreLibraryTrash(state: LibraryState, id: string): LibraryState {
  const next = normalizeLibraryState(state);
  const entry = next.trash.find((item) => item.id === id);
  if (!entry) return next;
  const replicas = next.replicas.map((replica) => {
    if (replica.status !== "trashed") return replica;
    const view = entry.records.find((record) => record.id === replica.id);
    if (!view) return replica;
    return { ...replica, status: (view.status === "ok" ? "active" : view.status) as ReplicaStatus };
  });
  const activeByEntry = new Set(replicas.filter((replica) => replica.status === "active").map((replica) => replica.entryId));
  const entries = next.entries.map((item) => activeByEntry.has(item.id) && item.status === "retired" ? { ...item, status: "active" as EntryStatus } : item);
  const categories = [...next.categories];
  for (const saved of entry.categories) if (!categories.some((category) => category.id === saved.id)) categories.push({ ...saved, icon: { ...saved.icon } });
  return normalizeLibraryState({ ...next, entries, replicas, categories, trash: next.trash.filter((item) => item.id !== id) });
}

/** Permanently discard Nova records from the trash (never touches user files). */
export function discardLibraryTrash(state: LibraryState, id?: string): LibraryState {
  const next = normalizeLibraryState(state);
  const removed = id ? next.trash.filter((entry) => entry.id === id) : next.trash;
  if (!removed.length) return { ...next, trash: id ? next.trash : [] };
  const replicaIds = new Set(removed.flatMap((entry) => entry.records.map((record) => record.id)));
  const replicas = next.replicas.filter((replica) => !(replica.status === "trashed" && replicaIds.has(replica.id)));
  const keptEntryIds = new Set(replicas.map((replica) => replica.entryId));
  const entries = next.entries.filter((entry) => keptEntryIds.has(entry.id) || entry.status === "active");
  return normalizeLibraryState({ ...next, entries, replicas, trash: id ? next.trash.filter((entry) => entry.id !== id) : [] });
}

export function deleteLibraryCategory(state: LibraryState, id: string): LibraryState {
  const next = normalizeLibraryState(state);
  const entry = next.categories.find((item) => item.id === id);
  if (!entry) return next;
  const removedIds = categoryDescendants(next, id);
  const affectedEntries = next.entries.filter((item) => item.categoryIds.some((categoryId) => removedIds.has(categoryId)));
  const affectedEntryIds = new Set(affectedEntries.map((item) => item.id));
  const targets = next.replicas.filter((replica) => replica.entryId && affectedEntryIds.has(replica.entryId) && replica.status !== "trashed");
  const targetIds = new Set(targets.map((replica) => replica.id));
  const replicas = next.replicas.map((replica) => targetIds.has(replica.id) ? { ...replica, status: "trashed" as ReplicaStatus } : replica);
  const entries = retiresWithoutActiveReplicas(next.entries, replicas);
  const snapshot = snapshotCategories(next, [...removedIds, ...affectedEntries.flatMap((item) => item.categoryIds)]);
  const trash = targets.length ? [...next.trash, buildTrashEntry(next, "category", entry.name, targets, snapshot)] : next.trash;
  return normalizeLibraryState({ ...next, entries, replicas, categories: next.categories.filter((item) => !removedIds.has(item.id)), trash });
}

export function moveLibraryRecordsToCategory(state: LibraryState, paths: string[], categoryId: string | null): LibraryState {
  const next = normalizeLibraryState(state);
  assertCategoryParent(next, categoryId);
  const keys = new Set(paths.map(windowsPathKey));
  const entryIds = new Set(next.replicas.filter((replica) => keys.has(windowsPathKey(replica.path))).map((replica) => replica.entryId));
  return normalizeLibraryState({
    ...next,
    entries: next.entries.map((entry) => entryIds.has(entry.id) ? { ...entry, categoryIds: categoryId ? [categoryId] : [] } : entry),
  });
}

export interface ProjectionAnalysis {
  status: "ok" | "parse_error";
  lastError: string;
  internalName: string;
  author: string;
  description: string;
  totalBlocks: number;
  totalVolume: number;
  regionCount: number;
  regionNames?: string[];
  enclosingSize: { x: number; y: number; z: number };
  minecraftDataVersion: number;
  fileName: string;
  displayName: string;
}

function applyAnalysis(entry: LibraryEntry, analysis: ProjectionAnalysis): LibraryEntry {
  return {
    ...entry,
    internalName: analysis.internalName,
    author: analysis.author || "",
    description: analysis.description || "",
    stats: {
      totalBlocks: analysis.totalBlocks || 0,
      totalVolume: analysis.totalVolume || 0,
      regionCount: analysis.regionCount || 0,
      regionNames: analysis.regionNames,
      enclosingSize: analysis.enclosingSize || { x: 0, y: 0, z: 0 },
      minecraftDataVersion: analysis.minecraftDataVersion || 0,
    },
  };
}

// --- C.4: same-process serial write queue + optimistic concurrency -------------
// All `loadLibrary -> mutate -> save` sequences funnel through this promise chain
// so concurrent callers cannot interleave and clobber each other. Cross-process
// serialisation is guaranteed by the Rust `CONFIG_WRITE_LOCK` inside
// `write_user_config_file`. Before writing we re-read the disk: if another window
// wrote a newer state, we merge it (union by id, memory wins) instead of
// overwriting it.
let libraryWriteChain: Promise<unknown> = Promise.resolve();
let loadedBaseline: string | null = null;

function enqueueLibraryWrite<T>(operation: () => Promise<T>): Promise<T> {
  const result = libraryWriteChain.then(operation, operation);
  libraryWriteChain = result.then(() => undefined, () => undefined);
  return result;
}

async function readDiskLibraryRaw(): Promise<RawLibraryState | null> {
  try { return JSON.parse(await readUserConfigFile(LIB_PATH)) as RawLibraryState; }
  catch { return null; }
}

/** Union merge of disk (base) and memory (latest intent); entries/replicas never physically vanish. */
function mergeLibraryStates(disk: LibraryState, memory: LibraryState): LibraryState {
  const mergeById = <T,>(base: T[], overlay: T[], key: (item: T) => string): T[] => {
    const map = new Map<string, T>();
    for (const item of base) map.set(key(item), item);
    for (const item of overlay) map.set(key(item), item);
    return [...map.values()];
  };
  return {
    ...disk,
    ...memory,
    schemaVersion: SCHEMA_VERSION,
    entries: mergeById(disk.entries, memory.entries, (entry) => entry.id),
    replicas: mergeById(disk.replicas, memory.replicas, (replica) => replica.id),
    folders: mergeById(disk.folders, memory.folders, (folder) => windowsPathKey(folder.path)),
    categories: mergeById(disk.categories, memory.categories, (category) => category.id),
    trash: memory.trash,
    redirects: memory.redirects,
    favorites: memory.favorites,
    recent: memory.recent,
  };
}

async function persistLibraryState(state: LibraryState): Promise<LibraryState> {
  return enqueueLibraryWrite(async () => {
    let normalized = normalizeLibraryState(state);
    const raw = await readDiskLibraryRaw();
    if (raw) {
      const disk = normalizeLibraryState(raw);
      if (loadedBaseline !== null && JSON.stringify(disk) !== loadedBaseline) {
        normalized = normalizeLibraryState(mergeLibraryStates(disk, normalized));
      }
    }
    await writeUserConfigFile(LIB_PATH, JSON.stringify(normalized, null, 2));
    loadedBaseline = JSON.stringify(normalized);
    return normalized;
  });
}

function newEntryId(): string {
  return `entry:${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function newReplicaId(entryId: string): string {
  return `replica:${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}:${entryId}`;
}

function emptyEntry(filePath: string, analysis: Partial<ProjectionAnalysis>): LibraryEntry {
  const fileName = analysis.fileName || fileNameFromPath(filePath);
  const at = Date.now();
  return {
    id: newEntryId(),
    createdAt: at,
    checksum: null,
    status: "active",
    categoryIds: [],
    displayName: analysis.displayName || fileName,
    internalName: analysis.internalName,
    author: analysis.author || "",
    description: analysis.description || "",
    tags: [],
    preview: {},
    stats: {
      totalBlocks: analysis.totalBlocks || 0,
      totalVolume: analysis.totalVolume || 0,
      regionCount: analysis.regionCount || 0,
      regionNames: analysis.regionNames,
      enclosingSize: analysis.enclosingSize || { x: 0, y: 0, z: 0 },
      minecraftDataVersion: analysis.minecraftDataVersion || 0,
    },
    sortIndex: 0,
    lastAnalyzedAt: at,
    lastError: analysis.lastError || "",
  };
}

async function addOrUpdateRecordFromAnalysisInternal(
  state: LibraryState,
  filePath: string,
  analysis: Partial<ProjectionAnalysis>,
  persist: boolean,
): Promise<LibraryState> {
  const nextState = normalizeLibraryState(state);
  const key = windowsPathKey(filePath);
  const replicaIndex = nextState.replicas.findIndex((replica) => windowsPathKey(replica.path) === key);
  const existing = replicaIndex >= 0 ? nextState.replicas[replicaIndex] : null;
  const status: ReplicaStatus = analysis.status === "parse_error" ? "parse_error" : "active";
  const fileName = existing?.fileName || fileNameFromPath(existing?.path || filePath);

  if (existing) {
    const entryIndex = nextState.entries.findIndex((entry) => entry.id === existing.entryId);
    const full: ProjectionAnalysis = {
      status: analysis.status || "ok",
      lastError: analysis.lastError || "",
      internalName: analysis.internalName || "",
      author: analysis.author || "",
      description: analysis.description || "",
      totalBlocks: analysis.totalBlocks || 0,
      totalVolume: analysis.totalVolume || 0,
      regionCount: analysis.regionCount || 0,
      regionNames: analysis.regionNames,
      enclosingSize: analysis.enclosingSize || { x: 0, y: 0, z: 0 },
      minecraftDataVersion: analysis.minecraftDataVersion || 0,
      fileName,
      displayName: analysis.displayName || fileName,
    };
    nextState.replicas[replicaIndex] = { ...existing, status, lastAnalyzedAt: Date.now(), lastError: analysis.lastError || "" };
    if (entryIndex >= 0) nextState.entries[entryIndex] = applyAnalysis(nextState.entries[entryIndex], full);
  } else {
    // C.2 wiring: a brand-new replica must never become a `checksum = null` orphan.
    // Compute the file fingerprint and, when an entry already owns that content,
    // attach the replica to it (checksum merge kernel) instead of creating a
    // duplicate entry; otherwise create a fingerprinted entry.
    const fingerprintResult = await computeLitematicChecksum(filePath).catch(() => null);
    const fingerprint = fingerprintResult?.sha256 ?? null;
    const at = Date.now();
    const targetEntryId = fingerprint ? buildChecksumIndex(nextState).get(fingerprint) : undefined;
    if (targetEntryId) {
      const targetIndex = nextState.entries.findIndex((entry) => entry.id === targetEntryId);
      if (targetIndex >= 0 && nextState.entries[targetIndex].status === "retired") {
        nextState.entries[targetIndex] = { ...nextState.entries[targetIndex], status: "active", checksum: fingerprint };
      }
      nextState.replicas = [{
        id: newReplicaId(targetEntryId),
        entryId: targetEntryId,
        path: filePath,
        fileName,
        folderPath: parentDirectory(filePath),
        checksum: fingerprint,
        fileSize: fingerprintResult?.byte_count ?? 0,
        mtime: 0,
        status,
        lastAnalyzedAt: at,
        lastError: analysis.lastError || "",
      }, ...nextState.replicas];
    } else {
      const entry = emptyEntry(filePath, analysis);
      entry.checksum = fingerprint;
      entry.status = status === "active" ? "active" : "retired";
      const replica: Replica = {
        id: newReplicaId(entry.id),
        entryId: entry.id,
        path: filePath,
        fileName,
        folderPath: parentDirectory(filePath),
        checksum: fingerprint,
        fileSize: fingerprintResult?.byte_count ?? 0,
        mtime: 0,
        status,
        lastAnalyzedAt: at,
        lastError: analysis.lastError || "",
      };
      nextState.entries = [entry, ...nextState.entries];
      nextState.replicas = [replica, ...nextState.replicas];
    }
  }
  return persist ? await persistLibraryState(nextState) : nextState;
}

export async function loadLibrary(): Promise<LibraryState> {
  return enqueueLibraryWrite(async () => {
    const builtinPath = await builtinImportFolderPath().catch(() => "");
    let raw: RawLibraryState | null = null;
    try { raw = JSON.parse(await readUserConfigFile(LIB_PATH)) as RawLibraryState; }
    catch { raw = null; }
    const base = raw ?? { entries: [], replicas: [], folders: [] };
    // The built-in import folder is mounted by default and coerced back to
    // `builtin_import` (5.1.1.7). It is a derived-but-stable mount, so an absent
    // library file is not written just to add it; a migration/coercion commit is
    // still a single atomic write.
    const state = ensureBuiltinImportFolder(normalizeLibraryState(base), builtinPath);
    if (raw !== null && JSON.stringify(base) !== JSON.stringify(state)) {
      await writeUserConfigFile(LIB_PATH, JSON.stringify(state, null, 2));
    }
    loadedBaseline = JSON.stringify(state);
    return state;
  });
}

export async function saveLibrary(state: LibraryState): Promise<void> {
  await persistLibraryState(state);
}

export function projectionRecordAnalysis(parsed: any): Partial<ProjectionAnalysis> {
  const meta = parsed?.metadata || {};
  return {
    status: "ok",
    lastError: "",
    internalName: typeof meta.name === "string" ? meta.name : "",
    author: meta.author || "",
    description: typeof meta.description === "string" ? stripTagsSummaryFromDescription(meta.description) : "",
    totalBlocks: meta.total_blocks || 0,
    totalVolume: meta.total_volume || 0,
    regionCount: meta.region_count || 0,
    regionNames: Array.isArray(parsed?.regions) && parsed.regions.every((region: any) => typeof region?.name === "string") ? parsed.regions.map((region: any) => region.name) : undefined,
    enclosingSize: meta.enclosing_size || { x: 0, y: 0, z: 0 },
    minecraftDataVersion: meta.minecraft_data_version || 0,
  };
}

export async function analyzeFile(filePath: string): Promise<Partial<ProjectionAnalysis>> {
  try {
    const fileName = fileNameFromPath(filePath);
    return { ...projectionRecordAnalysis(await inspectProject({ sourcePath: filePath })), fileName, displayName: fileName };
  } catch (error: any) {
    return { status: "parse_error", lastError: String(error), fileName: fileNameFromPath(filePath) };
  }
}

/** Apply a fresh analysis result to the replica at `filePath` without persisting. */
export function applyRecordAnalysis(state: LibraryState, filePath: string, analysis: Partial<ProjectionAnalysis>): LibraryState {
  const next = normalizeLibraryState(state);
  const key = windowsPathKey(filePath);
  const replicaIndex = next.replicas.findIndex((replica) => windowsPathKey(replica.path) === key);
  if (replicaIndex < 0) return next;
  const replica = next.replicas[replicaIndex];
  const status: ReplicaStatus = analysis.status === "parse_error" ? "parse_error" : "active";
  next.replicas[replicaIndex] = { ...replica, status, lastAnalyzedAt: Date.now(), lastError: analysis.lastError || "" };
  const entryIndex = next.entries.findIndex((entry) => entry.id === replica.entryId);
  if (entryIndex >= 0) next.entries[entryIndex] = applyAnalysis(next.entries[entryIndex], {
    status: analysis.status || "ok",
    lastError: analysis.lastError || "",
    internalName: analysis.internalName || "",
    author: analysis.author || "",
    description: analysis.description || "",
    totalBlocks: analysis.totalBlocks || 0,
    totalVolume: analysis.totalVolume || 0,
    regionCount: analysis.regionCount || 0,
    regionNames: analysis.regionNames,
    enclosingSize: analysis.enclosingSize || { x: 0, y: 0, z: 0 },
    minecraftDataVersion: analysis.minecraftDataVersion || 0,
    fileName: replica.fileName,
    displayName: replica.fileName,
  });
  return next;
}

export function markReplicaMissing(state: LibraryState, filePath: string): LibraryState {
  const next = normalizeLibraryState(state);
  const key = windowsPathKey(filePath);
  // Only an `active` replica is eligible to become `missing`; `trashed` / `parse_error`
  // replicas are protected from this transition (A.3 recycle-bin state machine).
  return { ...next, replicas: next.replicas.map((replica) => windowsPathKey(replica.path) === key && replica.status === "active" ? { ...replica, status: "missing" as ReplicaStatus } : replica) };
}

async function addOrUpdateRecordInternal(state: LibraryState, filePath: string, persist: boolean): Promise<LibraryState> {
  const exists = await checkFileExists(filePath);
  const nextState = normalizeLibraryState(state);
if (!exists) {
    const key = windowsPathKey(filePath);
    const hasReplica = nextState.replicas.some((replica) => windowsPathKey(replica.path) === key);
    if (!hasReplica) return nextState;
    const missing = markReplicaMissing(nextState, filePath);
    return persist ? await persistLibraryState(missing) : missing;
  }
  const analysis = await analyzeFile(filePath);
  return await addOrUpdateRecordFromAnalysisInternal(nextState, filePath, analysis, persist);
}

export async function addOrUpdateRecord(state: LibraryState, filePath: string): Promise<LibraryState> {
  return await addOrUpdateRecordInternal(state, filePath, true);
}

export async function addOrUpdateRecordFromAnalysis(
  state: LibraryState,
  filePath: string,
  analysis: Partial<ProjectionAnalysis>,
): Promise<LibraryState> {
  return await addOrUpdateRecordFromAnalysisInternal(state, filePath, analysis, true);
}

/**
 * Activation semantics (裁决 B.3): a file that is not yet in the library is scanned
 * in (fingerprinted entry/replica via the merge kernel); a file already tracked simply
 * becomes the current / most-recent reference. Files under a `non_archived` folder are
 * excluded from the import and only record a `path` recent reference (5.1.1.5 / 6.3.4).
 */
export async function activateProjectionRecord(state: LibraryState, filePath: string): Promise<LibraryState> {
  let next = normalizeLibraryState(state);
  const underNonArchived = next.folders.some((folder) => folder.type === "non_archived" && isUnderFolder(filePath, folder.path, folder.recursive));
  if (!underNonArchived) {
    const key = windowsPathKey(filePath);
    if (!next.replicas.some((replica) => windowsPathKey(replica.path) === key)) {
      next = await addOrUpdateRecordInternal(next, filePath, false);
    }
  }
  const replica = next.replicas.find((item) => windowsPathKey(item.path) === windowsPathKey(filePath));
  const ref: RecentRef = underNonArchived || !replica ? { kind: "path", path: filePath } : { kind: "entry", entryId: replica.entryId };
  return await persistLibraryState(withRecent(next, ref));
}

function withRecent(state: LibraryState, ref: RecentRef): LibraryState {
  const key = ref.kind === "entry" ? `entry:${ref.entryId}` : `path:${windowsPathKey(ref.path || "")}`;
  const dedupKey = (candidate: RecentRef) => candidate.kind === "entry" ? `entry:${candidate.entryId}` : `path:${windowsPathKey(candidate.path || "")}`;
  return { ...state, recent: [ref, ...state.recent.filter((candidate) => dedupKey(candidate) !== key)].slice(0, RECENT_LIMIT) };
}

/**
 * Searches enabled local-library folders without adding matches to recent content.
 */
export async function searchLocalLibraryFolderRecords(state: LibraryState, query: string): Promise<LibraryRecordView[]> {
  const normalizedState = normalizeLibraryState(state);
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const existingByPath = new Map(libraryRecordViews(normalizedState).map((record) => [windowsPathKey(record.path), record]));
  const foundByPath = new Map<string, LibraryRecordView>();
  for (const folder of normalizedState.folders.filter((item) => item.enabled && item.type !== "non_archived")) {
    const entries = await listLitematicFileEntriesInDirectory(folder.path, folder.recursive);
    for (const entry of entries) {
      const fileName = entry.name || fileNameFromPath(entry.path);
      const searchable = `${fileName} ${entry.path}`.toLowerCase();
      if (!searchable.includes(needle)) continue;
      foundByPath.set(windowsPathKey(entry.path), existingByPath.get(windowsPathKey(entry.path)) || recordViewFromDirectoryEntry(entry));
    }
  }
  return [...foundByPath.values()].sort((left, right) => left.fileName.localeCompare(right.fileName));
}

function recordViewFromDirectoryEntry(entry: DirectoryEntryInfo): LibraryRecordView {
  const fileName = fileNameFromPath(entry.path) || entry.name;
  return {
    id: entry.path,
    entryId: "",
    path: entry.path,
    fileName,
    displayName: fileName,
    author: "",
    description: "",
    tags: [],
    totalBlocks: 0,
    totalVolume: 0,
    regionCount: 0,
    enclosingSize: { x: 0, y: 0, z: 0 },
    minecraftDataVersion: 0,
    fileSize: entry.file_size || 0,
    mtime: entry.mtime_ms || 0,
    checksum: null,
    categoryIds: [],
    status: "ok",
    lastAnalyzedAt: entry.mtime_ms || 0,
    lastError: "",
    sort_index: 0,
  };
}

/** Absolute path of the built-in import folder (`projection-library/import`). */
export async function builtinImportFolderPath(): Promise<string> {
  return await getUserConfigFilePath(BUILTIN_IMPORT_RELATIVE_PATH);
}

/** True when `path` is the built-in import folder (by path identity). */
export function isBuiltinImportFolderPath(path: string, builtinPath: string): boolean {
  return !!builtinPath && folderKey(path) === folderKey(builtinPath);
}

/** Merges two alias records of the built-in import folder into one (fresh sync status wins). */
function mergeBuiltinFolderInto(base: LocalLibraryFolder, candidate: LocalLibraryFolder): LocalLibraryFolder {
  const latest = candidate.lastSyncedAt >= base.lastSyncedAt ? candidate : base;
  return {
    ...base,
    ...candidate,
    enabled: base.enabled && candidate.enabled,
    recursive: base.recursive || candidate.recursive,
    autoActivate: true,
    sort_index: Math.min(base.sort_index, candidate.sort_index),
    lastSyncedAt: latest.lastSyncedAt,
    lastScanCount: latest.lastScanCount,
    lastError: latest.lastError,
  };
}

/**
 * Ensures the built-in import folder is mounted as a `builtin_import` folder
 * (5.1.1.7, 5.1.3.2, 6.3.3). It is always tracked, never removable and can never
 * be set to non-archived; an existing alias at the fixed path is coerced back to
 * `builtin_import` (and re-created when the filesystem directory was removed).
 *
 * Every alias of the built-in import folder — the migration's **relative**
 * `projection-library/import` record, an old config-base absolute record, or any
 * `builtin_import`-typed folder — is folded into the single current absolute path
 * (`windowsPathKey` identity), so a migration leftover + derived absolute record do
 * not coexist as "two built-in import folders". The merge is idempotent (a second
 * pass is a no-op), preserves unknown fields and is deterministic (anchor = the
 * first candidate's position), and keeps `folderRefsFor` emitting only absolute paths.
 */
export function ensureBuiltinImportFolder(state: LibraryState, builtinPath: string): LibraryState {
  if (!builtinPath) return state;
  const key = folderKey(builtinPath);
  const legacyKey = folderKey(BUILTIN_IMPORT_RELATIVE_PATH);
  const isBuiltinCandidate = (folder: LocalLibraryFolder): boolean =>
    folder.type === "builtin_import" || folderKey(folder.path) === key || folderKey(folder.path) === legacyKey;
  const candidates = state.folders.filter(isBuiltinCandidate);
  if (!candidates.length) {
    return {
      ...state,
      folders: [
        ...state.folders,
        {
          path: builtinPath,
          type: "builtin_import",
          enabled: true,
          recursive: true,
          autoActivate: true,
          sort_index: state.folders.length,
          lastSyncedAt: 0,
          lastScanCount: 0,
          lastError: "",
        },
      ],
    };
  }
  const builtin: LocalLibraryFolder = {
    ...candidates.reduce(mergeBuiltinFolderInto),
    path: builtinPath,
    type: "builtin_import",
    autoActivate: true,
  };
  const folders: LocalLibraryFolder[] = [];
  let placed = false;
  for (const folder of state.folders) {
    if (isBuiltinCandidate(folder)) {
      if (!placed) { folders.push(builtin); placed = true; }
      continue;
    }
    folders.push(folder);
  }
  return { ...state, folders };
}

export async function addLocalLibraryFolder(state: LibraryState, folderPath: string): Promise<LibraryState> {
  const normalizedPath = folderPath.trim();
  if (!normalizedPath) return normalizeLibraryState(state);
  const builtinPath = await builtinImportFolderPath().catch(() => "");
  const isBuiltin = isBuiltinImportFolderPath(normalizedPath, builtinPath);
  const nextState = normalizeLibraryState(state);
  const key = folderKey(normalizedPath);
  if (!nextState.folders.some((folder) => folderKey(folder.path) === key)) {
    nextState.folders.push({
      path: normalizedPath,
      type: isBuiltin ? "builtin_import" : "tracked",
      enabled: true,
      recursive: true,
      autoActivate: true,
      sort_index: nextState.folders.length,
      lastSyncedAt: 0,
      lastScanCount: 0,
      lastError: "",
    });
  }
  return await persistLibraryState(nextState);
}

export async function updateLocalLibraryFolder(
  state: LibraryState,
  folderPath: string,
  patch: Partial<Omit<LocalLibraryFolder, "path">>,
): Promise<LibraryState> {
  const key = folderKey(folderPath);
  const builtinPath = await builtinImportFolderPath().catch(() => "");
  const isBuiltin = isBuiltinImportFolderPath(folderPath, builtinPath);
  // The built-in import folder must never become non-archived (5.1.1.6 / 6.3.3).
  if (isBuiltin && patch.type && patch.type !== "builtin_import") {
    throw new Error("内置导入文件夹不能设为非归档。");
  }
  const nextState = normalizeLibraryState(state);
  return await persistLibraryState({
    ...nextState,
    folders: nextState.folders.map((folder) => {
      if (folderKey(folder.path) !== key) return folder;
      const type: LibraryFolderType = folder.type === "builtin_import" ? "builtin_import" : (patch.type ?? folder.type);
      return {
        ...folder,
        ...patch,
        type,
        autoActivate: type === "builtin_import" ? true : (patch.autoActivate ?? folder.autoActivate),
        lastSyncedAt: patch.lastSyncedAt ?? folder.lastSyncedAt,
        lastScanCount: patch.lastScanCount ?? folder.lastScanCount,
        lastError: patch.lastError ?? folder.lastError,
      };
    }),
  });
}

export async function removeLocalLibraryFolder(state: LibraryState, folderPath: string): Promise<LibraryState> {
  const key = folderKey(folderPath);
  const builtinPath = await builtinImportFolderPath().catch(() => "");
  if (isBuiltinImportFolderPath(folderPath, builtinPath)) {
    throw new Error("内置导入文件夹不可移除。");
  }
  const nextState = normalizeLibraryState(state);
  return await persistLibraryState({ ...nextState, folders: nextState.folders.filter((folder) => folderKey(folder.path) !== key) });
}

/**
 * A.1 wiring fix: folder sync is no longer a bare file count — it delegates to the
 * C.2 scan & merge kernel (`scanLibraryFolders` → Rust `scan` + checksum merge), so a
 * refresh/sync discovers **new** files and merges them into entries. The external
 * result shape (`importedCount` / `scannedFolderCount` / `state` / `errors`) and the
 * per-folder `lastSyncedAt` / `lastScanCount` / `lastError` semantics are preserved.
 */
async function syncFoldersInternal(state: LibraryState, onlyFolderPath?: string): Promise<SyncLibraryFoldersResult> {
  const normalized = normalizeLibraryState(state);
  const targetKey = onlyFolderPath ? folderKey(onlyFolderPath) : "";
  const targets = normalized.folders.filter((folder) => {
    // Non-archived folders are never scanned (5.1.1.3 / 6.3.4).
    if (folder.type === "non_archived") return false;
    if (!folder.enabled && !targetKey) return false;
    if (!targetKey) return true;
    return folderKey(folder.path) === targetKey;
  });

  const outcome = await scanLibraryFolders(normalized, onlyFolderPath ? { onlyFolderPath } : {});
  const errors = [...outcome.errors, ...outcome.failures.map((failure) => `${failure.path}：${failure.reason}`)];
  return { importedCount: outcome.scannedCount, scannedFolderCount: targets.length, state: outcome.state, errors };
}

export async function syncLocalLibraryFolder(state: LibraryState, folderPath: string): Promise<SyncLibraryFoldersResult> {
  return await syncFoldersInternal(state, folderPath);
}

export async function syncAllLocalLibraryFolders(state: LibraryState): Promise<SyncLibraryFoldersResult> {
  return await syncFoldersInternal(state);
}

export async function setRecordPreview(state: LibraryState, filePath: string, previewPath: string): Promise<LibraryState> {
  const nextState = normalizeLibraryState(state);
  const key = windowsPathKey(filePath);
  const entryIds = new Set(nextState.replicas.filter((replica) => windowsPathKey(replica.path) === key).map((replica) => replica.entryId));
  return await persistLibraryState({
    ...nextState,
    entries: nextState.entries.map((entry) => entryIds.has(entry.id)
      ? { ...entry, preview: { ...entry.preview, path: previewPath, preview_image_path: previewPath, updatedAt: Date.now() } }
      : entry),
  });
}

export async function reorderLibraryRecords(state: LibraryState, orderedPaths: string[]): Promise<LibraryState> {
  const nextState = normalizeLibraryState(state);
  const order = new Map(orderedPaths.map((path, index) => [windowsPathKey(path), index]));
  const entryOrder = new Map<string, number>();
  for (const replica of nextState.replicas) {
    const index = order.get(windowsPathKey(replica.path));
    if (index !== undefined && !entryOrder.has(replica.entryId)) entryOrder.set(replica.entryId, index);
  }
  return await persistLibraryState({
    ...nextState,
    entries: nextState.entries.map((entry) => entryOrder.has(entry.id) ? { ...entry, sortIndex: entryOrder.get(entry.id)! } : entry),
  });
}

/** Follows the append-only ID redirect chain to the surviving entry id (5.3.1.8 / 6.7). */
export function resolveEntryId(state: LibraryState, id: string): string {
  return resolveRedirectChain(normalizeLibraryState(state), id);
}

/**
 * Applies successful batch-rename results to the library (D.11). Pure: file bytes do
 * not change, only the replica's file name / path move, so the entry's checksum and id
 * are untouched. The caller persists the returned state.
 */
export function applyReplicaRenames(state: LibraryState, renames: { replicaId: string; fileName: string }[]): LibraryState {
  const normalized = normalizeLibraryState(state);
  const byId = new Map(renames.map((item) => [item.replicaId, item.fileName]));
  const replicas = normalized.replicas.map((replica) => {
    const fileName = byId.get(replica.id);
    if (!fileName) return replica;
    const parent = parentDirectory(replica.path);
    const separator = replica.path.includes("\\") ? "\\" : "/";
    return { ...replica, fileName, folderPath: parent, path: parent ? `${parent}${separator}${fileName}` : fileName };
  });
  return normalizeLibraryState({ ...normalized, replicas });
}

/** Favorites follow the ID redirect chain and are de-duplicated on the surviving id (裁决 12). */
export function getFavorites(state: LibraryState): string[] {
  const normalized = normalizeLibraryState(state);
  return [...new Set(normalized.favorites.map((id) => resolveRedirectChain(normalized, id)))];
}

/**
 * Recent references (裁决 11/12): `entry` refs follow the redirect chain; `path`
 * refs do not (no entry id to follow) and stay de-duplicated by `windowsPathKey`.
 */
export function getRecent(state: LibraryState): RecentRef[] {
  const normalized = normalizeLibraryState(state);
  const seen = new Set<string>();
  const result: RecentRef[] = [];
  for (const ref of normalized.recent) {
    if (ref.kind === "entry") {
      if (!ref.entryId) continue;
      const resolved = resolveRedirectChain(normalized, ref.entryId);
      const key = `entry:${resolved}`;
      if (seen.has(key)) continue;
      seen.add(key);
      result.push({ kind: "entry", entryId: resolved });
    } else {
      if (!ref.path) continue;
      const key = `path:${windowsPathKey(ref.path)}`;
      if (seen.has(key)) continue;
      seen.add(key);
      result.push({ kind: "path", path: ref.path, checksum: ref.checksum });
    }
  }
  return result;
}

export function addRecentEntry(state: LibraryState, entryId: string): LibraryState {
  return withRecent(normalizeLibraryState(state), { kind: "entry", entryId });
}

export function addRecentPath(state: LibraryState, path: string, checksum?: string | null): LibraryState {
  return withRecent(normalizeLibraryState(state), { kind: "path", path, checksum: checksum ?? undefined });
}

/** Adds an entry to favorites (independent of `recent`), keyed on the surviving id (5.8.1.7c). */
export function addFavorite(state: LibraryState, entryId: string): LibraryState {
  const normalized = normalizeLibraryState(state);
  const resolved = resolveRedirectChain(normalized, entryId);
  if (normalized.favorites.includes(resolved)) return normalized;
  return { ...normalized, favorites: [...normalized.favorites, resolved] };
}

/** Removes an entry (and any redirect aliases) from favorites. */
export function removeFavorite(state: LibraryState, entryId: string): LibraryState {
  const normalized = normalizeLibraryState(state);
  const resolved = resolveRedirectChain(normalized, entryId);
  return { ...normalized, favorites: normalized.favorites.filter((id) => resolveRedirectChain(normalized, id) !== resolved) };
}

// ---------------------------------------------------------------------------
// C.1 background-task client surface (delegates to libraryTasks.ts)
// ---------------------------------------------------------------------------

export function submitLibraryTask(kind: LibraryTaskKind, params: unknown): Promise<string> {
  return submitTask(kind, params);
}

export function cancelLibraryTask(taskId: string): Promise<boolean> {
  return cancelTask(taskId);
}

export { subscribeTask as subscribeLibraryTask };

// ---------------------------------------------------------------------------
// C.2 scan & merge kernel
// ---------------------------------------------------------------------------

/** Lightweight per-file scan result produced by the Rust `scan` task (summary.files). */
export interface ScannedFileInfo {
  path: string;
  fileName: string;
  folderPath: string;
  fileSize: number;
  mtimeMs: number;
  checksum: string;
}

export interface CoveredFolder {
  path: string;
  recursive: boolean;
}

export interface ScanIntegrationStats {
  markedMissing: string[];
  newReplicas: number;
  revivedEntries: number;
  /** Entries whose metadata must be (re)parsed after the merge (new entry or changed/revived file). */
  pendingAnalysis: { entryId: string; path: string }[];
}

function isUnderFolder(filePath: string, folderPath: string, recursive: boolean): boolean {
  if (windowsPathKey(filePath) === windowsPathKey(folderPath)) return true;
  if (!recursive) return windowsPathKey(parentDirectory(filePath)) === windowsPathKey(folderPath);
  const fileKey = windowsPathKey(filePath);
  const folderKey = windowsPathKey(folderPath);
  return fileKey.startsWith(`${folderKey}\\`) || fileKey.startsWith(`${folderKey}/`);
}

/**
 * Pure scan-merge kernel (C.2, design 2.1.4(1)):
 * 1. resolve each scanned file to a checksum (TS-side `size+mtime` cache reuse when allowed);
 * 2. merge by checksum into the active-entry index (covers `retired` for revival, excludes `merged`);
 * 3. upsert replicas keyed by `windowsPathKey(path)`;
 * 4. mark tracked replicas that did not appear this round as `missing` (never delete records).
 */
export function integrateScannedFiles(
  state: LibraryState,
  files: ScannedFileInfo[],
  options: { coveredFolders: CoveredFolder[]; useCache: boolean; forceRecheck: boolean; now: number; noAutoImportFolders?: CoveredFolder[] },
): { state: LibraryState; stats: ScanIntegrationStats } {
  const entries = state.entries.map((entry) => ({ ...entry, categoryIds: [...entry.categoryIds], tags: [...entry.tags], preview: { ...entry.preview }, stats: { ...entry.stats } }));
  const replicas = state.replicas.map((replica) => ({ ...replica }));
  const checksumIndex = buildChecksumIndex({ ...state, entries, replicas });
  const replicaByPath = new Map<string, Replica>();
  for (const replica of replicas) replicaByPath.set(windowsPathKey(replica.path), replica);
  const presentKeys = new Set(files.map((file) => windowsPathKey(file.path)));
  const stats: ScanIntegrationStats = { markedMissing: [], newReplicas: 0, revivedEntries: 0, pendingAnalysis: [] };
  // One representative path per entry: a new entry or a changed/revived file needs a
  // metadata (re)parse, but an already-tracked unchanged file is left untouched (C.2).
  const pendingByEntry = new Map<string, string>();

  for (const file of files) {
    const key = windowsPathKey(file.path);
    const existing = replicaByPath.get(key) || null;
    // B.1: a folder whose "auto-activate" switch is off does not import *new* files;
    // already-tracked replicas (and missing marking) are still processed normally.
    if (!existing && options.noAutoImportFolders?.some((folder) => isUnderFolder(file.path, folder.path, folder.recursive))) continue;
    let checksum = file.checksum;
    // TS-side cache: reuse the stored fingerprint when size+mtime are unchanged.
    if (
      options.useCache && !options.forceRecheck && existing && existing.checksum &&
      existing.fileSize === file.fileSize && existing.mtime === file.mtimeMs
    ) {
      checksum = existing.checksum;
    }
    if (!checksum) continue;

    let targetEntryId = checksumIndex.get(checksum);
    let createdEntryId: string | null = null;
    if (!targetEntryId) {
      const existingEntry = existing ? entries.find((entry) => entry.id === existing.entryId) : undefined;
      if (existingEntry && (existingEntry.checksum === null || existingEntry.checksum === checksum)) {
        // Fill a migrated entry's missing fingerprint instead of creating a duplicate.
        targetEntryId = existingEntry.id;
        existingEntry.checksum = checksum;
        if (existingEntry.status === "retired") { existingEntry.status = "active"; stats.revivedEntries += 1; }
      } else {
        targetEntryId = newEntryId();
        createdEntryId = targetEntryId;
        entries.unshift({
          id: targetEntryId,
          createdAt: options.now,
          checksum,
          status: "active",
          categoryIds: [],
          displayName: file.fileName || fileNameFromPath(file.path),
          internalName: undefined,
          author: "",
          description: "",
          tags: [],
          preview: {},
          stats: { totalBlocks: 0, totalVolume: 0, regionCount: 0, enclosingSize: { x: 0, y: 0, z: 0 }, minecraftDataVersion: 0 },
          sortIndex: entries.length,
          lastAnalyzedAt: options.now,
          lastError: "",
        });
      }
      checksumIndex.set(checksum, targetEntryId);
    } else {
      const target = entries.find((entry) => entry.id === targetEntryId);
      if (target && target.status === "retired") { target.status = "active"; stats.revivedEntries += 1; }
    }

    const fileName = file.fileName || fileNameFromPath(file.path);
    const folderPath = file.folderPath || parentDirectory(file.path);
    if (existing) {
      // A file is "changed" when its fingerprint inputs moved, its content fingerprint
      // differs (incl. a migrated `null`), it was `missing` and reappeared, or the caller
      // forced a full recheck.
      const changed = options.forceRecheck
        || existing.fileSize !== file.fileSize
        || existing.mtime !== file.mtimeMs
        || existing.checksum !== checksum
        || existing.status === "missing";
      existing.entryId = targetEntryId;
      existing.path = file.path;
      existing.fileName = fileName;
      existing.folderPath = folderPath;
      existing.checksum = checksum;
      existing.fileSize = file.fileSize;
      existing.mtime = file.mtimeMs;
      existing.status = "active";
      existing.lastAnalyzedAt = options.now;
      if (createdEntryId === targetEntryId || changed) pendingByEntry.set(targetEntryId, file.path);
    } else {
      const replica: Replica = {
        id: newReplicaId(targetEntryId),
        entryId: targetEntryId,
        path: file.path,
        fileName,
        folderPath,
        checksum,
        fileSize: file.fileSize,
        mtime: file.mtimeMs,
        status: "active",
        lastAnalyzedAt: options.now,
        lastError: "",
      };
      replicas.push(replica);
      replicaByPath.set(key, replica);
      stats.newReplicas += 1;
      if (createdEntryId === targetEntryId) pendingByEntry.set(targetEntryId, file.path);
    }
  }

  stats.pendingAnalysis = [...pendingByEntry.entries()].map(([entryId, path]) => ({ entryId, path }));

  // Missing marking is scoped to the covered folders so an incremental scan never
  // hides replicas that live in folders this round did not visit (5.1.3.1).
  for (const replica of replicas) {
    if (replica.status !== "active") continue;
    if (presentKeys.has(windowsPathKey(replica.path))) continue;
    const covered = options.coveredFolders.some((folder) => isUnderFolder(replica.path, folder.path, folder.recursive));
    if (!covered) continue;
    replica.status = "missing";
    stats.markedMissing.push(replica.id);
  }

  return { state: { ...state, entries, replicas }, stats };
}

export interface ScanScope {
  kind: "all" | "folder" | "entry" | "replica";
  folderPath?: string;
  entryId?: string;
  replicaId?: string;
}

export interface ScanOptions {
  onlyFolderPath?: string;
  forceRecheck?: boolean;
  useCache?: boolean;
  scope?: ScanScope;
  /** Progress relay for the underlying `scan` task (`processed` / `total`). */
  onProgress?: (progress: { processed: number; total: number }) => void;
}

export interface ScanOutcome {
  state: LibraryState;
  status: "completed" | "cancelled" | "failed";
  scannedCount: number;
  failures: { path: string; reason: string }[];
  markedMissing: string[];
  newReplicas: number;
  revivedEntries: number;
  moved: DirtyReplicaMove[];
  merges: { fromId: string; toId: string }[];
  errors: string[];
}

/**
 * True when `path` is an absolute path the Rust task scope check accepts. Relative
 * folder paths cannot be attributed and would fail the whole submission (4.3.1),
 * so they are never handed to `scan`.
 */
function isAbsoluteFolderPath(path: string): boolean {
  return /^[a-zA-Z]:[\\/]/.test(path) || /^\\\\/.test(path) || /^\//.test(path);
}

function folderRefsFor(state: LibraryState, onlyFolderPath?: string): { refs: LibraryFolderRef[]; covered: CoveredFolder[] } {
  const refs: LibraryFolderRef[] = [];
  const covered: CoveredFolder[] = [];
  for (const folder of state.folders) {
    if (!folder.enabled && !onlyFolderPath) continue;
    if (onlyFolderPath && windowsPathKey(folder.path) !== windowsPathKey(onlyFolderPath)) continue;
    // Only absolute paths are submittable; a relative leftover would reject the
    // whole `scan` at the command layer instead of being silently skipped.
    if (!isAbsoluteFolderPath(folder.path)) continue;
    const type = folder.type === "non_archived" ? "non_archive" : "tracked";
    refs.push({ path: folder.path, type });
    if (type === "tracked") covered.push({ path: folder.path, recursive: folder.recursive });
  }
  return { refs, covered };
}

async function checksumFiles(paths: string[], now: number): Promise<{ files: ScannedFileInfo[]; failures: { path: string; reason: string }[] }> {
  const files: ScannedFileInfo[] = [];
  const failures: { path: string; reason: string }[] = [];
  for (const path of paths) {
    try {
      const { sha256, byte_count } = await computeLitematicChecksum(path);
      files.push({ path, fileName: fileNameFromPath(path), folderPath: parentDirectory(path), fileSize: byte_count, mtimeMs: now, checksum: sha256 });
    } catch (error: any) {
      failures.push({ path, reason: String(error) });
    }
  }
  return { files, failures };
}

/**
 * Scans tracked/built-in-import folders and merges the result into the library.
 * A full/folder scan runs on the Rust queue (`scan` task, with progress); entry/
 * replica granularity is expanded by TS into per-replica checksum commands because
 * those ids are only resolvable in the TS layer (裁决 7). The library is written
 * once, at the terminal event.
 */
export async function scanLibraryFolders(state: LibraryState, options: ScanOptions = {}): Promise<ScanOutcome> {
  let normalized = normalizeLibraryState(state);
  const scope: ScanScope = options.scope
    ?? (options.onlyFolderPath ? { kind: "folder", folderPath: options.onlyFolderPath } : { kind: "all" });
  const useCache = options.useCache ?? true;
  const forceRecheck = options.forceRecheck ?? false;

  let files: ScannedFileInfo[] = [];
  let failures: { path: string; reason: string }[] = [];
  let covered: CoveredFolder[] = [];
  let status: ScanOutcome["status"] = "completed";
  const errors: string[] = [];
  let noAutoImportFolders: CoveredFolder[] = [];

  if (scope.kind === "entry" || scope.kind === "replica") {
    const replicas = scope.kind === "replica"
      ? normalized.replicas.filter((replica) => replica.id === scope.replicaId)
      : entryActiveReplicas(normalized, scope.entryId || "");
    const computed = await checksumFiles(replicas.map((replica) => replica.path), Date.now());
    files = computed.files;
    failures = computed.failures;
    covered = [];
  } else {
    // The built-in import folder is a derived mount: fold every alias onto the current
    // absolute path and (re)create its directory before building the scan refs, so a
    // relative leftover is never submitted to Rust (4.3.1) and an externally deleted
    // folder does not surface as `directory not found` (5.1.3.2). Regular `tracked`
    // folders are NOT auto-created and a missing one still reports a per-folder failure.
    if (normalized.folders.some((folder) => folder.type === "builtin_import")) {
      const builtinPath = await builtinImportFolderPath().catch(() => "");
      if (builtinPath) normalized = ensureBuiltinImportFolder(normalized, builtinPath);
    }
    const { refs, covered: coveredFolders } = folderRefsFor(normalized, options.onlyFolderPath);
    // B.1: folders whose "auto-activate" switch is off keep tracking existing replicas
    // but do not import their new files; the kernel filters those files out.
    noAutoImportFolders = coveredFolders.filter((folder) => {
      const record = normalized.folders.find((item) => folderKey(item.path) === folderKey(folder.path));
      return record ? record.autoActivate === false : false;
    });
    const scopePayload = scope.kind === "folder" ? { kind: "folder", folderPath: scope.folderPath || options.onlyFolderPath || "" } : { kind: "all" };
    const finished = await submitAndAwait(
      "scan",
      { folders: refs, useCache, forceRecheck, scope: scopePayload },
      options.onProgress ? { onProgress: (payload) => options.onProgress!({ processed: payload.processed, total: payload.total }) } : {},
    );
    status = finished.status;
    failures = finished.failures;
    // Missing marking is only sound when the whole scan finished: a cancelled or
    // failed task did not visit every file, so unvisited replicas must stay active.
    covered = status === "completed" ? coveredFolders : [];
    const summaryFiles = Array.isArray(finished.summary?.files) ? (finished.summary.files as any[]) : [];
    files = summaryFiles
      .map((file) => ({
        path: String(file.path ?? ""),
        fileName: String(file.fileName ?? ""),
        folderPath: String(file.folderPath ?? ""),
        fileSize: Number(file.fileSize ?? 0),
        mtimeMs: Number(file.mtimeMs ?? 0),
        checksum: String(file.checksum ?? ""),
      }))
      .filter((file) => file.path && file.checksum);
    if (status === "failed") errors.push(String(finished.summary?.message ?? "扫描任务失败"));
  }

  const integration = integrateScannedFiles(normalized, files, { coveredFolders: covered, useCache, forceRecheck, now: Date.now(), noAutoImportFolders });
  // Backfill `EntryStats` only for entries that are new or whose file changed/was
  // revived (C.2): a fresh/cached analysis overwrites the zeroed placeholder. A parse
  // failure marks that replica `parse_error` and does NOT fail the whole scan.
  let integratedState = integration.state;
  const pending = integration.stats.pendingAnalysis;
  if (pending.length) {
    const analyses = await Promise.all(pending.map((item) => analyzeFile(item.path)));
    pending.forEach((item, index) => { integratedState = applyRecordAnalysis(integratedState, item.path, analyses[index]); });
  }
  const reconciled = reconcileDirtyReplicas(integratedState);
  const merged = autoMergeByChecksum(reconciled.state, Date.now());
  const stamped: LibraryState = {
    ...merged.state,
    folders: merged.state.folders.map((folder) => {
      if (status !== "completed") return folder;
      if (options.onlyFolderPath && windowsPathKey(folder.path) !== windowsPathKey(options.onlyFolderPath)) return folder;
      if (scope.kind !== "all" && scope.kind !== "folder") return folder;
      return { ...folder, lastSyncedAt: Date.now(), lastScanCount: files.filter((file) => isUnderFolder(file.path, folder.path, folder.recursive)).length, lastError: failures.map((failure) => `${failure.path}：${failure.reason}`).join("；") };
    }),
  };
  const persisted = await persistLibraryState(stamped);
  return {
    state: persisted,
    status,
    scannedCount: files.length,
    failures,
    markedMissing: integration.stats.markedMissing,
    newReplicas: integration.stats.newReplicas,
    revivedEntries: integration.stats.revivedEntries,
    moved: reconciled.moved,
    merges: merged.merges,
    errors,
  };
}

// ---------------------------------------------------------------------------
// C.3 content-change orchestration (sync / diverge)
// ---------------------------------------------------------------------------

export interface ContentChangeOutcome {
  state: LibraryState;
  ok: boolean;
  failures: { path: string; reason: string }[];
}

function primaryActiveReplica(state: LibraryState, entryId: string): Replica | null {
  const replicas = entryActiveReplicas(state, entryId).sort((left, right) => (left.id < right.id ? -1 : left.id > right.id ? 1 : 0));
  return replicas[0] || null;
}

/** Copies the source replica's bytes over every other active replica of the entry. */
async function overwriteReplicas(source: Replica, targets: Replica[]): Promise<{ synced: Replica[]; failures: { path: string; reason: string }[] }> {
  const synced: Replica[] = [];
  const failures: { path: string; reason: string }[] = [];
  for (const target of targets) {
    try {
      await copyFileToDirectory(source.path, parentDirectory(target.path), target.fileName, true);
      synced.push(target);
    } catch (error: any) {
      failures.push({ path: target.path, reason: String(error) });
    }
  }
  return { synced, failures };
}

/**
 * Sync (5.4.1.3): overwrite all active replicas with the changed replica's new
 * bytes, recompute once and update the entry fingerprint — the entry ID is unchanged.
 * Partial failures are reported but do not roll back the replicas that succeeded.
 */
export async function syncEntryContent(state: LibraryState, entryId: string, sourceReplicaId?: string): Promise<ContentChangeOutcome> {
  const normalized = normalizeLibraryState(state);
  const entry = normalized.entries.find((item) => item.id === entryId);
  if (!entry) throw new Error("条目不存在，请刷新后重试。");
  const source = sourceReplicaId ? normalized.replicas.find((replica) => replica.id === sourceReplicaId) : primaryActiveReplica(normalized, entryId);
  if (!source) throw new Error("没有可用于同步的活跃副本。");
  const { sha256 } = await computeLitematicChecksum(source.path);
  const targets = entryActiveReplicas(normalized, entryId).filter((replica) => replica.id !== source.id);
  const { failures } = await overwriteReplicas(source, targets);
  const synced = applyContentSync(normalized, entryId, sha256);
  const persisted = await persistLibraryState(synced);
  return { state: persisted, ok: failures.length === 0, failures };
}

/**
 * Diverge (5.4.1.4): move the changed replica to the entry owning its new
 * checksum (existing or new); the source entry keeps its other replicas and is
 * retired when none remain active. No files are touched.
 */
export async function divergeEntryContent(state: LibraryState, replicaId: string): Promise<ContentChangeOutcome> {
  let normalized = normalizeLibraryState(state);
  const replica = normalized.replicas.find((item) => item.id === replicaId);
  if (!replica) throw new Error("副本不存在，请刷新后重试。");
  const { sha256 } = await computeLitematicChecksum(replica.path);
  normalized = { ...normalized, replicas: normalized.replicas.map((item) => item.id === replicaId ? { ...item, checksum: sha256 } : item) };
  const diverged = applyContentDiverge(normalized, replicaId, Date.now());
  const persisted = await persistLibraryState(diverged ? diverged.state : normalized);
  return { state: persisted, ok: true, failures: [] };
}

// ---------------------------------------------------------------------------
// C.7 non-archived manual activation
// ---------------------------------------------------------------------------

/** Browses a non-archived folder file: records a `path` recent ref only, no entry/replica (5.1.1.5 / 6.3.4). */
export async function activateNonArchivedPath(state: LibraryState, path: string): Promise<LibraryState> {
  return await persistLibraryState(withRecent(normalizeLibraryState(state), { kind: "path", path }));
}

// ---------------------------------------------------------------------------
// C.8 tags & description write-back (write file -> sync -> recompute, ID stable)
// ---------------------------------------------------------------------------

async function writeEntryMetadata(state: LibraryState, entryId: string, patch: Record<string, unknown>, mutate: (entry: LibraryEntry) => LibraryEntry): Promise<LibraryState> {
  const normalized = normalizeLibraryState(state);
  const entry = normalized.entries.find((item) => item.id === entryId);
  if (!entry) throw new Error("条目不存在，请刷新后重试。");
  const source = primaryActiveReplica(normalized, entryId);
  if (!source) throw new Error("条目没有活跃副本，无法写回文件内元数据。");
  // 1. write the file-internal field via Rust `edit_metadata` (unknown fields preserved).
  await editProjectMetadata({ sourcePath: source.path, patch, outputPath: null });
  // 2. recompute the source fingerprint, then propagate the new bytes to every other active replica.
  const { sha256 } = await computeLitematicChecksum(source.path);
  const targets = entryActiveReplicas(normalized, entryId).filter((replica) => replica.id !== source.id);
  const { failures } = await overwriteReplicas(source, targets);
  // 3. update entry fingerprint (ID unchanged) + mirror the field, best-effort copy report.
  const synced = applyContentSync(normalized, entryId, sha256);
  const updated: LibraryState = {
    ...synced,
    entries: synced.entries.map((item) => item.id === entryId ? { ...mutate(item), lastError: failures.length ? `部分副本未同步：${failures.map((failure) => failure.path).join("、")}` : "" } : item),
  };
  return await persistLibraryState(updated);
}

/** Writes `Metadata.NovaTags` (List<String>) into the file and mirrors it on the entry (5.7 / 6.1.5). */
export async function setEntryTags(state: LibraryState, entryId: string, tags: string[]): Promise<LibraryState> {
  const clean = [...new Set(tags.map((tag) => tag.trim()).filter(Boolean))];
  return await writeEntryMetadata(state, entryId, { tags: clean }, (entry) => ({ ...entry, tags: clean }));
}

/** Writes `Metadata.Description` into the file; the entry description is not a content change. */
export async function setEntryDescription(state: LibraryState, entryId: string, description: string): Promise<LibraryState> {
  return await writeEntryMetadata(state, entryId, { description }, (entry) => ({ ...entry, description }));
}

// ---------------------------------------------------------------------------
// C.9 reposition a missing replica
// ---------------------------------------------------------------------------

export type RepositionOutcome =
  | { kind: "repositioned"; state: LibraryState }
  | { kind: "invalid"; state: LibraryState }
  | { kind: "merged"; state: LibraryState; occupiedByEntryId: string }
  | { kind: "conflict"; entryId: string; occupiedByEntryId: string; targetChecksum: string; occupantChecksum: string | null };

export interface RepositionOptions {
  /** On a checksum mismatch with the occupying record: `reject` (default) keeps `missing`; `relocate` joins the occupant entry. */
  onConflict?: "reject" | "relocate";
}

/**
 * Repositions a `missing` replica onto a new path (5.9.1.3 / 5.9.3.2 / 裁决 10).
 * A valid target restores `active`; an invalid one stays `missing` (no bad path is
 * written). If the target is already occupied by another record, checksum decides:
 * equal → merge into the occupant entry; different → return a conflict for the user
 * to reject or relocate (unless `onConflict: "relocate"`).
 */
export async function repositionReplica(state: LibraryState, replicaId: string, newPath: string, options: RepositionOptions = {}): Promise<RepositionOutcome> {
  const normalized = normalizeLibraryState(state);
  const replica = normalized.replicas.find((item) => item.id === replicaId);
  if (!replica) throw new Error("副本不存在，请刷新后重试。");
  const exists = await checkFileExists(newPath);
  if (!exists) return { kind: "invalid", state: normalized };

  const key = windowsPathKey(newPath);
  const occupant = normalized.replicas.find((item) => item.id !== replicaId && windowsPathKey(item.path) === key) || null;
  const targetChecksum = (await computeLitematicChecksum(newPath)).sha256;

  if (occupant) {
    const occupantEntry = normalized.entries.find((entry) => entry.id === occupant.entryId);
    const occupantChecksum = occupantEntry?.checksum ?? null;
    const sameContent = !!occupantChecksum && occupantChecksum === targetChecksum;
    if (!sameContent && options.onConflict !== "relocate") {
      return { kind: "conflict", entryId: replica.entryId, occupiedByEntryId: occupant.entryId, targetChecksum, occupantChecksum };
    }
    const record = occupantChecksum && occupantChecksum === targetChecksum ? targetChecksum : occupantChecksum || targetChecksum;
    const replicas = normalized.replicas.map((item) => item.id === replicaId
      ? { ...item, entryId: occupant.entryId, path: newPath, fileName: fileNameFromPath(newPath), folderPath: parentDirectory(newPath), checksum: record, status: "active" as ReplicaStatus }
      : item);
    const entries = retiresWithoutActiveReplicas(normalized.entries, replicas);
    const persisted = await persistLibraryState({ ...normalized, entries, replicas });
    return { kind: "merged", state: persisted, occupiedByEntryId: occupant.entryId };
  }

  const replicas = normalized.replicas.map((item) => item.id === replicaId
    ? { ...item, path: newPath, fileName: fileNameFromPath(newPath), folderPath: parentDirectory(newPath), checksum: targetChecksum, status: "active" as ReplicaStatus }
    : item);
  const entries = normalized.entries.map((entry) => entry.id === replica.entryId && entry.checksum === null ? { ...entry, checksum: targetChecksum } : entry);
  const persisted = await persistLibraryState({ ...normalized, entries, replicas });
  return { kind: "repositioned", state: persisted };
}

// ---------------------------------------------------------------------------
// C.10 redundancy audit (read-only)
// ---------------------------------------------------------------------------

/** Read-only per-entry replica distribution / duplicate counts (5.5.1.1), decoupled from dispatch. */
export function auditLibraryRedundancy(state: LibraryState): RedundancyReport {
  return auditRedundancy(normalizeLibraryState(state));
}

// ---------------------------------------------------------------------------
// C.11 trash record maintenance (+ B.8 physical operations)
// ---------------------------------------------------------------------------

function trashEntryFor(state: LibraryState, replica: Replica, trashPath: string): LibraryTrashEntry {
  const entry = state.entries.find((item) => item.id === replica.entryId);
  const at = Date.now();
  return {
    id: trashId(),
    replicaSnapshot: { ...replica },
    originalPath: replica.path,
    originalFileName: replica.fileName,
    trashPath,
    movedAt: at,
    status: "in_trash",
    deletedAt: at,
    name: replica.fileName || entry?.displayName || "",
    kind: "projection",
    records: entry ? [toRecordView(entry, replica)] : [],
    categories: [],
  };
}

/**
 * Moves active replicas physically into the trash directory and records one
 * `LibraryTrashEntry` per successfully moved file (5.6.1 / 4.3.2). Trashed replicas
 * no longer count as members or duplicates; emptied entries retire.
 */
/**
 * Moves replicas out of the library (5.6.1 / 4.3.2). `active` (and `parse_error`)
 * replicas are physically moved into the trash directory and recorded; `missing`
 * replicas have no file on disk, so their library record is removed outright and
 * reported via `removedRecords` (never a silent no-op). Unknown ids are surfaced as
 * `record-not-found` failures, and trashed-byte replicas no longer count as members
 * or duplicates; emptied entries retire.
 */
export async function moveReplicasToTrash(state: LibraryState, replicaIds: string[]): Promise<{ state: LibraryState; moved: string[]; removedRecords: string[]; failures: { path: string; reason: string }[] }> {
  const normalized = normalizeLibraryState(state);
  const idSet = new Set(replicaIds);
  const candidates = normalized.replicas.filter((replica) => idSet.has(replica.id));
  const physical = candidates.filter((replica) => replica.status === "active" || replica.status === "parse_error");
  const recordOnly = candidates.filter((replica) => replica.status === "missing");
  const foundIds = new Set(candidates.map((replica) => replica.id));
  const failures: { path: string; reason: string }[] = [...idSet].filter((id) => !foundIds.has(id)).map((id) => ({ path: id, reason: "record-not-found" }));
  const removedRecords: string[] = recordOnly.map((replica) => replica.id);

  const results: TrashMoveResult[] = physical.length ? await libraryTrashMove(physical.map((replica) => ({ sourcePath: replica.path, trashName: replica.fileName }))) : [];
  const resultBySource = new Map(results.map((result) => [windowsPathKey(result.sourcePath), result]));
  const trashEntries: LibraryTrashEntry[] = [];
  const moved: string[] = [];
  const recordOnlyIds = new Set(removedRecords);
  const physicalIds = new Set(physical.map((replica) => replica.id));
  const replicas: Replica[] = [];
  for (const replica of normalized.replicas) {
    if (recordOnlyIds.has(replica.id)) continue; // missing file: drop the library record only
    if (!physicalIds.has(replica.id)) { replicas.push(replica); continue; }
    const result = resultBySource.get(windowsPathKey(replica.path));
    if (!result || !result.ok) {
      failures.push({ path: replica.path, reason: result?.reason || "移入回收区失败" });
      replicas.push(replica);
      continue;
    }
    trashEntries.push(trashEntryFor(normalized, replica, result.trashPath));
    moved.push(replica.id);
    replicas.push({ ...replica, status: "trashed" as ReplicaStatus });
  }
  const entries = retiresWithoutActiveReplicas(normalized.entries, replicas);
  const persisted = await persistLibraryState({ ...normalized, entries, replicas, trash: [...normalized.trash, ...trashEntries] });
  return { state: persisted, moved, removedRecords, failures };
}

/** Restores one trashed file back to its original path; `legacy` records are not restorable (裁决 6). */
export async function restoreLibraryTrashAsync(state: LibraryState, trashId: string, renameTo?: string): Promise<{ state: LibraryState; ok: boolean; reason?: string }> {
  const normalized = normalizeLibraryState(state);
  const trashEntry = normalized.trash.find((item) => item.id === trashId);
  if (!trashEntry) return { state: normalized, ok: false, reason: "missing-record" };
  if (trashEntry.status === "legacy" || !trashEntry.trashPath) return { state: normalized, ok: false, reason: "legacy" };
  const targetPath = renameTo ? `${parentDirectory(trashEntry.originalPath)}\\${renameTo}` : trashEntry.originalPath;
  const [result] = await libraryTrashRestore([{ trashPath: trashEntry.trashPath, targetPath, renameTo: renameTo ?? null }]);
  if (!result || !result.ok) return { state: normalized, ok: false, reason: result?.reason || "restore-failed" };

  const restoredPath = result.targetPath || targetPath;
  const replicaId = trashEntry.replicaSnapshot?.id;
  const replicas = normalized.replicas.map((replica) => {
    if (replica.id !== replicaId) return replica;
    return { ...replica, path: restoredPath, fileName: fileNameFromPath(restoredPath), folderPath: parentDirectory(restoredPath), status: "active" as ReplicaStatus };
  });
  const activeByEntry = new Set(replicas.filter((replica) => replica.status === "active").map((replica) => replica.entryId));
  const entries = normalized.entries.map((entry) => activeByEntry.has(entry.id) && entry.status === "retired" ? { ...entry, status: "active" as EntryStatus } : entry);
  const persisted = await persistLibraryState({ ...normalized, entries, replicas, trash: normalized.trash.filter((item) => item.id !== trashId) });
  return { state: persisted, ok: true };
}

/**
 * Permanently removes trash records, physically deleting their files when present
 * (5.6.3.2). `legacy` records have no physical file and are simply cleared; items
 * that cannot be deleted are kept and reported.
 */
export async function purgeLibraryTrashAsync(state: LibraryState, trashIds?: string[]): Promise<{ state: LibraryState; failures: { trashPath: string; reason: string }[] }> {
  const normalized = normalizeLibraryState(state);
  const targets = trashIds ? normalized.trash.filter((entry) => trashIds.includes(entry.id)) : normalized.trash;
  if (!targets.length) return { state: normalized, failures: [] };
  const physical = targets.filter((entry) => entry.status !== "legacy" && entry.trashPath);
  const results = physical.length ? await libraryTrashPurge(physical.map((entry) => ({ trashPath: entry.trashPath }))) : [];
  const resultByPath = new Map(results.map((result) => [result.trashPath, result]));
  const failures: { trashPath: string; reason: string }[] = [];
  const removable = new Set<string>();
  for (const entry of targets) {
    if (entry.status === "legacy" || !entry.trashPath) { removable.add(entry.id); continue; }
    const result = resultByPath.get(entry.trashPath);
    if (result?.ok) removable.add(entry.id);
    else failures.push({ trashPath: entry.trashPath, reason: result?.reason || "purge-failed" });
  }
  const persisted = await persistLibraryState({ ...normalized, trash: normalized.trash.filter((entry) => !removable.has(entry.id)) });
  return { state: persisted, failures };
}

// ---------------------------------------------------------------------------
// D.2 index client surface (delegates to libraryIndexClient.ts)
// ---------------------------------------------------------------------------

/**
 * Projects the library's entries into the flat, indexable shape the Rust
 * inverted index stores (design 2.4.3). Only active entries are indexed — the
 * view filters only ever show those. `fileName` comes from the entry's first
 * active replica; `categoryPath` joins the entry's category paths; `version` is
 * the data-version label (empty when unknown, so it adds no index noise).
 */
export function libraryIndexEntries(state: LibraryState): LibraryIndexEntry[] {
  const normalized = normalizeLibraryState(state);
  const replicasByEntry = new Map<string, Replica[]>();
  for (const replica of normalized.replicas) {
    const list = replicasByEntry.get(replica.entryId);
    if (list) list.push(replica);
    else replicasByEntry.set(replica.entryId, [replica]);
  }
  return normalized.entries
    .filter((entry) => entry.status === "active")
    .map((entry) => {
      const replicas = replicasByEntry.get(entry.id) ?? [];
      const primary = replicas.find((replica) => replica.status === "active") ?? replicas[0];
      return {
        entryId: entry.id,
        displayName: entry.displayName,
        fileName: primary?.fileName ?? "",
        author: entry.author,
        description: entry.description,
        tags: entry.tags,
        categoryPath: entry.categoryIds.map((id) => categoryPath(normalized, id)).filter(Boolean).join(" / "),
        version: entry.stats.minecraftDataVersion > 0 ? minecraftVersionLabel(entry.stats.minecraftDataVersion) : "",
      };
    });
}

/** Submits an incremental `index` build for the current library state. */
export async function refreshLibraryIndex(state: LibraryState): Promise<string> {
  return submitIndexBuild(libraryIndexEntries(state));
}

/** Read-only index lookup (design 2.2.2(C)); results are entry ids + readiness. */
export function queryLibraryIndex(query: LibraryIndexQuery): Promise<LibraryIndexQueryResult> {
  return queryIndex(query);
}

/** Subscribes to index readiness; refreshes when an `index` task finishes. */
export function subscribeIndexStatus(listener: (status: LibraryIndexStatus) => void): Promise<UnsubscribeIndexStatus> {
  return subscribeIndexStatusImpl(listener);
}

// ---------------------------------------------------------------------------
// E.1 built-in import folder + E.2 online-library landing
// ---------------------------------------------------------------------------

export interface BuiltinImportOutcome {
  state: LibraryState;
  copied: string[];
  skipped: string[];
  failures: { path: string; reason: string }[];
}

/**
 * Copies `.litematic` files into the built-in import folder and then merges them
 * by scan (E.1, 5.1.1.7b). The folder is created on demand by the copy primitive
 * (so an externally deleted folder is rebuilt — 5.1.3.2); a file already inside
 * the folder is skipped instead of re-copied.
 */
export async function importFilesToBuiltinFolder(state: LibraryState, filePaths: string[]): Promise<BuiltinImportOutcome> {
  const builtinPath = await builtinImportFolderPath();
  const normalized = ensureBuiltinImportFolder(normalizeLibraryState(state), builtinPath);
  const copied: string[] = [];
  const skipped: string[] = [];
  const failures: { path: string; reason: string }[] = [];
  for (const filePath of filePaths) {
    if (!/\.litematic$/i.test(filePath)) { failures.push({ path: filePath, reason: "不是 .litematic 文件" }); continue; }
    if (folderKey(parentDirectory(filePath)) === folderKey(builtinPath)) { skipped.push(filePath); continue; }
    try {
      await copyFileToDirectory(filePath, builtinPath, fileNameFromPath(filePath), false);
      copied.push(filePath);
    } catch (error: any) {
      failures.push({ path: filePath, reason: String(error) });
    }
  }
  await saveLibrary(normalized);
  const outcome = await scanLibraryFolders(normalized, { onlyFolderPath: builtinPath, useCache: true, forceRecheck: false });
  return { state: outcome.state, copied, skipped, failures: [...failures, ...outcome.failures] };
}

/**
 * Online-library landing (E.2 / 裁决 15): a downloaded projection is copied into
 * the built-in import folder and merged by a scan — the old "直接入库" semantics
 * are retired. `projection-library-record-imported` notification stays the same
 * and is emitted by the caller after this resolves.
 */
export async function importDownloadedToBuiltinFolder(state: LibraryState, path: string): Promise<LibraryState> {
  const outcome = await importFilesToBuiltinFolder(state, [path]);
  if (outcome.failures.length && !outcome.copied.length && !outcome.skipped.length) {
    throw new Error(outcome.failures[0].reason);
  }
  return outcome.state;
}

// ---------------------------------------------------------------------------
// E.5 packaged export (flat zip) — delegates to the `export-zip` background task
// ---------------------------------------------------------------------------

export interface ExportZipOptions {
  /** Preview image is optional; the projection body is always included. */
  includePreview: boolean;
  includeMaterialList: boolean;
  includeManifest: boolean;
  outputPath: string;
}

export interface ExportZipResult {
  status: "completed" | "cancelled" | "failed";
  outputPath: string;
  entryCount: number;
  files: string[];
  failures: { path: string; reason: string }[];
  errors: string[];
}

function sanitizeArchiveStem(name: string, fallback: string): string {
  const raw = (name || fallback || "projection").trim().replace(/\.litematic$/i, "");
  const safe = raw.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_").replace(/^[.\s]+/, "").trim();
  return safe || "projection";
}

/** Default `txt` material list, reusing `statsService` / `stats_api` (裁决 4). */
async function buildMaterialListText(filePath: string): Promise<string> {
  const data = await loadStructureStats(filePath, false);
  const lines: string[] = [];
  for (const material of data.materials) {
    if (material.materialStatus !== "item") continue;
    lines.push(`${material.name || material.id}\t${formatMaterialQuantity(material, material.totalCount)}`);
  }
  return lines.join("\r\n");
}

/**
 * Packages the given active entries into a flat zip through the single serial
 * `export-zip` task (E.5, 5.9.1.4–5.9.1.7). The projection body is always
 * included (`include.projection` is constant true, 裁决 16); preview / material
 * list / manifest are optional. TS owns the per-entry projection → zip projection
 * (flat archive stem, dedup) and the material-list text; Rust only writes bytes.
 */
export async function exportLibraryZip(state: LibraryState, entryIds: string[], options: ExportZipOptions): Promise<ExportZipResult> {
  const normalized = normalizeLibraryState(state);
  if (!options.outputPath.trim()) throw new Error("请选择导出目标路径。");
  const wanted = new Set(entryIds);
  const entries: LibraryExportEntry[] = [];
  const used = new Set<string>();
  for (const entry of normalized.entries) {
    if (entry.status !== "active" || !wanted.has(entry.id)) continue;
    const primary = entryActiveReplicas(normalized, entry.id)[0];
    if (!primary) continue;
    const stem = sanitizeArchiveStem(entry.displayName, primary.fileName);
    let candidate = stem;
    for (let counter = 2; used.has(candidate.toLowerCase()); counter += 1) candidate = `${stem}_${counter}`;
    used.add(candidate.toLowerCase());
    let previewPath: string | undefined;
    if (options.includePreview) {
      const preview = entry.preview.preview_image_path || entry.preview.path;
      if (preview && await checkFileExists(preview).catch(() => false)) previewPath = preview;
    }
    let materialListText: string | undefined;
    if (options.includeMaterialList) {
      materialListText = await buildMaterialListText(primary.path).catch(() => undefined);
    }
    entries.push({
      archiveName: candidate,
      sourcePath: primary.path,
      fileName: primary.fileName,
      displayName: entry.displayName || primary.fileName,
      author: entry.author,
      tags: [...entry.tags],
      checksum: entry.checksum,
      previewPath,
      materialListText,
    });
  }
  if (!entries.length) throw new Error("没有可导出的条目。");
  const params: LibraryExportZipParams = {
    outputPath: options.outputPath,
    exportedAt: new Date().toISOString(),
    includePreview: options.includePreview,
    includeMaterialList: options.includeMaterialList,
    includeManifest: options.includeManifest,
    entries,
  };
  const submitted = await libraryExportZip(params);
  const finished = await awaitLibraryTask(submitted.taskId);
  const summary = (finished.summary || {}) as Record<string, unknown>;
  return {
    status: finished.status,
    outputPath: String(summary.outputPath ?? options.outputPath),
    entryCount: Number(summary.entryCount ?? 0),
    files: Array.isArray(summary.files) ? (summary.files as string[]) : [],
    failures: finished.failures,
    errors: finished.status === "failed" ? [String(summary.message ?? "导出失败")] : [],
  };
}