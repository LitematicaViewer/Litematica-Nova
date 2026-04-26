import { readWorkspaceFile } from "./backend";

export interface BlockStateDb {
  blocks: Record<string, { properties: Record<string, string[]>; default_properties?: Record<string, string> }>;
}

export interface I18nDb {
  property_keys: Record<string, string>;
  property_values: Record<string, Record<string, string>>;
}

let dbCache: BlockStateDb | null = null;
let i18nCache: I18nDb | null = null;

export async function loadDatabases() {
  if (!dbCache) {
    try {
      dbCache = JSON.parse(await readWorkspaceFile("data/minecraft_blockstates/26.1.json"));
    } catch (e) {
      console.warn(e);
    }
  }
  if (!i18nCache) {
    try {
      i18nCache = JSON.parse(await readWorkspaceFile("data/minecraft_blockstates/26.1.zh_cn.json"));
    } catch (e) {
      console.warn(e);
    }
  }
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
