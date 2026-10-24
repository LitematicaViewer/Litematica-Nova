import { saveDialog } from "../platform/dialogs";
import { getProjectStats, exportMaterialList, prepareMaterialRecipes, queryMaterialRecipes, type MaterialRecipesResult, type ApplicationMaterialScope, type MaterialWebExportOptions, type StatisticsDisplayView } from "../platform/applicationOperations";
import { t, getUiLocale, getMinecraftTranslations, getI18nRevision, initI18n } from "./i18n";
import { windowsPathKey } from "./filePathIdentity";

export interface MaterialItem {
  id: string;
  itemId?: string;
  name: string;
  sourceDisplayName?: string;
  count: number;
  blockCount: number;
  containerItemCount: number;
  totalCount: number;
  iconHint: string;
  stackSize: number;
  canFitContainer: boolean;
  materialStatus: "item" | "no_item" | "unknown";
  hasCustomStackSize: boolean;
  regionCounts: Array<{ region: string; count: number }>;
  layerCounts: Array<{ layer: number; worldY: number; count: number }>;
}

export interface NonItemBlock {
  id: string;
  name: string;
  sourceDisplayName?: string;
  count: number;
  reason: "no_item" | "unknown";
}

export interface ContainerScanSummary {
  enabled: boolean;
  containers_scanned: number;
  item_stacks_scanned: number;
  warnings: string[];
}

export interface RegionInfo {
  name: string;
  bounds: unknown;
  size: unknown;
  position: unknown;
}

export interface LayerInfo {
  layer: number;
  world_y: number;
  total_non_air_blocks: number;
  unique_block_types: number;
}

export interface StructuralBlockCount { id: string; count: number }

export interface StatsData extends StatisticsDisplayView {
  sourceRevision?: string;
  timing?: { requestStarted: number; mappedAt: number; cacheReused: boolean };
}

export function mapStructureStats(parsed: { statisticsView?: StatisticsDisplayView }): StatsData {
  if (!parsed.statisticsView) throw new Error(t("native.stats.displayDataMissing"));
  return parsed.statisticsView;
}

const statsCompleted = new Map<string, { revision: string; data: StatsData }>();

export async function loadStructureStats(filePath: string, includeContainerItems = false, operationId = crypto.randomUUID()): Promise<StatsData> {
  await initI18n();
  const key = JSON.stringify([windowsPathKey(filePath), includeContainerItems, getUiLocale(), getI18nRevision()]);
  const cached = statsCompleted.get(key);
  const started = performance.now();
  const pending = getProjectStats({ sourcePath: filePath, includeContainerItems, knownRevision: cached?.revision, uiLocale: getUiLocale(), languageMap: getMinecraftTranslations() }, operationId).then(analysis => {
    const received = performance.now();
    if (analysis.cache_unchanged) {
      if (!cached || analysis.source_revision !== cached.revision) throw new Error(t("native.stats.cacheChanged"));
      cached.data.timing = { requestStarted: started, mappedAt: received, cacheReused: true };
      return cached.data;
    }
    const result = mapStructureStats(analysis);
    result.sourceRevision = analysis.source_revision;
    result.timing = { requestStarted: started, mappedAt: received, cacheReused: false };
    if (analysis.source_revision) {
      if (statsCompleted.size >= 8) statsCompleted.delete(statsCompleted.keys().next().value!);
      statsCompleted.set(key, { revision: analysis.source_revision, data: result });
    }
    return result;
  });
  return pending;
}

export function materialDisplayName(material: Pick<MaterialItem, "name">): string { return material.name; }
export function refreshStatisticsNames(data: StatsData): StatsData { return data; }

export async function fetchMaterialRecipeCache(filePath?: string, includeContainerItems = false, operationId = crypto.randomUUID()): Promise<MaterialRecipesResult | null> {
  if (filePath) {
    await initI18n();
    return queryMaterialRecipes({ sourcePath: filePath, includeContainerItems, uiLocale: getUiLocale(), languageMap: getMinecraftTranslations() }, operationId);
  }
  const status = await prepareMaterialRecipes("26.3");
  if (status.status !== "available" || !status.has_recipes) throw new Error(t("native.stats.recipesPending"));
  return null;
}

export function recordStatisticsCommit(id: string, phase: string, actualDuration: number, baseDuration: number, startTime: number, commitTime: number): void {
  console.info(`[NOVA_STATS_REACT] id=${id} phase=${phase} actual_ms=${actualDuration.toFixed(3)} base_ms=${baseDuration.toFixed(3)} start=${startTime.toFixed(3)} commit=${commitTime.toFixed(3)}`);
}

export async function loadStructuralBlocks(filePath: string): Promise<StructuralBlockCount[]> {
  const analysis = await getProjectStats({ sourcePath: filePath, includeContainerItems: false, includeStructuralBlocks: true });
  if (!analysis.structural_block_counts) throw new Error(t("native.stats.structuralDataMissing"));
  // Field adaptation only; structure analysis and counts are authoritative Rust data.
  return analysis.structural_block_counts.map(row => ({ id: row.block_id, count: row.count }));
}

export function hasInventoryMaterialSemantics(analysis: { source_revision?: string; material_semantics?: string } | null | undefined): boolean {
  return typeof analysis?.source_revision === "string" && typeof analysis?.material_semantics === "string" && analysis.material_semantics.startsWith("minecraft-inventory-v9:");
}

function defaultExportName(filePath: string): string {
  return `materials_${(filePath.split(/[\\/]/).pop() || "projection").replace(/\.litematic$/i, "").replace(/[<>:"/\\|?*\x00-\x1f]/g, "_")}`;
}

export async function exportMaterialListWithOptions(filePath: string, options: {
  scope: ApplicationMaterialScope; includeContainerItems: boolean; multiplier: number;
  minecraftVersion: string; terminalRawMaterials: boolean; format: "svg" | "txt" | "html" | "csv";
  webOptions?: MaterialWebExportOptions;
}): Promise<boolean> {
  const extension = options.format === "html" && options.webOptions?.mode === "multi" ? "zip" : options.format;
  const savePath = await saveDialog({ defaultPath: `${defaultExportName(filePath)}.${extension}`, filters: [{ name: extension.toUpperCase(), extensions: [extension] }] });
  if (!savePath) return false;
  await initI18n();
  await exportMaterialList({ sourcePath: filePath, ...options, outputPath: savePath, uiLocale: getUiLocale(), languageMap: getMinecraftTranslations() });
  return true;
}
