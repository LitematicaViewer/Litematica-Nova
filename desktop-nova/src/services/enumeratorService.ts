import { getAppDataFilePath, listDirectoryEntries, readAppDataFile } from "../platform/files";
import { readWorkspaceFile } from "../platform/files";
import { getAllBlocks, loadDatabases } from "./blockstateDb";
import { translateBlockId } from "./i18n";

export type EnumeratorCollectionCategory = "base" | "version" | "system_enum" | "creative" | "map" | "custom" | "runtime";
export type EnumeratorValueType = "block" | "item" | "entity" | "enchantment" | "mixed" | "unknown";

export interface EnumeratorCollection {
  id: string;
  name: string;
  symbol: string;
  category: EnumeratorCollectionCategory;
  valueType: EnumeratorValueType;
  version: string;
  values: string[];
  description: string;
  updatedAt: string;
  readOnly: boolean;
  missing?: boolean;
  sourcePath?: string;
  sourceRelativePath?: string;
}

export interface EnumeratorValueRow {
  id: string;
  name: string;
  iconHint: string;
  iconLookupMode: "default" | "item_first";
  type: EnumeratorValueType;
  customCollections: string[];
}

interface PersistedEnumeratorCollection {
  id?: string;
  name?: string;
  symbol?: string;
  version?: string;
  value_type?: string;
  values?: unknown;
  description?: string;
  updated_at?: string;
}

interface ActiveLanguageMap {
  blockIds: string[];
  itemIds: string[];
  entityIds: string[];
  enchantmentIds: string[];
}

const DEFAULT_ENUM_ROOTS = ["enumerator/base", "enumerator/base/wiki", "enumerator/base/builtin"];
const COLLECTION_DIRS: Record<Exclude<EnumeratorCollectionCategory, "base" | "runtime">, string> = {
  version: "enumerator/version",
  system_enum: "enumerator/system_enum",
  creative: "enumerator/creative",
  map: "enumerator/map",
  custom: "enumerator/custom",
};

const BASE_COLLECTION_DEFS = [
  {
    id: "base:dv-blocks",
    fileName: "DV_Blocks.json",
    name: "方块全集",
    symbol: "B",
    valueType: "block" as const,
  },
  {
    id: "base:dv-items",
    fileName: "DV_Items.json",
    name: "物品全集",
    symbol: "I",
    valueType: "item" as const,
  },
  {
    id: "base:dv-enchantments",
    fileName: "DV_Enchantments.json",
    name: "魔咒全集",
    symbol: "E",
    valueType: "enchantment" as const,
  },
  {
    id: "base:dv-entities",
    fileName: "DV_Entities.json",
    name: "实体全集",
    symbol: "M",
    valueType: "entity" as const,
  },
];

function uniq(values: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const value of values) {
    const normalized = String(value || "").trim();
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    out.push(normalized);
  }
  return out;
}

function nowIso(): string {
  return new Date().toISOString();
}

function normalizeValueType(value: unknown): EnumeratorValueType {
  switch (String(value || "").trim().toLowerCase()) {
    case "block":
      return "block";
    case "item":
      return "item";
    case "entity":
      return "entity";
    case "enchantment":
      return "enchantment";
    case "mixed":
      return "mixed";
    default:
      return "unknown";
  }
}

function normalizeStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return uniq(value.map((item) => normalizeCollectionValue(item)).filter(Boolean));
}

function slugify(value: string): string {
  const compact = String(value || "")
    .trim()
    .toLowerCase()
    .replace(/[\s\p{P}\p{S}]+/gu, "_")
    .replace(/[^a-z0-9_\u4e00-\u9fff]/g, "")
    .replace(/_+/g, "_")
    .replace(/^_|_$/g, "");
  return compact || "collection";
}

function safeSymbol(value: string, fallback: string): string {
  const trimmed = String(value || "").trim();
  return trimmed || fallback;
}

function normalizeLookupKey(value: string): string {
  return String(value || "").trim().toLowerCase();
}

function normalizeCollectionValue(value: unknown): string {
  const normalized = String(value || "").trim();
  if (!normalized) return "";
  if (normalized.startsWith("E/")) {
    const entityId = normalized.slice(2).trim();
    if (!entityId) return "";
    return entityId.includes(":") ? `E/${entityId.toLowerCase()}` : `E/minecraft:${entityId.toLowerCase()}`;
  }
  if (normalized.includes(":")) {
    return normalized.toLowerCase();
  }
  return `minecraft:${normalized.toLowerCase()}`;
}

function itemKeyToId(prefix: string, key: string): string {
  return `minecraft:${key.slice(prefix.length).replace(/\./g, ":")}`;
}

function readJson<T>(rawText: string | null): T | null {
  if (!rawText) return null;
  try {
    return JSON.parse(rawText) as T;
  } catch {
    return null;
  }
}

function pathParent(path: string): string {
  const normalized = String(path || "").replace(/[\\/]+$/, "");
  const separatorIndex = Math.max(normalized.lastIndexOf("\\"), normalized.lastIndexOf("/"));
  return separatorIndex >= 0 ? normalized.slice(0, separatorIndex) : normalized;
}

function fileBaseName(fileName: string): string {
  return String(fileName || "").replace(/\.[^.]+$/, "");
}

async function readJsonFile(relativePath: string): Promise<string | null> {
  try {
    return await readAppDataFile(relativePath);
  } catch {
    return null;
  }
}

async function resolveSourcePath(relativePath: string): Promise<string> {
  try {
    return await getAppDataFilePath(relativePath);
  } catch {
    return "";
  }
}

async function resolveCollectionDir(relativeDir: string): Promise<{ absDir: string; relativeDir: string }> {
  const markerPath = await getAppDataFilePath(`${relativeDir}/.dir`);
  return {
    absDir: pathParent(markerPath),
    relativeDir,
  };
}

async function loadActiveLanguageMap(): Promise<ActiveLanguageMap> {
  const raw = await readWorkspaceFile("pack-in/lang/zh_cn.json").catch(() => "");
  const parsed = readJson<Record<string, string>>(raw) || {};
  const blocks: string[] = [];
  const items: string[] = [];
  const entities: string[] = [];
  const enchantments: string[] = [];

  for (const key of Object.keys(parsed)) {
    if (key.startsWith("block.minecraft.")) {
      blocks.push(itemKeyToId("block.minecraft.", key));
      continue;
    }
    if (key.startsWith("item.minecraft.")) {
      items.push(itemKeyToId("item.minecraft.", key));
      continue;
    }
    if (key.startsWith("entity.minecraft.")) {
      entities.push(itemKeyToId("entity.minecraft.", key));
      continue;
    }
    if (key.startsWith("enchantment.minecraft.")) {
      enchantments.push(itemKeyToId("enchantment.minecraft.", key));
    }
  }

  return {
    blockIds: uniq(blocks),
    itemIds: uniq(items),
    entityIds: uniq(entities),
    enchantmentIds: uniq(enchantments),
  };
}

async function loadActiveBlocks(): Promise<{ version: string; values: string[] }> {
  await loadDatabases();
  return { version: "bundled", values: getAllBlocks() };
}

async function loadBaseCollections(): Promise<EnumeratorCollection[]> {
  const roots = uniq([
    ...DEFAULT_ENUM_ROOTS,
  ].filter(Boolean));
  const languageMap = await loadActiveLanguageMap();
  const activeBlocks = await loadActiveBlocks();
  const fallbackByType: Record<string, string[]> = {
    block: activeBlocks.values,
    item: languageMap.itemIds,
    entity: languageMap.entityIds,
    enchantment: languageMap.enchantmentIds,
  };

  const result: EnumeratorCollection[] = [];
  for (const def of BASE_COLLECTION_DEFS) {
    let values: string[] = [];
    let sourcePath = "";
    let sourceRelativePath = "";
    for (const root of roots) {
      const relativePath = `${root}/${def.fileName}`;
      const raw = await readJsonFile(relativePath);
      const parsed = readJson<string[]>(raw);
      if (parsed && parsed.length) {
        values = normalizeStringArray(parsed);
        sourcePath = await resolveSourcePath(relativePath);
        sourceRelativePath = relativePath;
        break;
      }
    }

    if (!values.length) {
      values = uniq(fallbackByType[def.valueType] || []);
    }

    result.push({
      id: def.id,
      name: def.name,
      symbol: def.symbol,
      category: "base",
      valueType: def.valueType,
      version: values.length ? "builtin" : "missing",
      values,
      description: values.length
        ? (sourcePath ? "来自随程序提供的枚举资源。" : "由本地游戏数据/语言资源回填。")
        : "基础全集文件缺失，且无法从当前资源回填。",
      updatedAt: nowIso(),
      readOnly: true,
      missing: values.length === 0,
      sourcePath,
      sourceRelativePath,
    });
  }
  return result;
}

function inferValueType(values: string[]): EnumeratorValueType {
  if (values.length === 0) return "mixed";
  if (values.every((value) => value.startsWith("minecraft:"))) return "mixed";
  return "unknown";
}

function normalizeFolderCollection(
  category: Exclude<EnumeratorCollectionCategory, "base" | "runtime">,
  fileName: string,
  relativePath: string,
  sourcePath: string,
  raw: string,
  index: number,
): EnumeratorCollection | null {
  const parsed = readJson<PersistedEnumeratorCollection | string[]>(raw);
  if (Array.isArray(parsed)) {
    const values = normalizeStringArray(parsed);
    return {
      id: `${category}:${slugify(fileBaseName(fileName))}`,
      name: fileBaseName(fileName),
      symbol: safeSymbol("", `${category.slice(0, 1).toUpperCase()}${index + 1}`),
      category,
      valueType: inferValueType(values),
      version: category === "version" ? fileBaseName(fileName) : category,
      values,
      description: "从枚举器目录中的 JSON 字符串数组读取。",
      updatedAt: nowIso(),
      readOnly: category !== "custom",
      sourcePath,
      sourceRelativePath: relativePath,
    };
  }

  if (!parsed || typeof parsed !== "object") return null;
  const name = String(parsed.name || "").trim() || fileBaseName(fileName);
  const values = normalizeStringArray(parsed.values);
  return {
    id: String(parsed.id || "").trim() || `${category}:${slugify(fileBaseName(fileName))}`,
    name,
    symbol: safeSymbol(String(parsed.symbol || "").trim(), `${category.slice(0, 1).toUpperCase()}${index + 1}`),
    category,
    valueType: normalizeValueType(parsed.value_type) === "unknown" ? inferValueType(values) : normalizeValueType(parsed.value_type),
    version: String(parsed.version || "").trim() || (category === "version" ? fileBaseName(fileName) : category),
    values,
    description: String(parsed.description || "").trim(),
    updatedAt: String(parsed.updated_at || "").trim() || nowIso(),
    readOnly: category !== "custom",
    sourcePath,
    sourceRelativePath: relativePath,
  };
}

async function loadFolderCollections(category: Exclude<EnumeratorCollectionCategory, "base" | "runtime">): Promise<EnumeratorCollection[]> {
  const dir = await resolveCollectionDir(COLLECTION_DIRS[category]);
  const entries = await listDirectoryEntries(dir.absDir).catch(() => []);
  const jsonFiles = entries
    .filter((entry) => entry.is_file && entry.extension.toLowerCase() === "json")
    .sort((a, b) => a.name.localeCompare(b.name, "zh-CN"));

  const collections: EnumeratorCollection[] = [];
  for (let index = 0; index < jsonFiles.length; index += 1) {
    const entry = jsonFiles[index];
    const relativePath = `${dir.relativeDir}/${entry.name}`;
    const raw = await readJsonFile(relativePath);
    if (!raw) continue;
    const collection = normalizeFolderCollection(category, entry.name, relativePath, entry.path, raw, index);
    if (!collection) continue;
    collections.push(collection);
  }
  return collections;
}

export async function loadEnumeratorCollections(runtimeCollections: EnumeratorCollection[] = []): Promise<EnumeratorCollection[]> {
  const [baseCollections, versionCollections, systemCollections, creativeCollections, mapCollections, customCollections] = await Promise.all([
    loadBaseCollections(),
    loadFolderCollections("version"),
    loadFolderCollections("system_enum"),
    loadFolderCollections("creative"),
    loadFolderCollections("map"),
    loadFolderCollections("custom"),
  ]);
  return [
    ...baseCollections,
    ...versionCollections,
    ...systemCollections,
    ...creativeCollections,
    ...mapCollections,
    ...runtimeCollections,
    ...customCollections,
  ];
}

/**
 * Builds a block-to-color lookup from map collections named like `map_707070_stone`.
 *
 * @param collections Enumerator collections to inspect.
 * @returns Normalized block IDs mapped to their six-digit map colors.
 */
export function buildMapColorLookup(collections: EnumeratorCollection[]): Map<string, string> {
  const colors = new Map<string, string>();
  for (const collection of collections) {
    if (collection.category !== "map") continue;
    const match = collection.name.trim().match(/^map_([0-9a-f]{6})(?:_|$)/i);
    if (!match) continue;
    const color = `#${match[1].toLowerCase()}`;
    for (const value of collection.values) {
      const normalizedValue = normalizeCollectionValue(value);
      if (!normalizedValue || colors.has(normalizedValue)) continue;
      colors.set(normalizedValue, color);
    }
  }
  return colors;
}
