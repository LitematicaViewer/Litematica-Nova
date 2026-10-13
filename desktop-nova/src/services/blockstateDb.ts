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
 * Loads the active BlockState database and its property translations from the
 * game-resource registry, so activating another `game_data` entry takes effect
 * after `invalidateBlockstateDbCache()` + `loadDatabases()`.
 */
export async function loadDatabases() {
  if (dbCache && i18nCache) return;
  try {
    const active = await readActiveGameDataResource();
    dbCache = JSON.parse(active.dbText);
    i18nCache = JSON.parse(active.i18nText);
  } catch (e) {
    console.warn(e);
    // Leave both caches empty so a later call can retry after the user fixes
    // the active resource.
    dbCache = null;
    i18nCache = null;
  }
}

/** Drops the cached BlockState database so the next load re-reads the active resource. */
export function invalidateBlockstateDbCache(): void {
  dbCache = null;
  i18nCache = null;
}

export function hasBlockDatabase(): boolean {
  return !!dbCache && Object.keys(dbCache.blocks || {}).length > 0;
}

export function getBlockProperties(blockId: string): Record<string, string[]> {
  return dbCache?.blocks[blockId]?.properties || {};
}

export function getDefaultProperties(blockId: string): Record<string, string> {
  return dbCache?.blocks[blockId]?.default_properties || {};
}

export function translateKey(key: string): string {
  return i18nCache?.property_keys?.[key] || key;
}

export function translateValue(key: string, value: string): string {
  if (value === "$keep") return "保持原状态";
  return i18nCache?.property_values?.[key]?.[value] || value;
}

export function getAllBlocks(): string[] {
  return Object.keys(dbCache?.blocks || {});
}
