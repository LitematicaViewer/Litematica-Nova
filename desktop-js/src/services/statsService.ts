import { executeBackend } from "./backend";
import { writeWorkspaceFile } from "./backend";
import { save } from "@tauri-apps/plugin-dialog";
import { translateBlockId, translateBuildingType } from "./i18n";

export interface MaterialItem {
  id: string;
  name: string;
  count: number;
  iconHint: string;
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
  enclosingSize: { x: number, y: number, z: number };
  density: number;
  buildingType: string;
  redstoneRatio: number;
  fluidRatio: number;
  entityCount: number;
  materials: MaterialItem[];
  regions: RegionInfo[];
  layers: LayerInfo[];
  raw: any;
}

export async function loadStructureStats(filePath: string): Promise<StatsData> {
  const out = await executeBackend("litematica_core.exe", ["stats", "--input", filePath, "--json"]);
  const parsed = JSON.parse(out);
  
  const meta = parsed.metadata || {};
  const stats = parsed.structure_stats || {};
  const matList = parsed.material_items || [];
  const regions = parsed.regions || [];
  const layers = parsed.layers_summary || [];
  const entitySum = parsed.entity_summary || {};
  
  // Note: the backend currently doesn't output fluid_ratio, redstone_ratio, density in "stats"
  // Let's compute density simply:
  const vol = meta.total_volume || 1;
  const density = vol > 0 ? (stats.total_non_air_blocks || 0) / vol : 0;

  const materials = matList.map((m: any) => ({
    id: m.block_id,
    name: translateBlockId(m.block_id),
    count: m.count,
    iconHint: m.icon_hint || m.block_id
  }));
  
  return {
    totalNonAirBlocks: stats.total_non_air_blocks || 0,
    regionCount: stats.total_regions || meta.region_count || 0,
    enclosingSize: meta.enclosing_size || { x:0, y:0, z:0 },
    density,
    buildingType: translateBuildingType(parsed.derived?.building?.building_type || "building"), // Fallback 
    redstoneRatio: parsed.derived?.building?.redstone_ratio || 0,
    fluidRatio: parsed.derived?.building?.fluid_ratio || 0,
    entityCount: entitySum.total_entities || 0,
    materials,
    regions,
    layers,
    raw: parsed
  };
}

export async function loadMaterialsScope(filePath: string, scopeArgs: string[]): Promise<MaterialItem[]> {
  const out = await executeBackend("litematica_core.exe", ["materials", "--input", filePath, ...scopeArgs, "--json"]);
  const parsed = JSON.parse(out);
  const matList = parsed.material_items || [];
  return matList.map((m: any) => ({
    id: m.block_id,
    name: translateBlockId(m.block_id),
    count: m.count,
    iconHint: m.icon_hint || m.block_id
  }));
}


export async function exportMaterials(filePath: string, materials: any[], multiplier: number, includeEntities: boolean) {
  const savePath = await save({ filters: [{ name: "CSV", extensions: ["csv"] }] });
  if (!savePath) return false;
  
  let csv = "名称,ID,总计,潜影盒估算\n";
  for (const m of materials) {
    const total = m.count * multiplier;
    const shulker = (total / (64 * 27)).toFixed(2);
    csv += `${m.name},${m.id},${total},${shulker}\n`;
  }
  
  await writeWorkspaceFile(savePath, csv);
  return true;
}
