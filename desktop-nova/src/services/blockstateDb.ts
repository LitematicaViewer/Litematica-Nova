import { readActiveGameDataResource } from "./gameResources";

export interface BlockStateDb {
  blocks: Record<string, { properties: Record<string, string[]>; default_properties?: Record<string, string> }>;
}

export interface I18nDb {
  property_keys: Record<string, string>;
  property_values: Record<string, Record<string, string>>;
}

let dbCache: BlockStateDb | null = null;
let i18nCache: I18nDb | null = null;

/**
 * Loads the active BlockState database and i18n database into memory.
 */
export async function loadDatabases(force = false): Promise<void> {
  if (!force && dbCache && i18nCache) return;
  try {
    const active = await readActiveGameDataResource();
    dbCache = JSON.parse(active.dbText);
    i18nCache = JSON.parse(active.i18nText);
  } catch (e) {
    console.warn(e);
    if (force) {
      dbCache = null;
      i18nCache = null;
    }
  }
}

/**
 * Clears the BlockState and property translation caches.
 */
export function invalidateBlockstateDbCache(): void {
  dbCache = null;
  i18nCache = null;
}

/**
 * Returns whether a usable BlockState database is currently loaded.
 */
export function hasBlockDatabase(): boolean {
  return !!dbCache && Object.keys(dbCache.blocks || {}).length > 0;
}

/**
 * Gets all declared properties for a block id.
 */
export function getBlockProperties(blockId: string): Record<string, string[]> {
  return dbCache?.blocks[blockId]?.properties || {};
}

/**
 * Gets the default property values for a block id.
 */
export function getDefaultProperties(blockId: string): Record<string, string> {
  return dbCache?.blocks[blockId]?.default_properties || {};
}

/**
 * Translates a BlockState property key with the active game data i18n map.
 */
export function translateKey(key: string): string {
  return i18nCache?.property_keys?.[key] || key;
}

/**
 * Translates a BlockState property value with the active game data i18n map.
 */
export function translateValue(key: string, value: string): string {
  if (value === "$keep") return "保持原状态";
  return i18nCache?.property_values?.[key]?.[value] || value;
}

/**
 * Lists every block id in the loaded BlockState database.
 */
export function getAllBlocks(): string[] {
  return Object.keys(dbCache?.blocks || {});
}
