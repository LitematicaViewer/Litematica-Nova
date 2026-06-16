import {
  readUserConfigFile,
  writeUserConfigFile,
  executeBackend,
  checkFileExists,
  DirectoryEntryInfo,
  listLitematicFileEntriesInDirectory,
} from "./backend";

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
  tags: string[];
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

export interface LibraryState {
  records: ProjectionRecord[];
  folders: LocalLibraryFolder[];
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

function normalizeLibraryState(raw?: Partial<LibraryState> | null): LibraryState {
  const records = (raw?.records || []).map((record, index) => ({
    ...record,
    sort_index: record.sort_index ?? index,
    preview_image_path: record.preview_image_path || record.previewPath,
  }));
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
  return { records, folders };
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
    tags: [],
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
    tags: existing?.tags || [],
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
  try {
    const data = await readUserConfigFile(LIB_PATH);
    return normalizeLibraryState(JSON.parse(data) as LibraryState);
  } catch {
    return { records: [], folders: [] };
  }
}

export async function saveLibrary(state: LibraryState): Promise<void> {
  await persistLibraryState(state);
}

export async function analyzeFile(filePath: string): Promise<Partial<ProjectionRecord>> {
  // #region agent log
  const analyzeStart = Date.now();
  fetch('http://127.0.0.1:7337/ingest/eabd7817-9767-4d77-8cb1-446d598a0056',{method:'POST',headers:{'Content-Type':'application/json','X-Debug-Session-Id':'eabd7817-9767-4d77-8cb1-446d598a0056'},body:JSON.stringify({sessionId:'eabd7817-9767-4d77-8cb1-446d598a0056',location:'libraryStore.ts:205',message:'analyzeFile start',data:{filePath},timestamp:Date.now(),hypothesisId:'D'})}).catch(()=>{});
  // #endregion
  
  try {
    const out = await executeBackend("litematica_core.exe", ["analyze", filePath]);
    
    // #region agent log
    fetch('http://127.0.0.1:7337/ingest/eabd7817-9767-4d77-8cb1-446d598a0056',{method:'POST',headers:{'Content-Type':'application/json','X-Debug-Session-Id':'eabd7817-9767-4d77-8cb1-446d598a0056'},body:JSON.stringify({sessionId:'eabd7817-9767-4d77-8cb1-446d598a0056',location:'libraryStore.ts:213',message:'analyzeFile backend complete',data:{duration:Date.now()-analyzeStart,outputLength:out?.length||0},timestamp:Date.now(),hypothesisId:'D'})}).catch(()=>{});
    // #endregion
    
    try {
      const parsed = JSON.parse(out);
      const meta = parsed.metadata || {};
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
    } catch {
      return { status: "parse_error", lastError: out.substring(0, 1000) };
    }
  } catch (error: any) {
    return { status: "parse_error", lastError: String(error) };
  }
}

export async function addOrUpdateRecord(state: LibraryState, filePath: string): Promise<LibraryState> {
  return await addOrUpdateRecordInternal(state, filePath, true);
}

/**
 * Adds or refreshes a projection record, moves it to the top of recent content, and persists the library.
 */
export async function activateProjectionRecord(state: LibraryState, filePath: string): Promise<LibraryState> {
  const updatedState = await addOrUpdateRecordInternal(state, filePath, false);
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
