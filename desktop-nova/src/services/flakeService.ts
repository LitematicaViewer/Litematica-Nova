import {
  readLayerMeta, readLayer, readProjectMarkers, editProjectOrigin,
  queryBlockDatabase, type BlockDatabaseState, type BlockDatabaseVersion,
} from "../platform/applicationOperations";
import { extractLayerPaletteStates, normalizeBlockId } from "./flakeStateHintResolver";
export { readLayerMeta, readLayer };

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
  minecraft_data_version: number | null;
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

export const loadProjectionMarkers = (sourcePath: string): Promise<ProjectionMarkers> =>
  readProjectMarkers({ sourcePath });
export const setProjectionOrigin = (sourcePath: string, origin: ProjectionMarkerPoint, outputPath?: string): Promise<ProjectionMarkerEditSummary> =>
  editProjectOrigin({ sourcePath, origin, outputPath: outputPath ?? null });
export const removeProjectionOrigin = (sourcePath: string, outputPath?: string): Promise<ProjectionMarkerEditSummary> =>
  editProjectOrigin({ sourcePath, origin: null, outputPath: outputPath ?? null });
export const UNKNOWN_LAYER_COLOR = "#808080";
export function isLayerAirBlock(blockId: string): boolean {
  return ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"].includes(normalizeBlockId(blockId).split("[")[0]);
}

export function layerPaletteState(entry: LayerPaletteEntry, propertyPool: Array<Record<string, string>>): string {
  // Database keys are canonical UTF-8 byte/ASCII property order, not the
  // current Windows locale's collation order.
  const properties = Object.entries(extractLayerPaletteStates(entry, propertyPool)).sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0);
  return `${normalizeBlockId(entry.block_id)}${properties.length ? `[${properties.map(([key, value]) => `${key}=${value}`).join(",")}]` : ""}`;
}

export function layerMapColor(visual: BlockDatabaseState | undefined): string {
  if (!visual?.mapColor || !Number.isInteger(visual.mapColor.rgb) || visual.mapColor.rgb < 0 || visual.mapColor.rgb > 0xffffff) return UNKNOWN_LAYER_COLOR;
  if (visual.mapColor.id === 0) return "transparent";
  return `#${visual.mapColor.rgb.toString(16).padStart(6, "0")}`;
}

export interface LayerPaletteVisuals {
  version: string;
  byPalette: Map<number, BlockDatabaseState>;
}
type LayerSourceVersion = Pick<BlockDatabaseVersion, "version" | "blockCount"> & { dataVersion: number | null };
let databaseVersions: Promise<LayerSourceVersion[]> | null = null;
const paletteVisuals = new Map<string, Promise<LayerPaletteVisuals>>();

/** Resolve source-version states together: one helper launch, one database
 * decode, one versioned mapping. Never use names or the latest game by default. */
export async function loadLayerPaletteVisuals(meta: LayerSliceMeta): Promise<LayerPaletteVisuals> {
  if (!meta.minecraft_data_version || meta.minecraft_data_version <= 0) throw new Error("投影没有游戏版本，请先在属性页转换版本");
  if (!databaseVersions) databaseVersions = queryBlockDatabase({}).then(result => [...result.versions || [], ...result.mapColorVersions || []]).catch(error => { databaseVersions = null; throw error; });
  const versions = await databaseVersions;
  const version = versions.find(version => version.dataVersion === meta.minecraft_data_version && version.blockCount > 0);
  if (!version) throw new Error("该游戏版本没有对应的方块数据，方块以未知标记显示");
  const states = meta.palette.map(entry => layerPaletteState(entry, meta.property_pool || []));
  const key = JSON.stringify([meta.minecraft_data_version, version.version, states]);
  if (!paletteVisuals.has(key)) {
    if (paletteVisuals.size >= 4) paletteVisuals.delete(paletteVisuals.keys().next().value!);
    paletteVisuals.set(key, queryBlockDatabase({ version: version.version, states: [...new Set(states)] }).then(result => {
      // An unavailable table/model is a retryable read, not a valid persisted
      // palette. Re-open/reload may succeed after resources finish installing.
      if (states.some(state => !result.states?.[state]?.mapColor || result.states[state].missing)) paletteVisuals.delete(key);
      return {
        version: version.version,
        byPalette: new Map(states.map((state, index) => [index, result.states?.[state]]).filter((entry): entry is [number, BlockDatabaseState] => !!entry[1])),
      };
    }).catch(error => { paletteVisuals.delete(key); throw error; }));
  }
  return paletteVisuals.get(key)!;
}
