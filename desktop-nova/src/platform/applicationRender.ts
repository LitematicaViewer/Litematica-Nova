export type RenderOperationState =
  | "requested"
  | "running"
  | "build_completed"
  | "artifact_ready"
  | "viewer_preparing"
  | "realtime_ready"
  | "gpu_accepted"
  | "gpu_resident"
  | "viewer_prepared"
  | "runtime_released"
  | "cancelled"
  | "failed";

// Neutral Presentation and project-analysis DTOs; no commercial runtime client.
export type PresentationMode = "hidden" | "popup" | "overlay" | "embedded";
export type PresentationState = "hidden" | "visible";

export interface ProjectSource {
  path: string;
}

export interface PresentationPlacement {
  x: number;
  y: number;
  width: number;
  height: number;
  dpiScale: number;
  focused: boolean;
}

export interface PresentationIntent {
  mode: PresentationMode;
  visible: boolean;
  placement: PresentationPlacement | null;
}

export interface RenderOperationProgress {
  phase: string;
  activity: string | null;
  completed: number | null;
  total: number | null;
  elapsedMs: number;
}

export interface IntegerVector {
  x: number;
  y: number;
  z: number;
}

export interface ProjectMetadata {
  name: string;
  author: string;
  description: string;
  time_created: number;
  time_modified: number;
  total_blocks: number;
  total_volume: number;
  region_count: number;
  enclosing_size: IntegerVector;
  litematic_version: number;
  litematic_subversion: number;
  minecraft_data_version: number;
}

export interface AnalysisScope {
  kind: string;
  region: string | null;
  layer: number | null;
  world_y: number | null;
}

export interface StructureStatsSummary {
  scope: AnalysisScope;
  total_non_air_blocks: number;
  unique_block_types: number;
  total_regions: number;
  total_layers: number;
  total_entities: number;
  total_tile_entities: number;
}

export interface RegionCountSummary {
  region: string;
  count: number;
}

export interface LayerCountSummary {
  layer: number;
  world_y: number;
  count: number;
}

export interface MaterialAnalysisItem {
  block_id: string;
  display_name: string;
  count: number;
  block_count: number;
  container_item_count: number;
  total_count: number;
  icon_hint: string;
  region_counts: RegionCountSummary[] | null;
  layer_counts: LayerCountSummary[] | null;
}

export interface RegionBounds {
  min_x: number;
  max_x: number;
  min_y: number;
  max_y: number;
  min_z: number;
  max_z: number;
}

export interface RegionAnalysisSummary {
  name: string;
  position: IntegerVector;
  size: IntegerVector;
  bounds: RegionBounds;
  total_non_air_blocks: number;
  unique_block_types: number;
  total_entities: number;
  total_tile_entities: number;
}

export interface LayerAnalysisSummary {
  layer: number;
  world_y: number;
  total_non_air_blocks: number;
  unique_block_types: number;
}

export interface EntityAnalysisItem {
  entity_id: string;
  count: number;
}

export interface EntityAnalysisSummary {
  total_entities: number;
  items: EntityAnalysisItem[];
}

export interface ContainerScanAnalysisSummary {
  enabled: boolean;
  containers_scanned: number;
  item_stacks_scanned: number;
  warnings: string[];
}

export interface ProjectAnalysis {
  metadata: ProjectMetadata;
  structure_stats: StructureStatsSummary;
  material_items: MaterialAnalysisItem[];
  regions: RegionAnalysisSummary[];
  layers_summary: LayerAnalysisSummary[];
  entity_summary: EntityAnalysisSummary;
  container_scan: ContainerScanAnalysisSummary;
  derived?: {
    building?: {
      building_type?: string;
      density?: number;
      redstone_ratio?: number;
      fluid_ratio?: number;
    };
  };
}

/** Product-facing render state. Renderer correlation IDs and process details never cross this boundary. */
export interface RenderOperationView {
  operationId: string;
  source: ProjectSource;
  state: RenderOperationState;
  progress: RenderOperationProgress;
  presentation: PresentationIntent;
  presentationState: PresentationState;
  analysis: ProjectAnalysis | null;
  error: string | null;
}

export interface ApplicationRenderSnapshot {
  operations: RenderOperationView[];
}

/** No public Full build operations exist; independent quick state lives in quickPreview. */
export function isApplicationRenderPending(_operation: RenderOperationView | null | undefined): boolean { return false; }
