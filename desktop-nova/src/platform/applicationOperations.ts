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

export interface StockpileTaskInput {
  sourcePath?: string; taskId?: string; scope?: ApplicationMaterialScope; includeContainerItems?: boolean; multiplier?: number;
}
export interface StockpileTaskPreferences {
  search: string; category: string; filter: string; sort: string; view: string;
  individualCounts: boolean; language: string;
  showAdvancedRecipeTree: boolean; showUnresolvedRecipes: boolean; showIconFallbackBadge: boolean;
}
export interface StockpileTaskSummary {
  taskId: string; projectId: string; name: string; sourcePath: string; sourceRevision: string;
  multiplier: number; createdAt: number; updatedAt: number;
}
export interface StockpileMaterial {
  id: string; namespace_id: string; display_name: string; required_count: number;
  stack_size: number; stacks: number; remainder: number; shulker_boxes: number;
  category: string; category_icon: string; item_icon_key: string; icon_path: string; icon_available: boolean;
  icon_diagnostic?: string | null; normalized_from: string[]; display_names: Record<string,string>;
  source_regions: string[]; recipe_status: string; craft_complexity: number;
}
export interface StockpileRecipeIngredient {
  item_id: string; display_name: string; icon_key: string; needed_count: number; visual_kind: string;
  unresolved: boolean; unresolved_reason: string | null; tag: string | null; possible_items: string[];
}
export interface StockpileRecipeTree extends StockpileRecipeIngredient {
  node_id: string; output_count: number; batch_count: number; extra_output: number; recipe_type: string;
  process_type: string; ingredients: StockpileRecipeIngredient[]; children: StockpileRecipeTree[]; depth: number;
  requires_fuel: boolean; fuel_estimate: number | null; decorative_smithing: boolean;
}
export interface MaterialDetailInput {
  material: unknown; materialSemantics: string; dataVersion: number; selectedRecipe?: string;
}
export interface MaterialDetail {
  material: StockpileMaterial;
  recipeOptions: Array<{ recipe_id: string; recipe_type: string; output_count: number }>;
  recipeTree: StockpileRecipeTree | null;
  iconImages: Record<string, string>;
  itemNames: Record<string, Record<string, string>>;
}
export const readMaterialDetail = (input: MaterialDetailInput): Promise<MaterialDetail> => invoke("application_read_material_detail", { input });
export interface StockpileTaskView extends StockpileTaskSummary {
  sourceStatus?: "current" | "changed" | "missing";
  context: { scope?: ApplicationMaterialScope; includeContainerItems?: boolean; sourceContentId?: string; sourceFileRevision?: string };
  preferences: StockpileTaskPreferences;
  materials: {
    schema_version: number; ui_locale?: string | null;
    project: { source_file: string; created_at: number; data_version: number; regions: string[] };
    summary: { total_blocks: number; unique_materials: number; total_stacks: number; estimated_shulker_boxes: number };
    materials: StockpileMaterial[];
    diagnostics: Array<{kind:string;source:string;target:string;message:string}>;
  };
  recipeTrees: Record<string,StockpileRecipeTree> | null;
  iconImages?: Record<string,string>; itemNames?: Record<string,Record<string,string>>;
  items: Array<{
    materialId: string; requiredCount: number; preparedCount: number; remainingCount: number; complete: boolean;
    status: string; quantity: number; preparingCount: number; doneCount: number; overfilledCount: number;
    stackSize: number; publicNote: string | null; storageLocation: string | null; locked: boolean;
  }>;
  requiredCount: number; preparedCount: number; remainingCount: number; preparingCount: number; overfilledCount: number;
  completedItems: number; totalItems: number;
}
export interface StockpileTaskWorkspace { legacyTasks: Array<{taskId:string;preparedCount:number;preparingCount:number;itemCount:number;updatedAt:number;sourceRevision?:string|null}>; tasks: StockpileTaskSummary[]; selectedTaskId: string | null; task: StockpileTaskView | null; outputPath?: string }
export interface StockpileTaskMutation {
  materialId?: string; quantity?: number; status?: "preparing" | "done"; clear?: boolean; reset?: boolean;
  publicNote?: string; storageLocation?: string; stackSize?: number; locked?: boolean;
}
export interface StockpileTaskCommandInput {
  action: "open" | "list" | "create" | "select" | "rename" | "sync" | "settings" | "mutate" | "details" | "export" | "import" | "bindLegacy";
  taskId?: string; projectId?: string; legacyTaskId?: string; sourceRevision?: string; expectedRevision?: string; sourcePath?: string;
  scope?: ApplicationMaterialScope; includeContainerItems?: boolean; multiplier?: number; name?: string;
  preferences?: Partial<StockpileTaskPreferences>; mutation?: StockpileTaskMutation; materialId?: string;
  outputPath?: string; inputPath?: string;
}
export const getStockpileTask = (input: StockpileTaskInput = {}): Promise<StockpileTaskWorkspace> => invoke("application_stockpile_task", { input });
export const commandStockpileTask = (input: StockpileTaskCommandInput): Promise<StockpileTaskWorkspace> => invoke("application_stockpile_task_command", { input });
export const setStockpileTaskQuantity = (input: { taskId: string; sourceRevision: string; materialId: string; quantity: number }): Promise<StockpileTaskWorkspace> => invoke("application_stockpile_task_set", { input });
export const resetStockpileTask = (input: { taskId: string; sourceRevision: string }): Promise<StockpileTaskWorkspace> => invoke("application_stockpile_task_reset", { input });
export const prepareMaterialTaskWindow = (): Promise<void> => invoke("application_prepare_material_task_window");
