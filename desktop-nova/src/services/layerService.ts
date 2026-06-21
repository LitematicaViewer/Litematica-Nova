import { executeBackend, checkFileExists } from "./backend";
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

export async function checkCacheExists(cacheFile: string): Promise<boolean> {
  if (!cacheFile) return false;
  return await checkFileExists(cacheFile);
}

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

// Convert palette block id to hex color string for canvas drawing
export function getBlockColor(blockId: string): string {
  // Simple heuristic color generation based on block id string hash to keep it stable
  let hash = 0;
  for (let i = 0; i < blockId.length; i++) {
    hash = blockId.charCodeAt(i) + ((hash << 5) - hash);
  }
  const c = (hash & 0x00FFFFFF).toString(16).toUpperCase();
  return "#" + "00000".substring(0, 6 - c.length) + c;
}
