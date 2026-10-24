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
  selection?: { sourceState: string; targetBlockId: string; bulk: boolean };
  propertyEdit?: { sourceState: string; properties: Record<string, string> };
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
  sourceVersion: string;
  effectiveReplacements: Record<string, string>;
  targetVersion: string;
  targetDataVersion: number;
  entries: { sourceBlockId: string; sourceIconId: string; sourceProperties: Record<string,string>; targetBlockId: string | null; targetProperties: Record<string,string>; targetPropertyOptions: Record<string,string[]>; sourceState: string; sourceName?: string; sourceIconState?: string; targetName?: string; category?: string; targetState: string | null; count: number; reason: string; valid: boolean }[];
  blockedPayloads: string[];
  payloadIssues?: { kind: "unsupported" | "unimplemented" | "invalid"; typeName: string; count: number; message: string }[];
  payloadChanges: string[];
  writable: boolean;
  changedBlockCount: number;
  outputPath: string | null;
}

export function convertProjectVersion(input: ApplicationConvertVersionInput, operationId = crypto.randomUUID()): Promise<ConversionPreview> {
  return queryApplicationData({ operationId, kind: "versionConvert", input });
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
  includeStructuralBlocks?: boolean;
  uiLocale?: string;
  languageMap?: Record<string, string>;
}

export interface ApplicationProjectMaterialsInput extends Omit<ApplicationProjectStatsInput, "knownRevision"> {
  scope: ApplicationMaterialScope;
}

export interface ApplicationProjectMaterialsOutput {
  material_items: MaterialAnalysisItem[];
}

/** The data worker owns all defaults, option dependencies and output semantics. */
export interface MaterialExportField {
  key: string;
  type: "select" | "number" | "boolean" | "text" | "password" | "multiline" | "multiselect";
  group: string;
  labelKey: string;
  descriptionKey?: string;
  visible: boolean;
  enabled: boolean;
  reasonKey?: string;
  value: unknown;
  options?: Array<{ value: string; labelKey?: string; label?: string }>;
  min?: number;
  max?: number;
  step?: number;
}
export interface MaterialExportView {
  settings: Record<string, unknown>;
  fields: MaterialExportField[];
  valid: boolean;
  errors: string[];
  defaultPath: string;
  extension: string;
  copy: boolean;
  sourceRevision?: string;
  exportText?: string;
  outputPath?: string;
  summary?: unknown;
}
export interface MaterialExportInput {
  action: "configure" | "export" | "reset";
  sourcePath?: string;
  taskId?: string;
  projectId?: string;
  sourceRevision?: string;
  settings?: Record<string, unknown>;
  outputPath?: string;
  uiLocale?: string;
  languageMap?: Record<string, string>;
  query?: { offset?: number; limit?: number };
}
export function commandMaterialExport(input: MaterialExportInput): Promise<MaterialExportView> {
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

export interface ApplicationProjectStatsOutput {
  source_revision?: string; material_semantics?: string; cache_unchanged?: boolean;
  statisticsView?: StatisticsDisplayView; structural_block_counts?: ProjectAnalysis["structural_block_counts"];
}
export function getProjectStats(input: ApplicationProjectStatsInput, operationId = crypto.randomUUID()): Promise<ApplicationProjectStatsOutput> {
  return queryApplicationData({ operationId, kind: "stats", input });
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
  sourcePath?: string; taskId?: string; scope?: ApplicationMaterialScope; includeContainerItems?: boolean; multiplier?: number; uiLocale?: string; languageMap?: Record<string, string>;
}
export interface StockpileTaskPreferences {
  search: string; category: string; filter: string; sort: string; view: string;
  individualCounts: boolean; language: string;
  showAdvancedRecipeTree: boolean; showUnresolvedRecipes: boolean; showIconFallbackBadge: boolean;
}
export interface StockpileSourceInput {
  sourceId?: string; sourcePath: string; projectId?: string;
  scope: ApplicationMaterialScope; includeContainerItems: boolean; copies: number;
}
export interface StockpileSource extends StockpileSourceInput {
  sourceId: string; projectId: string; label: string; minecraftVersion: string;
  resourceIdentity: string; sourceRevision: string;
  status: "current" | "changed" | "missing" | "incompatible";
  context?: { sourceFileRevision?: string; sourceContentId?: string; resourceIdentity?: string };
}
export interface StockpileDemandSource {
  sourceId: string; sourcePath: string; label: string; copies: number;
  perCopyCount: number; requiredCount: number; scope: ApplicationMaterialScope;
}
export interface StockpileTaskSummary {
  taskId: string; projectId: string; name: string; sourcePath: string; sourceRevision: string;
  multiplier: number; createdAt: number; updatedAt: number;
  sourceCount: number; totalCopies: number;
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
  unresolved: boolean; unresolved_reason: string | null; tag: string | null; tag_display_name?: string; possible_items: string[];
  possible_item_names?: string[];
}
export interface StockpileRecipeTree extends StockpileRecipeIngredient {
  node_id: string; output_count: number; batch_count: number; extra_output: number; recipe_type: string;
  process_type: string; ingredients: StockpileRecipeIngredient[]; children: StockpileRecipeTree[]; depth: number;
  recipe_type_name?: string;
  requires_fuel: boolean; fuel_estimate: number | null; decorative_smithing: boolean;
}
export interface MaterialDetailInput {
  sourcePath: string; materialId: string; includeContainerItems?: boolean; selectedRecipe?: string;
  uiLocale?: string; languageMap?: Record<string, string>;
  sourceQuery?: { region?: string; offset?: number; limit?: number };
}
export interface MaterialDetail {
  material: StockpileMaterial;
  recipeOptions: Array<{ recipe_id: string; recipe_type: string; output_count: number; display_name: string; recipe_type_name: string }>;
  recipeTree: StockpileRecipeTree | null;
  iconImages: Record<string, string>;
  itemNames: Record<string, Record<string, string>>;
  sourceSummary?: MaterialSourceSummary;
  sourceGroups?: MaterialSourceGroup[];
}
export const readMaterialDetail = (input: MaterialDetailInput, operationId = crypto.randomUUID()): Promise<MaterialDetail> => queryApplicationData({ operationId, kind: "materialDetail", input });
export interface StockpileTaskView extends StockpileTaskSummary {
  sourceStatus?: "current" | "changed" | "missing" | "incompatible";
  context: { scope?: ApplicationMaterialScope; includeContainerItems?: boolean; sourceContentId?: string; sourceFileRevision?: string };
  sources: StockpileSource[];
  demandSources: StockpileDemandSource[];
  preferences: StockpileTaskPreferences;
  view: StockpileQueryView;
  detailMaterialId?: string; sourceSummary?: MaterialSourceSummary; sourceGroups?: MaterialSourceGroup[];
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
export interface StockpileTaskWorkspace { exportText?: string; legacyTasks: Array<{taskId:string;preparedCount:number;preparingCount:number;itemCount:number;updatedAt:number;sourceRevision?:string|null}>; tasks: StockpileTaskSummary[]; selectedTaskId: string | null; task: StockpileTaskView | null; outputPath?: string }
export interface StockpileTaskMutation {
  materialId?: string; quantity?: number; status?: "preparing" | "done"; clear?: boolean; reset?: boolean;
  publicNote?: string; storageLocation?: string; stackSize?: number; locked?: boolean;
  requestId?: string; complete?: boolean; adjustment?: { unit: "each" | "stack" | "box" | "chest"; steps: number };
}
export interface StockpileTaskCommandInput {
  action: "open" | "list" | "create" | "select" | "rename" | "sync" | "settings" | "mutate" | "details" | "export" | "import" | "bindLegacy" | "exportView" | "setSources" | "addSources";
  taskId?: string; projectId?: string; legacyTaskId?: string; sourceRevision?: string; expectedRevision?: string; sourcePath?: string;
  scope?: ApplicationMaterialScope; includeContainerItems?: boolean; multiplier?: number; name?: string;
  sources?: StockpileSourceInput[]; sourceIds?: string[]; requestId?: string;
  preferences?: Partial<StockpileTaskPreferences>; mutation?: StockpileTaskMutation; materialId?: string;
  outputPath?: string; inputPath?: string; uiLocale?: string; languageMap?: Record<string, string>;
  query?: { offset?: number; limit?: number };
  sourceQuery?: { region?: string; offset?: number; limit?: number };
  export?: { source: "materials" | "task"; format: "copy" | "txt" | "csv"; scope: "all" | "visible" | "remaining" | "overfilled" | "prepared"; individualCounts: boolean; outputPath?: string };
}
export const getStockpileTask = (input: StockpileTaskInput = {}): Promise<StockpileTaskWorkspace> => invoke("application_stockpile_task", { input });
export const commandStockpileTask = (input: StockpileTaskCommandInput): Promise<StockpileTaskWorkspace> => invoke("application_stockpile_task_command", { input });
export const setStockpileTaskQuantity = (input: { taskId: string; sourceRevision: string; materialId: string; quantity: number }): Promise<StockpileTaskWorkspace> => invoke("application_stockpile_task_set", { input });
export const resetStockpileTask = (input: { taskId: string; sourceRevision: string }): Promise<StockpileTaskWorkspace> => invoke("application_stockpile_task_reset", { input });
export const prepareMaterialTaskWindow = (): Promise<void> => invoke("application_prepare_material_task_window");

/** Rust owns the ordering, quantities, state and display names in these views. */
export interface MaterialSourceSummary { regionCount: number; minY: number | null; maxY: number | null; totalCount: number; unlocatedCount: number }
export interface MaterialSourceLayer { layer: number; worldY: number; count: number; blockCount: number; containerItemCount: number }
export interface MaterialSourceGroup { region: string; displayName: string; minY: number | null; maxY: number | null; totalCount: number; layerCount: number; unlocatedCount: number; layers: MaterialSourceLayer[]; offset: number; limit: number }
export interface MaterialQueryRow {
  id: string; itemId: string; name: string; count: number; blockCount: number; containerItemCount: number; totalCount: number;
  iconHint: string; stackSize: number; canFitContainer: boolean; materialStatus: "item" | "no_item" | "unknown"; hasCustomStackSize: boolean;
  quantityText: string; sourceSummary?: MaterialSourceSummary; sourceGroups?: MaterialSourceGroup[];
}
export interface MaterialQueryInput {
  sourcePath: string; scope?: ApplicationMaterialScope; includeContainerItems: boolean;
  search: string; category: string; sort: string; individualCounts: boolean; offset: number; limit: number;
  uiLocale: string; languageMap?: Record<string, string>;
}
export interface MaterialQueryPage { rows: MaterialQueryRow[]; totalRows: number; totalMaterials: number; offset: number; limit: number; sourceRevision: string; includeContainerItems: boolean }
export const queryMaterials = (input: MaterialQueryInput, operationId = crypto.randomUUID()): Promise<MaterialQueryPage> => queryApplicationData({ operationId, kind: "materials", input });
export type StockpileItemView = StockpileTaskView["items"][number] & {
  material: StockpileMaterial; displayName: string; displayStatus: string; categoryLabel?: string;
  requiredText: string; preparedText: string; remainingText: string; preparingText: string; overfilledText: string;
  unitSteps: number[]; stacks: number; remainder: number; boxesRequired: number;
};
export interface StockpileQueryView {
  rows: StockpileItemView[]; totalRows: number; offset: number; limit: number; categories: string[];
  capabilities: { backup: boolean; import: boolean; reset: boolean; reason?: string };
  requiredText: string; preparedText: string; remainingText: string; completionPercent: number;
}
export interface StatisticsRuleResult { title: string; value: string; error: string; raw: string }
export interface StatisticsRuleView { results: StatisticsRuleResult[]; hasErrors: boolean; rules: string[] }
export interface StatisticsRulesInput { sourcePath: string; rules: string[]; uiLocale: string }
export const queryStatisticsRules = (input: StatisticsRulesInput, operationId = crypto.randomUUID()): Promise<StatisticsRuleView> => queryApplicationData({ operationId, kind: "statisticsRules", input });
export interface MaterialRecipesInput { sourcePath: string; includeContainerItems?: boolean; uiLocale: string; languageMap?: Record<string, string> }
export interface MaterialRecipesSummary {
  totalMaterials: number; excludedMaterials: number; totalRequiredCount: number;
  materialsWithRecipes: number; materialsWithoutRecipes: number; unresolvedMaterials: number;
  recipeOptionCount: number; recipeTreeCount: number; recipeTreeNodes: number; uniqueResourceCount: number;
  cacheHit: boolean; elapsedMs: number; recipeMs: number; namesMs: number; resourcesMs: number;
}
export interface MaterialRecipesResult { summary: MaterialRecipesSummary; sourceRevision: string }
export const queryMaterialRecipes = (input: MaterialRecipesInput, operationId = crypto.randomUUID()): Promise<MaterialRecipesResult> => queryApplicationData({ operationId, kind: "materialRecipes", input });
export type ApplicationDataQuery = { operationId: string; kind: "materialRecipes"; input: MaterialRecipesInput } | { operationId: string; kind: "stats"; input: ApplicationProjectStatsInput } | { operationId: string; kind: "versionConvert"; input: ApplicationConvertVersionInput } | { operationId: string; kind: "materialDetail"; input: MaterialDetailInput } | { operationId: string; kind: "materials"; input: MaterialQueryInput } | { operationId: string; kind: "statisticsRules"; input: StatisticsRulesInput };
export interface ApplicationDataOperation { operationId: string; state: "Pending" | "Running" | "Ready" | "Failed" | "Cancelled"; progress?: { stage: string; completed?: number; current?: number; total?: number; elapsedMs?: number }; error?: string }
function queryApplicationData(request: Extract<ApplicationDataQuery, { kind: "materialRecipes" }>): Promise<MaterialRecipesResult>;
function queryApplicationData(request: Extract<ApplicationDataQuery, { kind: "stats" }>): Promise<ApplicationProjectStatsOutput>;
function queryApplicationData(request: Extract<ApplicationDataQuery, { kind: "versionConvert" }>): Promise<ConversionPreview>;
function queryApplicationData(request: Extract<ApplicationDataQuery, { kind: "materialDetail" }>): Promise<MaterialDetail>;
function queryApplicationData(request: Extract<ApplicationDataQuery, { kind: "materials" }>): Promise<MaterialQueryPage>;
function queryApplicationData(request: Extract<ApplicationDataQuery, { kind: "statisticsRules" }>): Promise<StatisticsRuleView>;
function queryApplicationData(request: ApplicationDataQuery): Promise<MaterialRecipesResult | ApplicationProjectStatsOutput | ConversionPreview | MaterialDetail | MaterialQueryPage | StatisticsRuleView> { return invoke("application_data_query", { input: request }); }
export const getDataOperationStatus = (operationId: string, cancel = false): Promise<ApplicationDataOperation> => invoke("application_data_operation_status", { operationId, cancel });

export interface StatisticsDisplayView {
  totalNonAirBlocks: number; regionCount: number; enclosingSize: { x: number; y: number; z: number };
  density: number; buildingType: string; redstoneRatio: number | null; fluidRatio: number | null; entityCount: number; materialCount: number;
  regions: Array<{ name: string; bounds: unknown; size: unknown; position: unknown }>;
  layers: Array<{ layer: number; world_y: number; total_non_air_blocks: number; unique_block_types: number }>;
  containerScan: { enabled: boolean; containers_scanned: number; item_stacks_scanned: number; warnings: string[] };
}
