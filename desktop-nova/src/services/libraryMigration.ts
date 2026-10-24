import { windowsPathKey } from "./filePathIdentity";
import { normalizeLibraryCategoryIcon } from "./libraryCategoryIcons";

/**
 * schemaVersion 4 → 5 migration (design 2.3.3).
 *
 * The migration is a **pure, deterministic** in-memory computation: the same input
 * always produces the same output (same IDs, same `createdAt`, same recent list),
 * so a failed write can be safely retried. It never touches the disk itself; the
 * single atomic commit is performed by `loadLibrary` in `libraryStore.ts` (A.4).
 *
 * This module intentionally keeps no runtime dependency on `libraryStore` (only
 * type-only imports are erased), so the two modules never form a runtime cycle.
 */

const BUILTIN_IMPORT_PATH = "projection-library/import";
const SCHEMA_VERSION = 5;
const RECENT_LIMIT = 20;

/** Detects pre-v5 library data that must be structurally migrated. */
export function needsLibraryMigration(raw: Record<string, any>): boolean {
  if (!raw || typeof raw !== "object") return true;
  // Old single-layer shape is a definitive signal.
  if (Array.isArray(raw.records)) return true;
  const version = raw.schemaVersion;
  if (typeof version === "number" && version >= SCHEMA_VERSION) return false;
  // No explicit version: only treat as legacy when the two-level shape is absent.
  return !(Array.isArray(raw.entries) || Array.isArray(raw.replicas));
}

function finite(value: unknown): number {
  return Number.isFinite(value) ? Number(value) : 0;
}

function stringOr(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function asRecord(value: unknown): Record<string, any> {
  return value && typeof value === "object" ? (value as Record<string, any>) : {};
}

function fileNameFromPath(filePath: string): string {
  return filePath.split(/[\\/]/).pop() || "";
}

function parentDirectory(path: string): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  const lastSeparatorIndex = Math.max(normalized.lastIndexOf("\\"), normalized.lastIndexOf("/"));
  if (lastSeparatorIndex < 0) return "";
  const parent = normalized.slice(0, lastSeparatorIndex);
  return /^(?:\\\\\?\\)?[a-z]:$/i.test(parent) ? `${parent}${normalized[lastSeparatorIndex]}` : parent;
}

/** Deterministic 64-bit FNV-1a, rendered as 16 hex chars. Sync, no async dependency. */
function fnv1a64Hex(input: string): string {
  let hash = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  const mask = 0xffffffffffffffffn;
  for (let index = 0; index < input.length; index++) {
    hash ^= BigInt(input.charCodeAt(index));
    hash = (hash * prime) & mask;
  }
  return hash.toString(16).padStart(16, "0");
}

/** Stable entry id derived from the path identity, so retries and revival reuse the same id. */
export function deterministicEntryId(path: string): string {
  return `entry:${fnv1a64Hex(windowsPathKey(path))}`;
}

/** Stable replica id derived from the path identity. */
export function deterministicReplicaId(path: string): string {
  return `replica:${fnv1a64Hex(`replica:${windowsPathKey(path)}`)}`;
}

/** Groups legacy records by path identity so aliases of the same file merge first (1.2.1). */
function groupRecordsByPath(records: any[]): any[][] {
  const groups = new Map<string, any[]>();
  for (const record of records) {
    if (!record || typeof record !== "object") continue;
    const key = windowsPathKey(String(asRecord(record).path ?? ""));
    const list = groups.get(key) || [];
    list.push(record);
    groups.set(key, list);
  }
  return [...groups.values()];
}

/** Legacy alias merge: newest successful analysis + newest preview + category union. */
function mergeLegacyRecords(aliases: any[]): any {
  const latest = [...aliases].sort((left, right) => finite(right.lastAnalyzedAt) - finite(left.lastAnalyzedAt))[0];
  const successful = aliases.filter((record) => record.status === "ok").sort((left, right) => finite(right.lastAnalyzedAt) - finite(left.lastAnalyzedAt))[0] || latest;
  const previews = aliases.filter((record) => record.preview_image_path || record.previewPath).sort((left, right) => finite(right.previewUpdatedAt) - finite(left.previewUpdatedAt));
  const preview = previews[0];
  return {
    ...Object.assign({}, ...[...aliases].reverse()),
    internalName: successful.internalName,
    author: successful.author,
    description: successful.description,
    totalBlocks: successful.totalBlocks,
    totalVolume: successful.totalVolume,
    regionCount: successful.regionCount,
    regionNames: successful.regionNames,
    enclosingSize: successful.enclosingSize,
    minecraftDataVersion: successful.minecraftDataVersion,
    fileSize: finite(latest.fileSize) || finite(aliases.find((record) => record.fileSize)?.fileSize),
    mtime: Math.max(...aliases.map((record) => finite(record.mtime))),
    status: latest.status,
    lastError: latest.lastError,
    lastAnalyzedAt: latest.lastAnalyzedAt,
    categoryIds: [...new Set(aliases.flatMap((record) => Array.isArray(record.categoryIds) ? record.categoryIds : []))],
    previewPath: preview?.previewPath || preview?.preview_image_path,
    preview_image_path: preview?.preview_image_path || preview?.previewPath,
    previewUpdatedAt: preview?.previewUpdatedAt,
    sort_index: Math.min(...aliases.map((record) => Number.isFinite(record.sort_index) ? Number(record.sort_index) : 0)),
  };
}

/** Category normalization only (no entry coupling): dedupe id, repair parent/cycles, icon safety. */
function normalizeLegacyCategories(raw: unknown): any[] {
  const categories: any[] = [];
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
      sort_index: Number.isFinite(candidate.sort_index) ? Number(candidate.sort_index) : index,
      icon: normalizeLibraryCategoryIcon(candidate.icon),
    });
  }
  for (const entry of categories) {
    if (!entry.parentId || !ids.has(entry.parentId)) { entry.parentId = null; continue; }
    const visited = new Set([entry.id]);
    let parent: any = entry;
    while (parent?.parentId) {
      if (visited.has(parent.parentId)) { entry.parentId = null; break; }
      visited.add(parent.parentId);
      parent = categories.find((item) => item.id === parent.parentId);
    }
  }
  return categories;
}

function dedupeLegacyFolders(rawFolders: any[]): any[] {
  const byKey = new Map<string, any>();
  for (const [index, folder] of rawFolders.entries()) {
    const path = stringOr(asRecord(folder).path).trim();
    if (!path) continue;
    const candidate = {
      ...folder,
      path,
      type: "tracked",
      enabled: folder?.enabled !== false,
      recursive: folder?.recursive !== false,
      sort_index: Number.isFinite(folder?.sort_index) ? Number(folder.sort_index) : index,
      lastSyncedAt: finite(folder?.lastSyncedAt),
      lastScanCount: finite(folder?.lastScanCount),
      lastError: stringOr(folder?.lastError),
    };
    const key = windowsPathKey(path);
    const previous = byKey.get(key);
    byKey.set(key, previous ? {
      ...candidate, ...previous,
      enabled: previous.enabled && candidate.enabled,
      recursive: previous.recursive || candidate.recursive,
      sort_index: Math.min(previous.sort_index, candidate.sort_index),
    } : candidate);
  }
  const folders = [...byKey.values()];
  folders.sort((left, right) => left.sort_index - right.sort_index || left.path.localeCompare(right.path));
  folders.forEach((folder, index) => { folder.sort_index = index; });
  return folders;
}

/** Flat view-shaped snapshot for a legacy trash record (kept for the transitional trash view). */
function legacyRecordView(record: any): Record<string, any> {
  const path = stringOr(record.path);
  return {
    id: stringOr(record.id) || deterministicReplicaId(path),
    entryId: "",
    path,
    fileName: stringOr(record.fileName) || fileNameFromPath(path),
    displayName: stringOr(record.displayName) || fileNameFromPath(path),
    author: stringOr(record.author),
    description: stringOr(record.description),
    tags: Array.isArray(record.tags) ? record.tags.filter((tag: unknown) => typeof tag === "string") : [],
    totalBlocks: finite(record.totalBlocks),
    totalVolume: finite(record.totalVolume),
    regionCount: finite(record.regionCount),
    regionNames: Array.isArray(record.regionNames) && record.regionNames.every((name: unknown) => typeof name === "string") ? [...record.regionNames] : undefined,
    enclosingSize: asRecord(record.enclosingSize),
    minecraftDataVersion: finite(record.minecraftDataVersion),
    fileSize: finite(record.fileSize),
    mtime: finite(record.mtime),
    checksum: null,
    categoryIds: Array.isArray(record.categoryIds) ? record.categoryIds.filter((id: unknown) => typeof id === "string") : [],
    status: record.status === "missing" ? "missing" : record.status === "parse_error" ? "parse_error" : "ok",
    lastAnalyzedAt: finite(record.lastAnalyzedAt),
    lastError: stringOr(record.lastError),
    sort_index: Number.isFinite(record.sort_index) ? Number(record.sort_index) : 0,
  };
}

/**
 * Maps a legacy single-layer library into the v5 two-level shape.
 * No content merge is performed (legacy data has no `checksum`); merging is deferred
 * to the first scan that backfills checksums (A.5 / C.2).
 */
export function migrateLibraryState(raw: Record<string, any>): Record<string, any> {
  const rawRecords = Array.isArray(raw.records) ? raw.records : [];
  const rawFolders = Array.isArray(raw.folders) ? raw.folders : [];
  const rawCategories = Array.isArray(raw.categories) ? raw.categories : [];
  const rawTrash = Array.isArray(raw.trash) ? raw.trash : [];

  const categories = normalizeLegacyCategories(rawCategories);
  const categoryIds = new Set<string>(categories.map((category) => category.id));

  const entries: any[] = [];
  const replicas: any[] = [];

  for (const [index, aliases] of groupRecordsByPath(rawRecords).entries()) {
    const merged = mergeLegacyRecords(aliases);
    const path = stringOr(merged.path);
    const sourceCategoryIds: unknown[] = Array.isArray(merged.categoryIds) ? merged.categoryIds : [];
    const ids: string[] = [...new Set(sourceCategoryIds.filter((id): id is string => typeof id === "string" && !!id))];

    // Legacy `tags` become multiple category memberships (existing behaviour preserved).
    for (const tag of (Array.isArray(merged.tags) ? merged.tags : [])) {
      if (typeof tag !== "string" || !tag.trim()) continue;
      const name = tag.trim();
      let category = categories.find((item) => item.parentId === null && item.name === name);
      if (!category) {
        let id = `migrated:${encodeURIComponent(name)}`;
        while (categoryIds.has(id)) id += ":legacy";
        category = { id, name, parentId: null, sort_index: categories.length, icon: { kind: "lucide", name: "Folder" } };
        categories.push(category);
        categoryIds.add(id);
      }
      if (!ids.includes(category.id)) ids.push(category.id);
    }
    for (const id of ids) if (!categoryIds.has(id)) {
      let restored = `restored:${id}`;
      while (categoryIds.has(restored)) restored += ":legacy";
      categories.push({ id: restored, name: "恢复的分类", parentId: null, sort_index: categories.length, icon: { kind: "lucide", name: "Folder" } });
      categoryIds.add(restored);
      ids[ids.indexOf(id)] = restored;
    }

    const entryId = deterministicEntryId(path);
    const replicaStatus = merged.status === "missing" ? "missing" : merged.status === "parse_error" ? "parse_error" : "active";
    // createdAt chain (newest-first fallback): lastAnalyzedAt -> mtime -> 0 (deterministic, retry-safe).
    const createdAt = finite(merged.lastAnalyzedAt) || finite(merged.mtime) || 0;

    entries.push({
      id: entryId,
      createdAt,
      // checksum stays null: the pending marker for A.5. The first scan recomputes it,
      // bypassing the size+mtime cache because migrated data has no trustworthy cache.
      checksum: null,
      status: replicaStatus === "active" ? "active" : "retired",
      categoryIds: [...new Set(ids)],
      displayName: stringOr(merged.displayName) || fileNameFromPath(path),
      internalName: typeof merged.internalName === "string" ? merged.internalName : undefined,
      author: stringOr(merged.author),
      description: stringOr(merged.description),
      tags: [],
      preview: {
        path: merged.previewPath || merged.preview_image_path,
        preview_image_path: merged.preview_image_path || merged.previewPath,
        updatedAt: merged.previewUpdatedAt,
      },
      stats: {
        totalBlocks: finite(merged.totalBlocks),
        totalVolume: finite(merged.totalVolume),
        regionCount: finite(merged.regionCount),
        regionNames: Array.isArray(merged.regionNames) && merged.regionNames.every((name: unknown) => typeof name === "string") ? [...merged.regionNames] : undefined,
        enclosingSize: merged.enclosingSize || { x: 0, y: 0, z: 0 },
        minecraftDataVersion: finite(merged.minecraftDataVersion),
      },
      sortIndex: Number.isFinite(merged.sort_index) ? Number(merged.sort_index) : index,
      lastAnalyzedAt: finite(merged.lastAnalyzedAt),
      lastError: stringOr(merged.lastError),
    });

    replicas.push({
      id: deterministicReplicaId(path),
      entryId,
      path,
      fileName: fileNameFromPath(path),
      folderPath: parentDirectory(path),
      checksum: null,
      fileSize: finite(merged.fileSize),
      mtime: finite(merged.mtime),
      status: replicaStatus,
      lastAnalyzedAt: finite(merged.lastAnalyzedAt),
      lastError: stringOr(merged.lastError),
    });
  }

  const folders = dedupeLegacyFolders(rawFolders);
  const hasContent = rawRecords.length > 0 || rawFolders.length > 0 || rawCategories.length > 0;
  if (hasContent && !folders.some((folder) => windowsPathKey(folder.path) === windowsPathKey(BUILTIN_IMPORT_PATH))) {
    folders.push({ path: BUILTIN_IMPORT_PATH, type: "builtin_import", enabled: true, recursive: true, sort_index: folders.length, lastSyncedAt: 0, lastScanCount: 0, lastError: "" });
  }

  // Legacy trash never moved files: keep it as history, mark it legacy, not restorable, cleanable.
  const trash = rawTrash.map((entry) => {
    const record = asRecord(entry);
    return {
      ...record,
      records: (Array.isArray(record.records) ? record.records : []).map(legacyRecordView),
      categories: normalizeLegacyCategories(record.categories),
      replicaSnapshot: null,
      originalPath: "",
      originalFileName: "",
      trashPath: "",
      movedAt: finite(record.deletedAt),
      status: "legacy",
    };
  });

  const recent = [...entries]
    .sort((left, right) => left.sortIndex - right.sortIndex)
    .slice(0, RECENT_LIMIT)
    .map((entry) => ({ kind: "entry", entryId: entry.id }));

  const { records: _legacyRecords, ...rest } = raw;
  return {
    ...rest,
    schemaVersion: SCHEMA_VERSION,
    entries,
    replicas,
    folders,
    categories,
    trash,
    redirects: [],
    favorites: [],
    recent,
  };
}