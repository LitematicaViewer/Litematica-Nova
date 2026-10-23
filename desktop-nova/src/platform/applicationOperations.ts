import type {
  MaterialAnalysisItem,
  ProjectAnalysis,
} from "./applicationRender";
import { invoke } from "./tauri";

export interface ApplicationCloseRequest { requestId: number }
export const getApplicationCloseRequest = (): Promise<ApplicationCloseRequest | null> => invoke("application_get_close_request");
export const confirmApplicationClose = (requestId: number, draftsPreserved: boolean): Promise<void> => invoke("application_confirm_close", { requestId, draftsPreserved });
export const requestApplicationClose = (): Promise<void> => invoke("application_request_close");

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
  units?: unknown[];
  stages?: unknown[][];
  dryRun: boolean;
}

export interface ApplicationConvertVersionInput {
  sourcePath: string;
  targetVersion: string;
  replacements: Record<string, string>;
  expectedSourceSha256?: string;
  outputPath?: string;
  overwrite?: boolean;
}

export interface BlockDatabaseVersion {
  version: string;
  dataVersion: number;
  blockCount: number;
  stateCount: number;
  legacy: boolean;
  outputExtension: string;
}
export interface BlockDatabaseBlock {
  id: string;
  name: string;
  defaultState: string;
  properties: Record<string, string[]>;
  stateCount: number;
  iconState: string;
}
export interface BlockDatabaseState {
  state: string;
  name: string;
  sideTextureDataUrl: string | null;
  /** Top-down (+Y) plane; only populated when the caller asks for it. */
  topTextureDataUrl?: string | null;
  mapColor: { id: number; rgb: number } | null;
  tintIndices: number[];
  faces: { texture: string; tintIndex?: number; side?: number; rotationX?: number; rotationY?: number; rule?: string }[];
  projection: "model-side" | "model-top" | "no-side-model" | "no-top-model" | "color-only" | "missing";
  missing?: boolean;
  error?: string;
  biomeProfile?: string;
  tintProfileVersion?: string;
}
export interface BlockDatabaseResult extends Partial<BlockDatabaseState> {
  version?: string;
  versions?: BlockDatabaseVersion[];
  mapColorVersions?: Array<Pick<BlockDatabaseVersion, "version" | "blockCount" | "stateCount"> & { dataVersion: number | null; colorOnly: boolean }>;
  blocks?: BlockDatabaseBlock[];
  states?: Record<string, BlockDatabaseState>;
}
export function queryBlockDatabase(input: {
  version?: string;
  query?: string;
  state?: string;
  states?: string[];
  noTextures?: boolean;
  topPlane?: boolean;
}): Promise<BlockDatabaseResult> {
  return invoke("application_query_block_database", { input });
}

export interface ConversionPreview {
  sourceSha256: string;
  sourceDataVersion: number;
  targetVersion: string;
  targetDataVersion: number;
  entries: { sourceState: string; sourceName?: string; sourceIconState?: string; targetName?: string; category?: string; targetState: string | null; count: number; reason: string; valid: boolean }[];
  blockedPayloads: string[];
  payloadIssues?: { kind: "unsupported" | "unimplemented" | "invalid"; typeName: string; count: number; message: string }[];
  payloadChanges: string[];
  writable: boolean;
  changedBlockCount: number;
  outputPath: string | null;
}

export function convertProjectVersion(input: ApplicationConvertVersionInput): Promise<ConversionPreview> {
  return invoke("application_convert_project_version", { input });
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
  knownRevision?: string;
}

export interface ApplicationProjectMaterialsInput extends Omit<ApplicationProjectStatsInput, "knownRevision"> {
  scope: ApplicationMaterialScope;
}

export interface ApplicationProjectMaterialsOutput {
  material_items: MaterialAnalysisItem[];
}

export interface MaterialWebExportOptions {
  mode: "single" | "multi";
  accessPassword?: string;
  adminPassword?: string;
  whitelist: string[];
  whitelistEnabled: boolean;
  allowGuestReadonly: boolean;
  adminPageEnabled: boolean;
  targets: ("windows-x64" | "linux-x64" | "macos-x64" | "macos-arm64")[];
}

export interface ApplicationExportMaterialListInput extends ApplicationProjectMaterialsInput {
  uiLocale?: "zh-CN" | "en-US";
  languageMap?: Record<string, string>;
  multiplier: number;
  minecraftVersion: string;
  terminalRawMaterials: boolean;
  format: "svg" | "txt" | "html" | "csv";
  webOptions?: MaterialWebExportOptions;
  outputPath: string;
}

export function exportMaterialList(input: ApplicationExportMaterialListInput): Promise<{ output: string; material_count: number }> {
  return invoke("application_export_material_list", { input });
}

export function prepareMaterialRecipes(minecraftVersion: string): Promise<{ status: string; has_recipes: boolean }> {
  return invoke("application_prepare_material_recipes", { input: { minecraftVersion } });
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

export function readLayerBounds(input: ApplicationInspectProjectInput): Promise<[number, number] | null> {
  return invoke("application_read_layer_bounds", { input });
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

export function getProjectStats(input: ApplicationProjectStatsInput): Promise<ProjectAnalysis & { source_revision?: string; cache_unchanged?: boolean }> {
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
