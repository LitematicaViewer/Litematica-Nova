import {
  readLayerMeta, readLayer, readProjectMarkers, editProjectOrigin,
} from "../platform/applicationOperations";
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
export function getBlockColor(blockId: string): string {
  // Simple heuristic color generation based on block id string hash to keep it stable
  let hash = 0;
  for (let i = 0; i < blockId.length; i++) {
    hash = blockId.charCodeAt(i) + ((hash << 5) - hash);
  }
  const c = (hash & 0x00FFFFFF).toString(16).toUpperCase();
  return "#" + "00000".substring(0, 6 - c.length) + c;
}
