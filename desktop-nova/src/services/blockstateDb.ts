import { getUiLocale, t } from "./i18n";

import { getSelectedGameDataContext, readActiveGameDataResource } from "./gameResources";
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
let activeGameVersion = "";
let generation = 0;
export function invalidateBlockstateDbCache(): void {
  generation++;
  catalogs.clear();
  loads.clear();
  i18nCache = null;
}
function observeResources(): void {
  if (resourceListenerStarted) return;
  resourceListenerStarted = true;
  void listenEvent("resource-game-data-changed", invalidateBlockstateDbCache).catch(() => { resourceListenerStarted = false; });
}

export async function loadDatabases(version?: string) {
  observeResources();
  const context = await getSelectedGameDataContext();
  activeGameVersion = context.game_version;
  const wanted = version || context.game_version;
  const cacheKey = `${context.content_id}:${wanted}`;
  const startedGeneration = generation;
  if (!loads.has(cacheKey)) loads.set(cacheKey, (async () => {
    if (wanted === context.game_version) {
      const active = await readActiveGameDataResource();
      const db = JSON.parse(active.dbText) as BlockStateDb;
      if (!db.blocks || !Object.keys(db.blocks).length) throw new Error(t("native.db.empty", { version: wanted }));
      const blocks = Object.entries(db.blocks).map(([id, block]) => [id, {
        id, properties: block.properties || {}, defaultState: id + (
          Object.keys(block.default_properties || {}).length
            ? "[" + Object.entries(block.default_properties!).map(([key, value]) => `${key}=${value}`).join(",") + "]" : ""
        ),
      } as BlockDatabaseBlock] as const);
      if (startedGeneration !== generation) return;
      catalogs.set(wanted, new Map(blocks));
      i18nCache = JSON.parse(active.i18nText);
    } else {
      // An explicit projection/conversion-version query keeps its actual version.
      const result = await queryBlockDatabase({ version: wanted });
      if (!result.blocks?.length) throw new Error(t("native.db.notLoaded", { version: wanted }));
      if (startedGeneration !== generation) return;
      catalogs.set(wanted, new Map(result.blocks.map(block => [block.id, block])));
    }
  })().catch(error => { loads.delete(cacheKey); throw error; }));
  await loads.get(cacheKey);
}

export function getDatabaseBlockName(blockId: string, version = activeGameVersion): string | undefined {
  return catalogs.get(version)?.get(blockId)?.name;
}
export function hasBlockDatabase(): boolean {
  return (catalogs.get(activeGameVersion)?.size || 0) > 0;
}

export function getBlockProperties(blockId: string, version = activeGameVersion): Record<string, string[]> {
  return catalogs.get(version)?.get(blockId)?.properties || {};
}

export function getDefaultProperties(blockId: string, version = activeGameVersion): Record<string, string> {
  const state = catalogs.get(version)?.get(blockId)?.defaultState || "";
  const properties = state.match(/\[(.*)\]$/)?.[1];
  return properties ? Object.fromEntries(properties.split(",").map((pair) => pair.split("="))) : {};
}

export function translateKey(key: string): string {
  return getUiLocale() === "en-US" ? key : i18nCache?.property_keys?.[key] || key;
}

export function translateValue(key: string, value: string): string {
  if (value === "$keep") return t("native.db.keepState");
  if (getUiLocale() === "en-US") return value;
  return i18nCache?.property_values?.[key]?.[value] || value;
}

export function getAllBlocks(version = activeGameVersion): string[] {
  return [...(catalogs.get(version)?.keys() || [])];
}
