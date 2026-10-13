import { normalizeLibraryCategoryIcon } from "./libraryCategoryIcons";
import {
  readUserConfigFile,
  writeUserConfigFile,
  checkFileExists,
  DirectoryEntryInfo,
  listLitematicFileEntriesInDirectory,
} from "./backend";
import { inspectProject } from "../platform/applicationOperations";

export interface ProjectionRecord {
  id: string;
  path: string;
  fileName: string;
  displayName: string;
  author: string;
  description: string;
  totalBlocks: number;
  totalVolume: number;
  regionCount: number;
  enclosingSize: { x: number, y: number, z: number };
  minecraftDataVersion: number;
  fileSize: number;
  mtime: number;
  /** Legacy migration input only; not exposed as an editable tag system. */
  tags?: string[];
  categoryIds?: string[];
  status: "ok" | "missing" | "parse_error";
  lastAnalyzedAt: number;
  lastError: string;
  previewPath?: string;
  preview_image_path?: string;
  previewUpdatedAt?: number;
  sort_index?: number;
}

export interface LocalLibraryFolder {
  path: string;
  enabled: boolean;
  recursive: boolean;
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

export interface LibraryState {
  records: ProjectionRecord[];
  folders: LocalLibraryFolder[];
  categories?: LibraryCategory[];
  schemaVersion?: number;
}

const LIB_PATH = "projection-library/js_library.json";

function folderKey(path: string): string {
  return path.trim().replace(/[\\/]+$/, "").toLowerCase();
}

function normalizeFolder(folder: Partial<LocalLibraryFolder>, index: number): LocalLibraryFolder | null {
  const path = typeof folder.path === "string" ? folder.path.trim().replace(/[\\/]+$/, "") : "";
  if (!path) return null;
  return {
    path,
    enabled: folder.enabled !== false,
    recursive: folder.recursive !== false,
    sort_index: Number.isFinite(folder.sort_index) ? Number(folder.sort_index) : index,
    lastSyncedAt: Number(folder.lastSyncedAt || 0),
    lastScanCount: Number(folder.lastScanCount || 0),
    lastError: typeof folder.lastError === "string" ? folder.lastError : "",
  };
}

export const LIBRARY_CATEGORY_PRESETS = [
  ["warehouse", "仓库", "Warehouse"], ["packer", "打包机", "Package"], ["crafting", "合成站", "Hammer"],
  ["building", "建筑", "Building2"], ["farm", "农场", "Wheat"], ["cannon", "各类炮", "Crosshair"],
  ["all-items", "全物品", "Boxes"], ["furnace", "熔炉组", "Flame"], ["trees", "树场", "Trees"],
  ["mobs", "刷怪塔", "Skull"], ["stone", "刷石机", "Mountain"], ["iron", "刷铁机", "Anvil"], ["other", "杂项", "Folder"],
] as const;

export function normalizeLibraryState(raw?: Partial<LibraryState> | null): LibraryState {
  const categories: LibraryCategory[] = [];
  const categoryIds = new Set<string>();
  for (const [index, entry] of (Array.isArray(raw?.categories) ? raw.categories : []).entries()) {
    if (!entry || typeof entry.id !== "string" || !entry.id || categoryIds.has(entry.id)) continue;
    categoryIds.add(entry.id);
    categories.push({ id: entry.id, name: typeof entry.name === "string" && entry.name.trim() ? entry.name.trim() : "恢复的分类",
      parentId: typeof entry.parentId === "string" ? entry.parentId : null,
      sort_index: Number.isFinite(entry.sort_index) ? entry.sort_index : index,
      icon: normalizeLibraryCategoryIcon(entry.icon) });
  }
  // Seed only the initial migration. A user can rename/delete presets without their resurrection.
  if ((raw?.schemaVersion || 0) < 2 && !categories.length) {
    for (const [id, name, icon] of LIBRARY_CATEGORY_PRESETS) categories.push({ id: `builtin:${id}`, name, parentId: null, sort_index: categories.length, icon: { kind: "lucide", name: icon } });
    for (const entry of categories) categoryIds.add(entry.id);
  }
  for (const entry of categories) {
    if (!entry.parentId || !categoryIds.has(entry.parentId)) { entry.parentId = null; continue; }
    const visited = new Set([entry.id]); let parent: LibraryCategory | undefined = entry;
    while (parent?.parentId) {
      if (visited.has(parent.parentId)) { entry.parentId = null; break; }
      visited.add(parent.parentId); parent = categories.find((item) => item.id === parent!.parentId);
    }
  }
  const records = (Array.isArray(raw?.records) ? raw.records : []).map((record, index) => {
    const { tags, ...retained } = record;
    const ids = [...new Set((record.categoryIds || []).filter((id): id is string => typeof id === "string" && !!id))];
    // Multiple old tags become multiple memberships, never just the first tag.
    for (const tag of (Array.isArray(tags) ? tags : [])) {
      if (typeof tag !== "string" || !tag.trim()) continue;
      const name = tag.trim();
      let category = categories.find((item) => item.parentId === null && item.name === name);
      if (!category) {
        let id = `migrated:${encodeURIComponent(name)}`;
        while (categoryIds.has(id)) id += ":legacy";
        category = { id, name, parentId: null, sort_index: categories.length, icon: { kind: "lucide", name: "Folder" } };
        categories.push(category); categoryIds.add(id);
      }
      if (!ids.includes(category.id)) ids.push(category.id);
    }
    for (const id of ids) if (!categoryIds.has(id)) {
      categories.push({ id, name: "恢复的分类", parentId: null, sort_index: categories.length, icon: { kind: "lucide", name: "Folder" } }); categoryIds.add(id);
    }
    return { ...retained, categoryIds: ids, sort_index: record.sort_index ?? index, preview_image_path: record.preview_image_path || record.previewPath };
  });
  const folders: LocalLibraryFolder[] = [];
  const seen = new Set<string>();
  for (const [index, folder] of (raw?.folders || []).entries()) {
    const normalized = normalizeFolder(folder, index);
    if (!normalized) continue;
    const key = folderKey(normalized.path);
    if (seen.has(key)) continue;
    seen.add(key);
    folders.push(normalized);
  }
  folders.sort((left, right) => left.sort_index - right.sort_index || left.path.localeCompare(right.path));
  folders.forEach((folder, index) => {
    folder.sort_index = index;
  });
  return { records, folders, categories, schemaVersion: 2 };
}

export function categoryDescendants(state: LibraryState, id: string): Set<string> {
  const result = new Set<string>([id]); let changed = true;
  while (changed) { changed = false; for (const entry of state.categories || []) if (entry.parentId && result.has(entry.parentId) && !result.has(entry.id)) { result.add(entry.id); changed = true; } }
  return result;
}

export function categoryPath(state: LibraryState, id: string): string {
  const names: string[] = []; const seen = new Set<string>(); let entry = state.categories?.find((item) => item.id === id);
  while (entry && !seen.has(entry.id)) { seen.add(entry.id); names.unshift(entry.name); entry = state.categories?.find((item) => item.id === entry!.parentId); }
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
  const next = normalizeLibraryState(state); assertCategoryParent(next, parentId); const value = categoryName(name);
  if (next.categories!.some((entry) => entry.id === id || (entry.parentId === parentId && entry.name === value))) throw new Error("同一位置已存在此分类，请换一个名称。");
  return { ...next, categories: [...next.categories!, { id, name: value, parentId, sort_index: next.categories!.length, icon: { kind: "lucide", name: "Folder" } }] };
}

export function updateLibraryCategory(state: LibraryState, id: string, patch: Partial<Pick<LibraryCategory, "name" | "parentId" | "icon">>): LibraryState {
  const next = normalizeLibraryState(state); const entry = next.categories!.find((item) => item.id === id);
  if (!entry) throw new Error("分类已不存在，请刷新后重试。");
  const parentId = patch.parentId === undefined ? entry.parentId : patch.parentId; assertCategoryParent(next, parentId);
  if (parentId && categoryDescendants(next, id).has(parentId)) throw new Error("分类不能移动到自身或子分类中。");
  const name = patch.name === undefined ? entry.name : categoryName(patch.name);
  if (next.categories!.some((item) => item.id !== id && item.parentId === parentId && item.name === name)) throw new Error("目标位置存在同名分类。");
  return { ...next, categories: next.categories!.map((item) => item.id === id ? { ...item, name, parentId, icon: patch.icon === undefined ? item.icon : normalizeLibraryCategoryIcon(patch.icon) } : item) };
}

export function deleteLibraryCategory(state: LibraryState, id: string): LibraryState {
  const next = normalizeLibraryState(state); const entry = next.categories!.find((item) => item.id === id);
  if (!entry) return next;
  // Children and projection memberships move one level up; files and other memberships stay intact.
  return { ...next, categories: next.categories!.filter((item) => item.id !== id).map((item) => item.parentId === id ? { ...item, parentId: entry.parentId } : item),
    records: next.records.map((record) => ({ ...record, categoryIds: [...new Set((record.categoryIds || []).flatMap((categoryId) => categoryId === id ? (entry.parentId ? [entry.parentId] : []) : [categoryId]))] })) };
}

export function moveLibraryRecordsToCategory(state: LibraryState, paths: string[], categoryId: string | null): LibraryState {
  const next = normalizeLibraryState(state); assertCategoryParent(next, categoryId); const selected = new Set(paths);
  return { ...next, records: next.records.map((record) => selected.has(record.path) ? { ...record, categoryIds: categoryId ? [categoryId] : [] } : record) };
}

function fileNameFromPath(filePath: string): string {
  return filePath.split(/[\\/]/).pop() || "";
}

function projectionRecordFromDirectoryEntry(entry: DirectoryEntryInfo): ProjectionRecord {
  const fileName = entry.name || fileNameFromPath(entry.path);
  return {
    id: entry.path,
    path: entry.path,
    fileName,
    displayName: fileName,
    author: "",
    description: "",
    totalBlocks: 0,
    totalVolume: 0,
    regionCount: 0,
    enclosingSize: { x: 0, y: 0, z: 0 },
    minecraftDataVersion: 0,
    fileSize: entry.file_size || 0,
    mtime: entry.mtime_ms || 0,
    categoryIds: [],
    status: "ok",
    lastAnalyzedAt: entry.mtime_ms || 0,
    lastError: "",
    sort_index: 0,
  };
}

async function persistLibraryState(state: LibraryState): Promise<LibraryState> {
  const normalized = normalizeLibraryState(state);
  await writeUserConfigFile(LIB_PATH, JSON.stringify(normalized, null, 2));
  return normalized;
}

async function addOrUpdateRecordInternal(
  state: LibraryState,
  filePath: string,
  persist: boolean,
): Promise<LibraryState> {
  const exists = await checkFileExists(filePath);
  const nextState = normalizeLibraryState({
    ...state,
    records: [...state.records],
    folders: [...state.folders],
  });
  if (!exists) {
    const existingIdx = nextState.records.findIndex((record) => record.path === filePath);
    if (existingIdx >= 0) {
      nextState.records[existingIdx].status = "missing";
      if (persist) return await persistLibraryState(nextState);
    }
    return nextState;
  }

  const analysis = await analyzeFile(filePath);
  return await addOrUpdateRecordFromAnalysisInternal(nextState, filePath, analysis, persist);
}

async function addOrUpdateRecordFromAnalysisInternal(
  state: LibraryState,
  filePath: string,
  analysis: Partial<ProjectionRecord>,
  persist: boolean,
): Promise<LibraryState> {
  const nextState = normalizeLibraryState({
    ...state,
    records: [...state.records],
    folders: [...state.folders],
  });
  const fileName = fileNameFromPath(filePath);
  const existingIdx = nextState.records.findIndex((record) => record.path === filePath);
  const existing = existingIdx >= 0 ? nextState.records[existingIdx] : null;

  const record: ProjectionRecord = {
    id: filePath,
    path: filePath,
    fileName,
    displayName: analysis.displayName || fileName,
    author: analysis.author || "",
    description: analysis.description || "",
    totalBlocks: analysis.totalBlocks || 0,
    totalVolume: analysis.totalVolume || 0,
    regionCount: analysis.regionCount || 0,
    enclosingSize: analysis.enclosingSize || { x: 0, y: 0, z: 0 },
    minecraftDataVersion: analysis.minecraftDataVersion || 0,
    fileSize: existing?.fileSize || 0,
    mtime: existing?.mtime || 0,
    categoryIds: existing?.categoryIds || [],
    status: analysis.status || "ok",
    lastAnalyzedAt: Date.now(),
    lastError: analysis.lastError || "",
    previewPath: existing?.previewPath || existing?.preview_image_path,
    preview_image_path: existing?.preview_image_path || existing?.previewPath,
    previewUpdatedAt: existing?.previewUpdatedAt,
    sort_index: existing?.sort_index ?? -1,
  };

  if (existingIdx >= 0) {
    nextState.records[existingIdx] = record;
  } else {
    nextState.records.unshift(record);
  }

  return persist ? await persistLibraryState(nextState) : nextState;
}

export async function loadLibrary(): Promise<LibraryState> {
  let raw: LibraryState;
  try { raw = JSON.parse(await readUserConfigFile(LIB_PATH)) as LibraryState; }
  catch { return normalizeLibraryState({ records: [], folders: [] }); }
  const state = normalizeLibraryState(raw);
  // Commit migration once. A failed write is surfaced, not silently turned into an empty library.
  if (JSON.stringify(raw) !== JSON.stringify(state)) await writeUserConfigFile(LIB_PATH, JSON.stringify(state, null, 2));
  return state;
}

export async function saveLibrary(state: LibraryState): Promise<void> {
  await persistLibraryState(state);
}

export function projectionRecordAnalysis(parsed: any): Partial<ProjectionRecord> {
  const meta = parsed?.metadata || {};
  return {
    status: "ok",
    lastError: "",
    displayName: meta.name || "",
    author: meta.author || "",
    description: meta.description || "",
    totalBlocks: meta.total_blocks || 0,
    totalVolume: meta.total_volume || 0,
    regionCount: meta.region_count || 0,
    enclosingSize: meta.enclosing_size || { x: 0, y: 0, z: 0 },
    minecraftDataVersion: meta.minecraft_data_version || 0,
  };
}

export async function analyzeFile(filePath: string): Promise<Partial<ProjectionRecord>> {
  try {
    return projectionRecordAnalysis(await inspectProject({ sourcePath: filePath }));
  } catch (error: any) {
    return { status: "parse_error", lastError: String(error) };
  }
}

export async function addOrUpdateRecord(state: LibraryState, filePath: string): Promise<LibraryState> {
  return await addOrUpdateRecordInternal(state, filePath, true);
}

export async function addOrUpdateRecordFromAnalysis(
  state: LibraryState,
  filePath: string,
  analysis: Partial<ProjectionRecord>,
): Promise<LibraryState> {
  return await addOrUpdateRecordFromAnalysisInternal(state, filePath, analysis, true);
}

/** Moves an existing record to recent content without coupling selection to file analysis. */
export async function activateProjectionRecord(state: LibraryState, filePath: string): Promise<LibraryState> {
  const updatedState = normalizeLibraryState(state);
  const orderedRecords = [
    ...updatedState.records.filter((record) => record.path === filePath),
    ...updatedState.records
      .filter((record) => record.path !== filePath)
      .sort((left, right) => (left.sort_index ?? 0) - (right.sort_index ?? 0)),
  ].map((record, index) => ({ ...record, sort_index: index }));
  return await persistLibraryState({ ...updatedState, records: orderedRecords });
}

/**
 * Searches enabled local-library folders without adding matches to recent content.
 */
export async function searchLocalLibraryFolderRecords(
  state: LibraryState,
  query: string,
): Promise<ProjectionRecord[]> {
  const normalizedState = normalizeLibraryState(state);
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const existingByPath = new Map(normalizedState.records.map((record) => [record.path, record]));
  const foundByPath = new Map<string, ProjectionRecord>();
  for (const folder of normalizedState.folders.filter((item) => item.enabled)) {
    const entries = await listLitematicFileEntriesInDirectory(folder.path, folder.recursive);
    for (const entry of entries) {
      const fileName = entry.name || fileNameFromPath(entry.path);
      const searchable = `${fileName} ${entry.path}`.toLowerCase();
      if (!searchable.includes(needle)) continue;
      foundByPath.set(entry.path, existingByPath.get(entry.path) || projectionRecordFromDirectoryEntry(entry));
    }
  }
  return [...foundByPath.values()].sort((left, right) => left.fileName.localeCompare(right.fileName));
}

export async function addLocalLibraryFolder(state: LibraryState, folderPath: string): Promise<LibraryState> {
  const normalizedPath = folderPath.trim().replace(/[\\/]+$/, "");
  if (!normalizedPath) {
    return normalizeLibraryState(state);
  }
  const nextState = normalizeLibraryState({
    ...state,
    records: [...state.records],
    folders: [...state.folders],
  });
  const key = folderKey(normalizedPath);
  if (!nextState.folders.some((folder) => folderKey(folder.path) === key)) {
    nextState.folders.push({
      path: normalizedPath,
      enabled: true,
      recursive: true,
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
  const nextState = normalizeLibraryState({
    ...state,
    records: [...state.records],
    folders: state.folders.map((folder) => {
      if (folderKey(folder.path) !== key) return folder;
      return {
        ...folder,
        ...patch,
        lastSyncedAt: patch.lastSyncedAt ?? folder.lastSyncedAt,
        lastScanCount: patch.lastScanCount ?? folder.lastScanCount,
        lastError: patch.lastError ?? folder.lastError,
      };
    }),
  });
  return await persistLibraryState(nextState);
}

export async function removeLocalLibraryFolder(state: LibraryState, folderPath: string): Promise<LibraryState> {
  const key = folderKey(folderPath);
  const nextState = normalizeLibraryState({
    ...state,
    records: [...state.records],
    folders: state.folders.filter((folder) => folderKey(folder.path) !== key),
  });
  return await persistLibraryState(nextState);
}

async function syncFoldersInternal(
  state: LibraryState,
  onlyFolderPath?: string,
): Promise<SyncLibraryFoldersResult> {
  let nextState = normalizeLibraryState({
    ...state,
    records: [...state.records],
    folders: [...state.folders],
  });
  const targetKey = onlyFolderPath ? folderKey(onlyFolderPath) : "";
  const targets = nextState.folders.filter((folder) => {
    if (!folder.enabled && !targetKey) return false;
    if (!targetKey) return true;
    return folderKey(folder.path) === targetKey;
  });

  let importedCount = 0;
  const errors: string[] = [];
  for (const folder of targets) {
    try {
      const files = await listLitematicFileEntriesInDirectory(folder.path, folder.recursive);
      nextState = normalizeLibraryState({
        ...nextState,
        folders: nextState.folders.map((item) =>
          folderKey(item.path) === folderKey(folder.path)
            ? {
                ...item,
                lastSyncedAt: Date.now(),
                lastScanCount: files.length,
                lastError: "",
              }
            : item,
        ),
      });
      importedCount += files.length;
    } catch (error: any) {
      const message = `${folder.path}：${String(error)}`;
      errors.push(message);
      nextState = normalizeLibraryState({
        ...nextState,
        folders: nextState.folders.map((item) =>
          folderKey(item.path) === folderKey(folder.path)
            ? {
                ...item,
                lastSyncedAt: Date.now(),
                lastError: String(error),
              }
            : item,
        ),
      });
    }
  }

  const persisted = await persistLibraryState(nextState);
  return {
    importedCount,
    scannedFolderCount: targets.length,
    state: persisted,
    errors,
  };
}

export async function syncLocalLibraryFolder(
  state: LibraryState,
  folderPath: string,
): Promise<SyncLibraryFoldersResult> {
  return await syncFoldersInternal(state, folderPath);
}

export async function syncAllLocalLibraryFolders(state: LibraryState): Promise<SyncLibraryFoldersResult> {
  return await syncFoldersInternal(state);
}

export async function setRecordPreview(state: LibraryState, filePath: string, previewPath: string): Promise<LibraryState> {
  const nextState = normalizeLibraryState({
    ...state,
    records: state.records.map((record) => {
      if (record.path !== filePath) return record;
      return {
        ...record,
        previewPath,
        preview_image_path: previewPath,
        previewUpdatedAt: Date.now(),
      };
    }),
  });
  return await persistLibraryState(nextState);
}

export async function reorderLibraryRecords(state: LibraryState, orderedPaths: string[]): Promise<LibraryState> {
  const order = new Map(orderedPaths.map((path, index) => [path, index]));
  const records = state.records.map((record) => ({
    ...record,
    sort_index: order.has(record.path) ? order.get(record.path)! : (record.sort_index ?? orderedPaths.length),
  }));
  return await persistLibraryState({ ...state, records });
}
