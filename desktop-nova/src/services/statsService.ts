import { saveDialog } from "../platform/dialogs";
import { writeTextFileAbsolute } from "./backend";
import { getCachedCoreOutput } from "./coreReadCache";
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
  const out = await getCachedCoreOutput(
    filePath,
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
  const out = await getCachedCoreOutput(
    filePath,
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

function formatExportTimestamp(date = new Date()): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return [
    date.getFullYear(),
    pad(date.getMonth() + 1),
    pad(date.getDate()),
  ].join("-") + "_" + [pad(date.getHours()), pad(date.getMinutes()), pad(date.getSeconds())].join(".");
}

function defaultExportBaseName(filePath: string): string {
  const rawName = (filePath.split(/[\\/]/).pop() || "projection").replace(/\.litematic$/i, "");
  const safeName = rawName.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_");
  return `material_list_${safeName}_${formatExportTimestamp()}`;
}

function toExportRows(materials: MaterialItem[], multiplier: number): Array<{ item: string; total: number }> {
  return materials.map((material) => ({
    item: material.name || material.id,
    total: Math.max(0, Math.floor(material.totalCount * multiplier)),
  }));
}

function getDisplayWidth(value: string): number {
  let width = 0;
  for (const char of value) {
    const codePoint = char.codePointAt(0) || 0;
    width += isFullWidthCodePoint(codePoint) ? 2 : 1;
  }
  return width;
}

function isFullWidthCodePoint(codePoint: number): boolean {
  return (
    codePoint >= 0x1100 && (
      codePoint <= 0x115f ||
      codePoint === 0x2329 ||
      codePoint === 0x232a ||
      (codePoint >= 0x2e80 && codePoint <= 0xa4cf && codePoint !== 0x303f) ||
      (codePoint >= 0xac00 && codePoint <= 0xd7a3) ||
      (codePoint >= 0xf900 && codePoint <= 0xfaff) ||
      (codePoint >= 0xfe10 && codePoint <= 0xfe19) ||
      (codePoint >= 0xfe30 && codePoint <= 0xfe6f) ||
      (codePoint >= 0xff00 && codePoint <= 0xff60) ||
      (codePoint >= 0xffe0 && codePoint <= 0xffe6) ||
      (codePoint >= 0x1f300 && codePoint <= 0x1f64f) ||
      (codePoint >= 0x1f900 && codePoint <= 0x1f9ff) ||
      (codePoint >= 0x20000 && codePoint <= 0x3fffd)
    )
  );
}

function padDisplay(value: string, width: number, align: "left" | "right" = "left"): string {
  const padding = Math.max(0, width - getDisplayWidth(value));
  const spaces = " ".repeat(padding);
  return align === "right" ? `${spaces}${value}` : `${value}${spaces}`;
}

function buildArtTable(filePath: string, rows: Array<{ item: string; total: number }>): string {
  const title = `原理图的材料清单 '${(filePath.split(/[\\/]/).pop() || "projection").replace(/\.litematic$/i, "")}'`;
  const totalColumnWidth = Math.max(getDisplayWidth("Total"), ...rows.map((row) => getDisplayWidth(String(row.total))));
  const minItemWidth = Math.max(getDisplayWidth("Item"), ...rows.map((row) => getDisplayWidth(row.item)));
  const titleDrivenItemWidth = Math.max(minItemWidth, getDisplayWidth(title) - totalColumnWidth - 3);
  const itemColumnWidth = Math.max(minItemWidth, titleDrivenItemWidth);
  const titleContentWidth = itemColumnWidth + totalColumnWidth + 3;
  const border = `+${"-".repeat(itemColumnWidth + 2)}+${"-".repeat(totalColumnWidth + 2)}+`;
  const titleRow = `| ${padDisplay(title, titleContentWidth)} |`;
  const headerRow = `| ${padDisplay("Item", itemColumnWidth)} | ${padDisplay("Total", totalColumnWidth, "right")} |`;
  const bodyRows = rows.map((row) => `| ${padDisplay(row.item, itemColumnWidth)} | ${padDisplay(String(row.total), totalColumnWidth, "right")} |`);
  return [titleRow, border, headerRow, border, ...bodyRows, border].join("\r\n");
}

async function saveMaterialsFile(filePath: string, extension: "csv" | "txt", content: string, filterName: string): Promise<boolean> {
  const savePath = await saveDialog({
    defaultPath: `${defaultExportBaseName(filePath)}.${extension}`,
    filters: [{ name: filterName, extensions: [extension] }],
  });
  if (!savePath) return false;
  await writeTextFileAbsolute(savePath, content);
  return true;
}

export async function exportMaterialsCsv(
  filePath: string,
  materials: MaterialItem[],
  multiplier: number,
): Promise<boolean> {
  const rows = toExportRows(materials, multiplier);
  const csv = [
    "\uFEFF\"Item\",\"Total\"",
    ...rows.map((row) => `${csvEscape(row.item)},${row.total}`),
  ].join("\r\n");
  return saveMaterialsFile(filePath, "csv", csv, "CSV");
}

export async function exportMaterialsArtTable(
  filePath: string,
  materials: MaterialItem[],
  multiplier: number,
): Promise<boolean> {
  const rows = toExportRows(materials, multiplier);
  return saveMaterialsFile(filePath, "txt", buildArtTable(filePath, rows), "Text");
}

export async function exportMaterials(
  filePath: string,
  materials: MaterialItem[],
  multiplier: number,
): Promise<boolean> {
  return exportMaterialsCsv(filePath, materials, multiplier);
}
