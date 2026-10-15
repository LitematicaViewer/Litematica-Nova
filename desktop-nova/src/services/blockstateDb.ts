import { readWorkspaceFile } from "./backend";
import { listGameResourceRegistry, readActiveGameDataResource } from "./gameResources";
import { listenEvent } from "../platform/events";
import { queryBlockDatabase, type BlockDatabaseBlock } from "../platform/applicationOperations";

export interface BlockStateDb {
  blocks: Record<string, { properties: Record<string, string[]>; default_properties?: Record<string, string> }>;
}

export interface I18nDb {
  property_keys: Record<string, string>;
  property_values: Record<string, Record<string, string>>;
}

const catalogs = new Map<string, Map<string, BlockDatabaseBlock>>();
const loads = new Map<string, Promise<void>>();
let i18nCache: I18nDb | null = null;
let resourceListenerStarted = false;
export function invalidateBlockstateDbCache(): void {
  catalogs.clear();
  loads.clear();
  i18nCache = null;
}
function observeResources(): void {
  if (resourceListenerStarted) return;
  resourceListenerStarted = true;
  void listenEvent("resource-game-data-changed", invalidateBlockstateDbCache).catch(() => { resourceListenerStarted = false; });
}

export async function loadDatabases(version = "26.3") {
  observeResources();
  if (!loads.has(version)) loads.set(version, (async () => {
    const snapshot = await listGameResourceRegistry();
    const entry = snapshot.active_game_data;
    if (!entry.builtin && entry.version === version) {
      const active = await readActiveGameDataResource();
      const db = JSON.parse(active.dbText) as BlockStateDb;
      if (!db.blocks || !Object.keys(db.blocks).length) throw new Error(`Minecraft ${version} 导入方块数据为空。`);
      const blocks = Object.entries(db.blocks).map(([id, block]) => [id, {
        id, properties: block.properties || {}, defaultState: id + (
          Object.keys(block.default_properties || {}).length
            ? "[" + Object.entries(block.default_properties!).map(([key, value]) => `${key}=${value}`).join(",") + "]"
            : ""
        ),
      } as BlockDatabaseBlock] as const);
      catalogs.set(version, new Map(blocks));
      i18nCache = JSON.parse(active.i18nText);
    } else {
      const result = await queryBlockDatabase({ version });
      if (!result.blocks?.length) throw new Error(`Minecraft ${version} 方块数据未加载，请重试。`);
      catalogs.set(version, new Map(result.blocks.map((block) => [block.id, block])));
      if (!i18nCache) i18nCache = JSON.parse(await readWorkspaceFile("data/minecraft_blockstates/26.1.zh_cn.json"));
    }
  })().catch((error) => { loads.delete(version); throw error; }));
  await loads.get(version);
}

export function getDatabaseBlockName(blockId: string, version = "26.3"): string | undefined {
  return catalogs.get(version)?.get(blockId)?.name;
}
export function hasBlockDatabase(): boolean {
  return (catalogs.get("26.3")?.size || 0) > 0;
}

export function getBlockProperties(blockId: string, version = "26.3"): Record<string, string[]> {
  return catalogs.get(version)?.get(blockId)?.properties || {};
}

export function getDefaultProperties(blockId: string, version = "26.3"): Record<string, string> {
  const state = catalogs.get(version)?.get(blockId)?.defaultState || "";
  const properties = state.match(/\[(.*)\]$/)?.[1];
  return properties ? Object.fromEntries(properties.split(",").map((pair) => pair.split("="))) : {};
}

export function translateKey(key: string): string {
  return i18nCache?.property_keys?.[key] || key;
}

export function translateValue(key: string, value: string): string {
  if (value === "$keep") return "保持原状态";
  return i18nCache?.property_values?.[key]?.[value] || value;
}

export function getAllBlocks(version = "26.3"): string[] {
  return [...(catalogs.get(version)?.keys() || [])];
}
