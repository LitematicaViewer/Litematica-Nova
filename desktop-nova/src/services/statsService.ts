import { saveDialog } from "../platform/dialogs";
import { executeBackend, writeTextFileAbsolute } from "./backend";
import { translateBlockId, translateBuildingType } from "./i18n";

export interface MaterialItem {
  id: string;
  name: string;
  count: number;
  blockCount: number;
  containerItemCount: number;
  totalCount: number;
  iconHint: string;
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
  redstoneRatio: number;
  fluidRatio: number;
  entityCount: number;
  materials: MaterialItem[];
  regions: RegionInfo[];
  layers: LayerInfo[];
  containerScan: ContainerScanSummary;
  raw: any;
}

function withContainerFlag(args: string[], includeContainerItems: boolean): string[] {
  return includeContainerItems ? [...args, "--include-container-items"] : args;
}

function mapMaterial(raw: any): MaterialItem {
  const id = raw.block_id || raw.id || "";
  const blockCount = Number(raw.block_count ?? raw.count ?? 0);
  const containerItemCount = Number(raw.container_item_count ?? 0);
  const totalCount = Number(raw.total_count ?? raw.count ?? blockCount + containerItemCount);
  return {
    id,
    name: translateBlockId(id) || raw.display_name || id,
    count: totalCount,
    blockCount,
    containerItemCount,
    totalCount,
    iconHint: raw.icon_hint || id,
  };
}

export async function loadStructureStats(filePath: string, includeContainerItems = false): Promise<StatsData> {
  const out = await executeBackend(
    "litematica_core.exe",
    withContainerFlag(["stats", "--input", filePath, "--json"], includeContainerItems),
  );
  const parsed = JSON.parse(out);

  const meta = parsed.metadata || {};
  const stats = parsed.structure_stats || {};
  const matList = parsed.material_items || [];
  const regions = parsed.regions || [];
  const layers = parsed.layers_summary || [];
  const entitySum = parsed.entity_summary || {};

  const vol = meta.total_volume || 1;
  const density = vol > 0 ? (stats.total_non_air_blocks || 0) / vol : 0;
  const materials = matList.map(mapMaterial);

  return {
    totalNonAirBlocks: stats.total_non_air_blocks || 0,
    regionCount: stats.total_regions || meta.region_count || 0,
    enclosingSize: meta.enclosing_size || { x: 0, y: 0, z: 0 },
    density,
    buildingType: translateBuildingType(parsed.derived?.building?.building_type || "building"),
    redstoneRatio: parsed.derived?.building?.redstone_ratio || 0,
    fluidRatio: parsed.derived?.building?.fluid_ratio || 0,
    entityCount: entitySum.total_entities || 0,
    materials,
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

export async function loadMaterialsScope(
  filePath: string,
  scopeArgs: string[],
  includeContainerItems = false,
): Promise<MaterialItem[]> {
  const out = await executeBackend(
    "litematica_core.exe",
    withContainerFlag(["materials", "--input", filePath, ...scopeArgs, "--json"], includeContainerItems),
  );
  const parsed = JSON.parse(out);
  return (parsed.material_items || []).map(mapMaterial);
}

export function formatMaterialUnits(count: number): string {
  const boxOfBoxesSize = 64 * 27 * 27;
  const boxSize = 64 * 27;
  const stackSize = 64;
  let remaining = Math.max(0, Math.floor(count));
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
    const total = Math.max(0, Math.floor(material.totalCount * multiplier));
    csv += [
      csvEscape(material.name || material.id),
      csvEscape(total),
      csvEscape(formatMaterialUnits(total)),
    ].join(",") + "\r\n";
  }

  await writeTextFileAbsolute(savePath, csv);
  return true;
}
