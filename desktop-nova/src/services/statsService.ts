import { saveDialog } from "../platform/dialogs";
import {
  getProjectMaterials,
  getProjectStats,
  exportMaterialList,
  prepareMaterialRecipes,
  type ApplicationMaterialScope,
  type MaterialWebExportOptions,
} from "../platform/applicationOperations";
import { writeTextFileAbsolute } from "./backend";
import { translateBuildingType, resolveBlockDisplayName, subscribeI18nChanges, t } from "./i18n";
import { windowsPathKey } from "./filePathIdentity";

export interface MaterialItem {
  id: string;
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
  bounds: any;
  size: any;
  position: any;
}

export interface LayerInfo {
  layer: number;
  world_y: number;
  total_non_air_blocks: number;
  unique_block_types: number;
}

export interface StructuralBlockCount { id: string; count: number }

export interface StatsData {
  totalNonAirBlocks: number;
  regionCount: number;
  enclosingSize: { x: number; y: number; z: number };
  density: number;
  buildingType: string;
  redstoneRatio: number | null;
  fluidRatio: number | null;
  entityCount: number;
  materials: MaterialItem[];
  structuralBlocks: StructuralBlockCount[];
  hasStructuralBlockCounts: boolean;
  nonItemBlocks: NonItemBlock[];
  regions: RegionInfo[];
  layers: LayerInfo[];
  containerScan: ContainerScanSummary;
  raw: any;
  timing?: { requestStarted: number; mappedAt: number; cacheReused: boolean };
}

function mapMaterial(raw: any): MaterialItem {
  const id = raw.block_id || raw.id || "";
  const blockCount = Number(raw.block_count ?? raw.count ?? 0);
  const containerItemCount = Number(raw.container_item_count ?? 0);
  const totalCount = Number(raw.total_count ?? raw.count ?? blockCount + containerItemCount);
  return {
    id,
    name: resolveBlockDisplayName(id, raw.display_name),
    sourceDisplayName: String(raw.display_name || ""),
    count: totalCount,
    blockCount,
    containerItemCount,
    totalCount,
    iconHint: raw.icon_hint || id,
    stackSize: Number(raw.stack_size || 1),
    canFitContainer: raw.can_fit_container === true,
    materialStatus: raw.material_status || "unknown",
    hasCustomStackSize: raw.has_custom_stack_size === true,
    regionCounts: (raw.region_counts || []).map((entry: any) => ({
      region: String(entry.region || ""),
      count: Number(entry.count || 0),
    })),
    layerCounts: (raw.layer_counts || []).map((entry: any) => ({
      layer: Number(entry.layer || 0),
      worldY: Number(entry.world_y || 0),
      count: Number(entry.count || 0),
    })),
  };
}

export function mapStructureStats(parsed: any, includeContainerItems = false): StatsData {
  const meta = parsed.metadata || {};
  const stats = parsed.structure_stats || {};
  const matList = parsed.material_items || [];
  const regions = parsed.regions || [];
  const layers = parsed.layers_summary || [];
  const entitySum = parsed.entity_summary || {};

  const vol = meta.total_volume || 1;
  const density = vol > 0 ? (stats.total_non_air_blocks || 0) / vol : 0;
  // Old cached summaries mixed AIR-mapped blocks into the materials array.
  // Never export or bill those rows while a fresh v2 analysis is loading.
  const materials = matList.filter((entry: any) => entry.material_status === "item").map(mapMaterial);
  const nonItemBlocks = (parsed.non_item_blocks || matList.filter((entry: any) => entry.material_status !== "item"))
    .map((entry: any): NonItemBlock => ({
      id: entry.block_id || entry.id || "",
      name: resolveBlockDisplayName(entry.block_id || entry.id || "", entry.display_name),
      sourceDisplayName: String(entry.display_name || ""),
      count: Number(entry.block_count ?? entry.count ?? 0),
      reason: entry.reason === "no_item" || entry.material_status === "no_item" ? "no_item" : "unknown",
    }));

  return {
    totalNonAirBlocks: stats.total_non_air_blocks || 0,
    regionCount: stats.total_regions ?? meta.region_count ?? 0,
    enclosingSize: meta.enclosing_size || { x: 0, y: 0, z: 0 },
    density,
    buildingType: parsed.derived?.building?.building_type ? translateBuildingType(parsed.derived.building.building_type) : t("native.stats.notProvided"),
    redstoneRatio: Number.isFinite(parsed.derived?.building?.redstone_ratio) ? parsed.derived.building.redstone_ratio : null,
    fluidRatio: Number.isFinite(parsed.derived?.building?.fluid_ratio) ? parsed.derived.building.fluid_ratio : null,
    entityCount: entitySum.total_entities || 0,
    materials,
    structuralBlocks: (parsed.structural_block_counts || []).map((row: any) => ({ id: String(row.block_id || ""), count: Number(row.count || 0) })),
    hasStructuralBlockCounts: Array.isArray(parsed.structural_block_counts),
    nonItemBlocks,
    regions,
    layers,
    containerScan: parsed.container_scan || {
      enabled: includeContainerItems,
      containers_scanned: 0,
      item_stacks_scanned: 0,
      warnings: [],
    },
    raw: parsed,
  };
}

export async function loadStructureStats(filePath: string, includeContainerItems = false): Promise<StatsData> {
  const key = JSON.stringify([windowsPathKey(filePath), includeContainerItems]);
  const existing = statsInFlight.get(key);
  if (existing) return existing;
  const cached = statsCompleted.get(key);
  const started = performance.now();
  const pending = getProjectStats({ sourcePath: filePath, includeContainerItems, knownRevision: cached?.data.hasStructuralBlockCounts ? cached.revision : undefined }).then(analysis => {
    const received = performance.now();
    if (analysis.cache_unchanged) {
      if (!cached || analysis.source_revision !== cached.revision) throw new Error(t("native.stats.cacheChanged"));
      cached.data.timing = { requestStarted: started, mappedAt: received, cacheReused: true };
      console.info(`[NOVA_STATS_TIMING] webview_request_roundtrip_ms=${(received - started).toFixed(3)} map_ms=0.000 cache_reused=true`);
      return cached.data;
    }
    const result = mapStructureStats(analysis, includeContainerItems);
    result.timing = { requestStarted: started, mappedAt: performance.now(), cacheReused: false };
    if (analysis.source_revision) {
      statsCompleted.delete(key);
      if (statsCompleted.size >= 8) statsCompleted.delete(statsCompleted.keys().next().value!);
      statsCompleted.set(key, { revision: analysis.source_revision, data: result });
    }
    console.info(`[NOVA_STATS_TIMING] webview_request_roundtrip_ms=${(received - started).toFixed(3)} map_ms=${(performance.now() - received).toFixed(3)} cache_reused=false`);
    return result;
  });
  statsInFlight.set(key, pending);
  try { return await pending; }
  finally { if (statsInFlight.get(key) === pending) statsInFlight.delete(key); }
}

// Completed mappings are reused only after Application Host source-revision
// validation. No full analysis or unvalidated filename cache on page switches.
const statsInFlight = new Map<string, Promise<StatsData>>();
const statsCompleted = new Map<string, { revision: string; data: StatsData }>();
const materialsInFlight = new Map<string, Promise<MaterialItem[]>>();

export function materialDisplayName(material: Pick<MaterialItem, "id" | "name" | "sourceDisplayName">): string {
  return resolveBlockDisplayName(material.id, material.sourceDisplayName ?? material.name);
}

/** Keep quantities, channels, scopes and source revisions untouched. */
export function refreshStatisticsNames(data: StatsData): StatsData {
  data.buildingType = data.raw?.derived?.building?.building_type ? translateBuildingType(data.raw.derived.building.building_type) : t("native.stats.notProvided");
  for (const material of data.materials) material.name = materialDisplayName(material);
  for (const block of data.nonItemBlocks) block.name = materialDisplayName(block);
  return data;
}

subscribeI18nChanges(() => {
  for (const cached of statsCompleted.values()) refreshStatisticsNames(cached.data);
});

export async function fetchMaterialRecipeCache(): Promise<void> {
  const status = await prepareMaterialRecipes("26.3");
  if (status.status !== "available" || !status.has_recipes) throw new Error(t("native.stats.recipesPending"));
}

export function recordStatisticsCommit(id: string, phase: string, actualDuration: number, baseDuration: number, startTime: number, commitTime: number): void {
  console.info(`[NOVA_STATS_REACT] id=${id} phase=${phase} actual_ms=${actualDuration.toFixed(3)} base_ms=${baseDuration.toFixed(3)} start=${startTime.toFixed(3)} commit=${commitTime.toFixed(3)}`);
}

export async function loadMaterialsScope(
  filePath: string,
  scope: ApplicationMaterialScope,
  includeContainerItems = false,
): Promise<MaterialItem[]> {
  const key = JSON.stringify([windowsPathKey(filePath), scope.kind, scope.value, includeContainerItems]);
  const existing = materialsInFlight.get(key);
  if (existing) return existing;
  const pending = getProjectMaterials({
    sourcePath: filePath,
    scope,
    includeContainerItems,
  }).then(result => (result.material_items || []).filter((entry: any) => entry.material_status === "item").map(mapMaterial));
  materialsInFlight.set(key, pending);
  try { return await pending; }
  finally { if (materialsInFlight.get(key) === pending) materialsInFlight.delete(key); }
}

export function formatMaterialUnits(count: number, stackSize = 64): string {
  stackSize = Number.isFinite(stackSize) ? Math.max(1, Math.floor(stackSize)) : 1;
  const boxOfBoxesSize = stackSize * 27 * 27;
  const boxSize = stackSize * 27;
  let remaining = Number.isFinite(count) ? Math.max(0, Math.floor(count)) : 0;
  const boxOfBoxes = Math.floor(remaining / boxOfBoxesSize);
  remaining %= boxOfBoxesSize;
  const boxes = Math.floor(remaining / boxSize);
  remaining %= boxSize;
  const stacks = Math.floor(remaining / stackSize);
  const items = remaining % stackSize;

  const parts: string[] = [];
  if (boxOfBoxes) parts.push(t("native.units.boxOfBoxes", { count: boxOfBoxes }));
  if (boxes) parts.push(t("native.units.boxes", { count: boxes }));
  if (stacks) parts.push(t("native.units.stacks", { count: stacks }));
  if (items || parts.length === 0) parts.push(t("native.units.items", { count: items }));
  return parts.join(" ");
}

export function formatMaterialQuantity(material: MaterialItem, count: number, asItems = false): string {
  const total = Number.isFinite(count) ? Math.max(0, Math.floor(count)) : 0;
  if (material.materialStatus === "no_item") return t("native.units.blocks", { count: total });
  if (asItems || material.materialStatus === "unknown" || !material.canFitContainer || material.hasCustomStackSize) return t("native.units.items", { count: total });
  return formatMaterialUnits(total, material.stackSize);
}

export async function loadStructuralBlocks(filePath: string): Promise<StructuralBlockCount[]> {
  const data = await loadStructureStats(filePath);
  if (!data.hasStructuralBlockCounts) throw new Error(t("native.stats.structuralDataMissing"));
  return data.structuralBlocks;
}

export function hasInventoryMaterialSemantics(analysis: any): boolean {
  return analysis?.material_semantics === "minecraft-inventory-v5";
}

function csvEscape(value: string | number): string {
  const text = String(value);
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

function defaultExportName(filePath: string): string {
  const rawName = (filePath.split(/[\\/]/).pop() || "projection").replace(/\.litematic$/i, "");
  const safeName = rawName.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_");
  return `materials_${safeName}.csv`;
}

export async function exportMaterials(
  filePath: string,
  materials: MaterialItem[],
  multiplier: number,
): Promise<boolean> {
  const savePath = await saveDialog({
    defaultPath: defaultExportName(filePath),
    filters: [{ name: "CSV", extensions: ["csv"] }],
  });
  if (!savePath) return false;

  let csv = `\uFEFF${t("native.stats.csvHeaders")}\r\n`;
  for (const material of materials) {
    if (material.materialStatus !== "item") continue;
    const total = Math.max(0, Math.floor(material.totalCount * multiplier));
    csv += [
      csvEscape(materialDisplayName(material) || material.id),
      csvEscape(total),
      csvEscape(formatMaterialQuantity(material, total)),
    ].join(",") + "\r\n";
  }

  await writeTextFileAbsolute(savePath, csv);
  return true;
}

export async function exportMaterialListWithOptions(filePath: string, options: {
  scope: ApplicationMaterialScope; includeContainerItems: boolean; multiplier: number;
  minecraftVersion: string; terminalRawMaterials: boolean; format: "svg" | "txt" | "html" | "csv";
  webOptions?: MaterialWebExportOptions;
}): Promise<boolean> {
  const multiplayer = options.format === "html" && options.webOptions?.mode === "multi";
  if (multiplayer && (!options.webOptions?.accessPassword?.trim() || !options.webOptions?.adminPassword?.trim())) {
    throw new Error(t("native.stats.multiPasswords"));
  }
  if (multiplayer && !options.webOptions?.targets.length) throw new Error(t("native.stats.platformRequired"));
  const extension = multiplayer ? "zip" : options.format;
  const savePath = await saveDialog({
    defaultPath: defaultExportName(filePath).replace(/\.csv$/, `.${extension}`),
    filters: [{ name: extension.toUpperCase(), extensions: [extension] }],
  });
  if (!savePath) return false;
  await exportMaterialList({ sourcePath: filePath, ...options, outputPath: savePath });
  return true;
}

export function filterMaterialItems(materials: readonly MaterialItem[], query: string): MaterialItem[] {
  const search = query.trim().toLocaleLowerCase();
  return materials.filter(material => `${material.name} ${material.id}`.toLocaleLowerCase().includes(search));
}
