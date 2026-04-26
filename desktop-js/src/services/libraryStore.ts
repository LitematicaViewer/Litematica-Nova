import { readWorkspaceFile, writeWorkspaceFile, executeBackend, checkFileExists } from "./backend";

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
}

export interface LibraryState {
  records: ProjectionRecord[];
}

const LIB_PATH = "data/projection-library/js_library.json";

export async function loadLibrary(): Promise<LibraryState> {
  try {
    const data = await readWorkspaceFile(LIB_PATH);
    return JSON.parse(data);
  } catch (e) {
    return { records: [] };
  }
}

export async function saveLibrary(state: LibraryState): Promise<void> {
  await writeWorkspaceFile(LIB_PATH, JSON.stringify(state, null, 2));
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
  };

  const existingIdx = state.records.findIndex(r => r.path === filePath);
  if (existingIdx >= 0) {
    record.tags = state.records[existingIdx].tags;
    state.records[existingIdx] = record;
  } else {
    state.records.unshift(record);
  }

  await saveLibrary(state);
  return { ...state };
}
