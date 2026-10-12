import type {
  MaterialAnalysisItem,
  ProjectAnalysis,
} from "./applicationRender";
import { invoke } from "./tauri";

export interface ApplicationInspectProjectInput {
  sourcePath: string;
}

export interface ApplicationReadLayerInput extends ApplicationInspectProjectInput {
  y: number;
}

export interface ApplicationReadLayerContainerInput extends ApplicationReadLayerInput {
  x: number;
  z: number;
}

export interface ProjectionMarkerPoint {
  x: number;
  y: number;
  z: number;
}

export interface ApplicationEditProjectOriginInput extends ApplicationInspectProjectInput {
  origin: ProjectionMarkerPoint | null;
  outputPath: string | null;
}

export interface LayerPaletteEntry {
  block_id: string;
  property_id: number;
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

export interface LayerContainerData extends ProjectionMarkerPoint {
  block_id: string;
  nbt: Record<string, unknown>;
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

export interface ApplicationEditProjectMetadataInput {
  sourcePath: string;
  patch: unknown;
  outputPath: string | null;
}

export interface ApplicationReplaceBlocksInput {
  sourcePath: string;
  outputPath: string | null;
  rules: unknown[];
  dryRun: boolean;
}

export interface ApplicationOperationResult {
  message: string;
  outputPath: string | null;
}

export type ApplicationMaterialScope =
  | { kind: "all"; value: null }
  | { kind: "region"; value: string }
  | { kind: "layer"; value: string };

export interface ApplicationProjectStatsInput {
  sourcePath: string;
  includeContainerItems: boolean;
}

export interface ApplicationProjectMaterialsInput extends ApplicationProjectStatsInput {
  scope: ApplicationMaterialScope;
}

export interface ApplicationProjectMaterialsOutput {
  material_items: MaterialAnalysisItem[];
}

export interface ApplicationGenerateProjectInput<TPlan = unknown> {
  plan: TPlan;
  outputPath: string | null;
  dryRun: boolean;
}

export interface ApplicationGenerateProjectOutput<TSummary = unknown> {
  summary: TSummary;
  analysis: ProjectAnalysis | null;
  outputPath: string | null;
  diagnostics: string;
}

export interface ApplicationRuntimeCapability {
  available: boolean;
  message: string;
}

export interface ApplicationRuntimeHealth {
  ok: boolean;
  core: ApplicationRuntimeCapability;
  viewer: ApplicationRuntimeCapability;
}

export interface EmbeddedPreviewImage {
  width: number;
  height: number;
  data_url: string;
}

export function inspectProject(input: ApplicationInspectProjectInput): Promise<ProjectAnalysis> {
  return invoke("application_inspect_project", { input });
}

export function readLayerMeta(input: ApplicationInspectProjectInput): Promise<LayerSliceMeta> {
  return invoke("application_read_layer_meta", { input });
}

export function readLayer(input: ApplicationReadLayerInput): Promise<LayerSliceData> {
  return invoke("application_read_layer", { input });
}

export function readLayerContainer(
  input: ApplicationReadLayerContainerInput,
): Promise<LayerContainerData | null> {
  return invoke("application_read_layer_container", { input });
}

export function readProjectMarkers(input: ApplicationInspectProjectInput): Promise<ProjectionMarkers> {
  return invoke("application_read_project_markers", { input });
}

export function editProjectOrigin(
  input: ApplicationEditProjectOriginInput,
): Promise<ProjectionMarkerEditSummary> {
  return invoke("application_edit_project_origin", { input });
}

export function editProjectMetadata(
  input: ApplicationEditProjectMetadataInput,
): Promise<ApplicationOperationResult> {
  return invoke("application_edit_project_metadata", { input });
}

export function replaceProjectBlocks(
  input: ApplicationReplaceBlocksInput,
): Promise<ApplicationOperationResult> {
  return invoke("application_replace_blocks", { input });
}

export function getProjectStats(input: ApplicationProjectStatsInput): Promise<ProjectAnalysis> {
  return invoke("application_get_project_stats", { input });
}

export function getProjectMaterials(
  input: ApplicationProjectMaterialsInput,
): Promise<ApplicationProjectMaterialsOutput> {
  return invoke("application_get_project_materials", { input });
}

export function generateProject<TSummary = unknown, TPlan = unknown>(
  input: ApplicationGenerateProjectInput<TPlan>,
): Promise<ApplicationGenerateProjectOutput<TSummary>> {
  return invoke("application_generate_project", { input });
}

export function getApplicationRuntimeHealth(): Promise<ApplicationRuntimeHealth> {
  return invoke("application_runtime_health");
}

export function readEmbeddedPreview(sourcePath: string): Promise<EmbeddedPreviewImage | null> {
  return invoke("application_read_embedded_preview", { input: { sourcePath } });
}
