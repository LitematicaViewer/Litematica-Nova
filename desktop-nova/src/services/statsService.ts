import { saveDialog } from "../platform/dialogs";
import {
  getProjectMaterials,
  getProjectStats,
  type ApplicationMaterialScope,
} from "../platform/applicationOperations";
import { writeTextFileAbsolute } from "./backend";
import { translateBlockId, translateBuildingType } from "./i18n";

export interface MaterialItem {
  id: string;
  name: string;
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
  nonItemBlocks: NonItemBlock[];
  regions: RegionInfo[];
  layers: LayerInfo[];
  containerScan: ContainerScanSummary;
  raw: any;
}

function mapMaterial(raw: any): MaterialItem {
  const id = raw.block_id || raw.id || "";
  const blockCount = Number(raw.block_count ?? raw.count ?? 0);
  const containerItemCount = Number(raw.container_item_count ?? 0);
  const totalCount = Number(raw.total_count ?? raw.count ?? blockCount + containerItemCount);
  return {
    id,
    name: raw.display_name || translateBlockId(id) || id,
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
      name: entry.display_name || translateBlockId(entry.block_id || entry.id || ""),
      count: Number(entry.block_count ?? entry.count ?? 0),
      reason: entry.reason === "no_item" || entry.material_status === "no_item" ? "no_item" : "unknown",
    }));

  return {
    totalNonAirBlocks: stats.total_non_air_blocks || 0,
    regionCount: stats.total_regions || meta.region_count || 0,
    enclosingSize: meta.enclosing_size || { x: 0, y: 0, z: 0 },
    density,
    buildingType: parsed.derived?.building?.building_type ? translateBuildingType(parsed.derived.building.building_type) : "未提供",
    redstoneRatio: Number.isFinite(parsed.derived?.building?.redstone_ratio) ? parsed.derived.building.redstone_ratio : null,
    fluidRatio: Number.isFinite(parsed.derived?.building?.fluid_ratio) ? parsed.derived.building.fluid_ratio : null,
    entityCount: entitySum.total_entities || 0,
    materials,
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
  const analysis = await getProjectStats({
    sourcePath: filePath,
    includeContainerItems,
  });
  return mapStructureStats(analysis, includeContainerItems);
}

export async function loadMaterialsScope(
  filePath: string,
  scope: ApplicationMaterialScope,
  includeContainerItems = false,
): Promise<MaterialItem[]> {
  const result = await getProjectMaterials({
    sourcePath: filePath,
    scope,
    includeContainerItems,
  });
  return (result.material_items || []).filter((entry: any) => entry.material_status === "item").map(mapMaterial);
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
  if (boxOfBoxes) parts.push(`${boxOfBoxes}箱盒`);
  if (boxes) parts.push(`${boxes}盒`);
  if (stacks) parts.push(`${stacks}组`);
  if (items || parts.length === 0) parts.push(`${items}个`);
  return parts.join(" ");
}

export function formatMaterialQuantity(material: MaterialItem, count: number, asItems = false): string {
  const total = Number.isFinite(count) ? Math.max(0, Math.floor(count)) : 0;
  if (material.materialStatus === "no_item") return `${total.toLocaleString("zh-CN")} 方块`;
  if (asItems || material.materialStatus === "unknown" || !material.canFitContainer || material.hasCustomStackSize) return `${total.toLocaleString("zh-CN")} 个`;
  return formatMaterialUnits(total, material.stackSize);
}

export function hasInventoryMaterialSemantics(analysis: any): boolean {
  return analysis?.material_semantics === "minecraft-26.3-inventory-v2";
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

  let csv = "\uFEFF名称,数字,统计数据\r\n";
  for (const material of materials) {
    if (material.materialStatus !== "item") continue;
    const total = Math.max(0, Math.floor(material.totalCount * multiplier));
    csv += [
      csvEscape(material.name || material.id),
      csvEscape(total),
      csvEscape(formatMaterialQuantity(material, total)),
    ].join(",") + "\r\n";
  }

  await writeTextFileAbsolute(savePath, csv);
  return true;
}
