import { readUserConfigFile, writeUserConfigFile, executeBackend, checkFileExists } from "./backend";

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

export interface LibraryState {
  records: ProjectionRecord[];
}

const LIB_PATH = "projection-library/js_library.json";

export async function loadLibrary(): Promise<LibraryState> {
  try {
    const data = await readUserConfigFile(LIB_PATH);
    const parsed = JSON.parse(data) as LibraryState;
    parsed.records = (parsed.records || []).map((record, index) => ({
      ...record,
      sort_index: record.sort_index ?? index,
      preview_image_path: record.preview_image_path || record.previewPath,
    }));
    return parsed;
  } catch (e) {
    return { records: [] };
  }
}

export async function saveLibrary(state: LibraryState): Promise<void> {
  await writeUserConfigFile(LIB_PATH, JSON.stringify(state, null, 2));
}

export async function analyzeFile(filePath: string): Promise<Partial<ProjectionRecord>> {
  try {
    const out = await executeBackend("litematica_core.exe", ["analyze", filePath]);
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
    } catch(e) {
      return { status: "parse_error", lastError: out.substring(0, 1000) };
    }
  } catch (e: any) {
    return { status: "parse_error", lastError: e.toString() };
  }
}

export async function addOrUpdateRecord(state: LibraryState, filePath: string): Promise<LibraryState> {
  const exists = await checkFileExists(filePath);
  if (!exists) {
    const existingIdx = state.records.findIndex(r => r.path === filePath);
    if (existingIdx >= 0) {
      state.records[existingIdx].status = "missing";
      await saveLibrary(state);
    }
    return { ...state };
  }

  const analysis = await analyzeFile(filePath);
  const fileName = filePath.split(/[\\/]/).pop() || "";
  const existingIdx = state.records.findIndex(r => r.path === filePath);
  
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
    enclosingSize: analysis.enclosingSize || { x:0, y:0, z:0 },
    minecraftDataVersion: analysis.minecraftDataVersion || 0,
    fileSize: 0,
    mtime: 0,
    tags: [],
    status: analysis.status || "ok",
    lastAnalyzedAt: Date.now(),
    lastError: analysis.lastError || "",
    sort_index: existingIdx >= 0 ? state.records[existingIdx].sort_index : -1,
  };

  if (existingIdx >= 0) {
    record.tags = state.records[existingIdx].tags;
    record.previewPath = state.records[existingIdx].previewPath || state.records[existingIdx].preview_image_path;
    record.preview_image_path = state.records[existingIdx].preview_image_path || state.records[existingIdx].previewPath;
    record.previewUpdatedAt = state.records[existingIdx].previewUpdatedAt;
    record.sort_index = state.records[existingIdx].sort_index;
    state.records[existingIdx] = record;
  } else {
    state.records.unshift(record);
  }

  await saveLibrary(state);
  return { ...state };
}

export async function setRecordPreview(state: LibraryState, filePath: string, previewPath: string): Promise<LibraryState> {
  const newState = { ...state, records: state.records.map((record) => {
    if (record.path !== filePath) return record;
    return { ...record, previewPath, preview_image_path: previewPath, previewUpdatedAt: Date.now() };
  }) };
  await saveLibrary(newState);
  return newState;
}

export async function reorderLibraryRecords(state: LibraryState, orderedPaths: string[]): Promise<LibraryState> {
  const order = new Map(orderedPaths.map((path, index) => [path, index]));
  const records = state.records.map((record) => ({
    ...record,
    sort_index: order.has(record.path) ? order.get(record.path)! : (record.sort_index ?? orderedPaths.length),
  }));
  const newState = { ...state, records };
  await saveLibrary(newState);
  return newState;
}
