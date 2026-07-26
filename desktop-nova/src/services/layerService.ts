import { executeBackend, checkFileExists, invalidateBackendCache } from "./backend";
import { translateBlockId } from "./i18n";

export interface LayerPaletteEntry {
  block_id: string;
  property_id: number;
  block_state?: unknown;
  state?: unknown;
  states?: unknown;
  properties?: unknown;
  property_ids?: unknown;
  property_indices?: unknown;
  property_refs?: unknown;
  state_ids?: unknown;
  state_indices?: unknown;
}

export interface LayerSliceMeta {
  chunk_size: number;
  size_x: number;
  size_y: number;
  size_z: number;
  palette: LayerPaletteEntry[];
  property_pool: Array<Record<string, string>>;
}

export interface LayerSliceBlock {
  x: number;
  z: number;
  palette_id: number;
}

export interface LayerSliceData {
  y: number;
  blocks: LayerSliceBlock[];
}

export interface ProjectionMarkerPoint {
  x: number;
  y: number;
  z: number;
}

export interface ProjectionMarkers {
  origin: ProjectionMarkerPoint | null;
}

export interface ProjectionMarkerEditSummary {
  input: string;
  output: string;
  backup_path: string | null;
  origin: ProjectionMarkerPoint | null;
}

function backendPathsLookSame(left: string, right: string): boolean {
  return left.replace(/\//g, "\\").toLocaleLowerCase() === right.replace(/\//g, "\\").toLocaleLowerCase();
}

async function invalidateEditedProjection(filePath: string, outputPath?: string): Promise<void> {
  if (!outputPath) {
    await invalidateBackendCache(filePath);
    return;
  }
  await invalidateBackendCache(outputPath);
  if (backendPathsLookSame(filePath, outputPath)) return;
}

/**
 * Checks whether a generated layer cache file is present on disk.
 */
export async function checkCacheExists(cacheFile: string): Promise<boolean> {
  if (!cacheFile) return false;
  return await checkFileExists(cacheFile);
}

/**
 * Loads layer-view metadata from a generated cache file.
 */
export async function loadLayerMeta(cacheFile: string): Promise<LayerSliceMeta | null> {
  try {
    const out = await executeBackend("litematica_core.exe", ["cache-layer-meta", cacheFile]);
    const parsed = JSON.parse(out);
    return parsed.visual as LayerSliceMeta;
  } catch (e) {
    console.error("Failed to load layer meta", e);
    return null;
  }
}

/**
 * Loads a single Y layer slice from a generated cache file.
 */
export async function loadLayerSlice(cacheFile: string, y: number): Promise<LayerSliceData | null> {
  try {
    const out = await executeBackend("litematica_core.exe", ["cache-layer", cacheFile, `--y=${y}`]);
    const parsed = JSON.parse(out);
    return {
      y: parsed.y,
      blocks: parsed.blocks || []
    };
  } catch (e) {
    console.error("Failed to load layer slice", e);
    return null;
  }
}

/**
 * Loads projection marker points stored in the source `.litematic` file.
 */
export async function loadProjectionMarkers(filePath: string): Promise<ProjectionMarkers> {
  if (!filePath) return { origin: null };
  try {
    const out = await executeBackend("litematica_core.exe", ["projection-markers", filePath]);
    const parsed = JSON.parse(out);
    return {
      origin: normalizeProjectionMarkerPoint(parsed.origin),
    };
  } catch (e) {
    console.error("Failed to load projection markers", e);
    return { origin: null };
  }
}

/**
 * Moves the projection origin marker to a layer-view coordinate.
 */
export async function setProjectionOrigin(
  filePath: string,
  point: ProjectionMarkerPoint,
  outputPath?: string,
): Promise<ProjectionMarkerEditSummary> {
  const args = [
    "set-origin",
    filePath,
    `--x=${point.x}`,
    `--y=${point.y}`,
    `--z=${point.z}`,
  ];
  if (outputPath) args.push(`--output=${outputPath}`);
  const out = await executeBackend("litematica_core.exe", args);
  await invalidateEditedProjection(filePath, outputPath);
  const parsed = JSON.parse(out);
  return {
    input: String(parsed.input || filePath),
    output: String(parsed.output || outputPath || filePath),
    backup_path: parsed.backup_path ? String(parsed.backup_path) : null,
    origin: normalizeProjectionMarkerPoint(parsed.origin),
  };
}

/**
 * Removes the projection origin marker by restoring the enclosing bounds origin.
 */
export async function removeProjectionOrigin(filePath: string, outputPath?: string): Promise<ProjectionMarkerEditSummary> {
  const args = ["remove-origin", filePath];
  if (outputPath) args.push(`--output=${outputPath}`);
  const out = await executeBackend("litematica_core.exe", args);
  await invalidateEditedProjection(filePath, outputPath);
  const parsed = JSON.parse(out);
  return {
    input: String(parsed.input || filePath),
    output: String(parsed.output || outputPath || filePath),
    backup_path: parsed.backup_path ? String(parsed.backup_path) : null,
    origin: normalizeProjectionMarkerPoint(parsed.origin),
  };
}

/**
 * Loads every layer slice in one backend call. Lets the renderer prefetch the
 * whole projection so switching layers is a pure in-memory lookup with no
 * per-layer subprocess round-trip.
 */
export async function loadAllLayerSlices(cacheFile: string): Promise<LayerSliceData[] | null> {
  try {
    const out = await executeBackend("litematica_core.exe", ["cache-layers-all", cacheFile]);
    const parsed = JSON.parse(out);
    const layers = Array.isArray(parsed.layers) ? parsed.layers : [];
    return layers.map((layer: any) => ({
      y: layer.y,
      blocks: layer.blocks || [],
    }));
  } catch (e) {
    console.error("Failed to load all layer slices", e);
    return null;
  }
}

/**
 * Converts a palette block id to a stable hex color for canvas fallback drawing.
 */
export function getBlockColor(blockId: string): string {
  // Simple heuristic color generation based on block id string hash to keep it stable
  let hash = 0;
  for (let i = 0; i < blockId.length; i++) {
    hash = blockId.charCodeAt(i) + ((hash << 5) - hash);
  }
  const c = (hash & 0x00FFFFFF).toString(16).toUpperCase();
  return "#" + "00000".substring(0, 6 - c.length) + c;
}

function normalizeProjectionMarkerPoint(value: any): ProjectionMarkerPoint | null {
  if (!value || typeof value !== "object") return null;
  const x = Number(value.x);
  const y = Number(value.y);
  const z = Number(value.z);
  if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(z)) return null;
  return {
    x: Math.trunc(x),
    y: Math.trunc(y),
    z: Math.trunc(z),
  };
}
