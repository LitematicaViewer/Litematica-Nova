import { bindEnumeratorCollections, queryEnumeratorExpression } from "../platform/applicationOperations";
import { getAppDataFilePath, listDirectoryEntries, readAppDataFile, writeAppDataFile } from "../platform/files";
import { listGameResourceRegistry, getSelectedGameDataContext, readActiveGameDataResource } from "./gameResources";
import { readWorkspaceFile } from "./backend";
import { translateBlockId, t } from "./i18n";

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



function normalizeLookupKey(value: string): string { return String(value || "").trim().toLowerCase(); }

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

async function loadBaseCollections(): Promise<EnumeratorCollection[]> {
  const snapshot = await listGameResourceRegistry();
  const context = await getSelectedGameDataContext();
  const custom = snapshot.active_enum_catalog.builtin ? null : snapshot.active_enum_catalog;
  const result: EnumeratorCollection[] = [];
  for (const def of BASE_COLLECTION_DEFS) {
    const relativePath = custom ? `${custom.root_relpath}/${def.fileName}` : "";
    const sourcePath = custom ? await resolveSourcePath(relativePath) : `${context.root.replace(/[\\/]+$/, "")}/derived/enumerator/${def.fileName}`;
    let values: string[] = [];
    let failure = "";
    try {
      const raw = custom ? await readAppDataFile(relativePath) : await readWorkspaceFile(sourcePath);
      const parsed = JSON.parse(raw) as unknown;
      if (!Array.isArray(parsed) || parsed.some(value => typeof value !== "string")) throw new Error(t("native.resources.enumInvalid", { name: def.fileName }));
      values = normalizeStringArray(parsed);
    } catch (error) {
      if (!custom && def.valueType === "block") {
        try {
          const active = await readActiveGameDataResource();
          values = Object.keys((JSON.parse(active.dbText) as { blocks: Record<string, unknown> }).blocks);
        } catch (registryError) { failure = String(registryError); }
      } else { failure = String(error); }
    }
    result.push({
      id: def.id, name: def.name, symbol: def.symbol, category: "base", valueType: def.valueType,
      version: custom?.version || context.game_version, values,
      description: failure || t("native.enumerator.activeCatalog"), updatedAt: context.content_id,
      readOnly: true, missing: !!failure, sourcePath, sourceRelativePath: relativePath,
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
      description: t("native.enumerator.jsonArray"),
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

export async function loadEnumeratorCustomCollections(): Promise<EnumeratorCollection[]> {
  return loadFolderCollections("custom");
}

function targetCustomRelativePath(existing: EnumeratorCollection | null, name: string): string {
  if (existing?.sourceRelativePath) return existing.sourceRelativePath;
  return `${COLLECTION_DIRS.custom}/${slugify(name)}.json`;
}

export async function saveEnumeratorCollection(input: Partial<EnumeratorCollection> & Pick<EnumeratorCollection, "name" | "values">): Promise<EnumeratorCollection> {
  const customCollections = await loadEnumeratorCustomCollections();
  const baseId = String(input.id || "").trim();
  const existing = customCollections.find((collection) => collection.id === baseId) || null;
  const nextValues = uniq(input.values);
  const name = String(input.name || "").trim() || t("native.enumerator.unnamed");
  const relativePath = targetCustomRelativePath(existing, name);
  const next: EnumeratorCollection = {
    id: existing?.id || `custom:${slugify(name)}:${Date.now()}`,
    name,
    symbol: safeSymbol(String(input.symbol || existing?.symbol || "").trim(), existing ? existing.symbol : `C${customCollections.length + 1}`),
    category: "custom",
    valueType: normalizeValueType(input.valueType || existing?.valueType || "mixed") === "unknown"
      ? inferValueType(nextValues)
      : normalizeValueType(input.valueType || existing?.valueType || "mixed"),
    version: String(input.version || existing?.version || "custom").trim() || "custom",
    values: nextValues,
    description: String(input.description || existing?.description || "").trim(),
    updatedAt: nowIso(),
    readOnly: false,
    sourceRelativePath: relativePath,
    sourcePath: await resolveSourcePath(relativePath),
  };

  await writeAppDataFile(relativePath, JSON.stringify({
    id: next.id, name: next.name, symbol: next.symbol, version: next.version,
    value_type: next.valueType, values: next.values, description: next.description, updated_at: next.updatedAt,
  }, null, 2));
  return next;
}

export function createRuntimeProjectionCollection(filePath: string, materials: Array<{ id: string }>): EnumeratorCollection {
  const fileName = filePath.split(/[\\/]/).pop() || "当前统计结果";
  return {
    id: `runtime:${filePath}`,
    name: "当前统计结果",
    symbol: "L",
    category: "runtime",
    valueType: "mixed",
    version: "runtime",
    values: uniq(materials.map((material) => material.id)),
    description: t("native.enumerator.currentStats", { name: fileName }),
    updatedAt: nowIso(),
    readOnly: true,
  };
}

// Each WebView owns one current editor snapshot. Bind changes in order so a
// slow upload for a previous collection cannot replace a newer one.
let bound: { collections: EnumeratorCollection[]; ready: Promise<number> } | undefined;
export async function evaluateEnumeratorExpression(expression: string, collections: EnumeratorCollection[]) {
  if (!expression.trim() || collections.length === 0) return {values: [] as string[], dependencies: [] as string[], error: ""};
  if (bound?.collections !== collections) {
    const ready = (bound?.ready.catch(() => undefined) ?? Promise.resolve()).then(() =>
      bindEnumeratorCollections(collections.map(({id, name, symbol, values}) => ({id, name, symbol, values}))));
    bound = {collections, ready};
    void ready.catch(() => { if (bound?.ready === ready) bound = undefined; });
  }
  const result = await queryEnumeratorExpression(await bound.ready, expression);
  return {values: result.values, dependencies: result.dependencies,
    error: result.errorKey ? t(result.errorKey, result.token == null ? {} : {token: result.token}) : ""};
}

function detectValueType(value: string, membership: Record<string, Set<string>>): EnumeratorValueType {
  if (membership.blocks.has(value)) return "block";
  if (membership.items.has(value)) return "item";
  if (membership.entities.has(value)) return "entity";
  if (membership.enchantments.has(value)) return "enchantment";
  return "unknown";
}

function isItemLikeBlockCollection(collection: EnumeratorCollection): boolean {
  const normalizedId = normalizeLookupKey(collection.id);
  const normalizedName = normalizeLookupKey(collection.name);
  return normalizedId === "custom:itemlike_block" || normalizedName === "itemlike_block";
}

export function buildEnumeratorValueRows(values: string[], collections: EnumeratorCollection[]): EnumeratorValueRow[] {
  const membership = {
    blocks: new Set(collections.find((collection) => collection.id === "base:dv-blocks")?.values || []),
    items: new Set(collections.find((collection) => collection.id === "base:dv-items")?.values || []),
    entities: new Set(collections.find((collection) => collection.id === "base:dv-entities")?.values || []),
    enchantments: new Set(collections.find((collection) => collection.id === "base:dv-enchantments")?.values || []),
  };
  const customCollections = collections.filter((collection) => collection.category === "custom" || collection.category === "runtime");
  const itemLikeBlocks = new Set(
    collections
      .filter((collection) => isItemLikeBlockCollection(collection))
      .flatMap((collection) => collection.values),
  );
  return uniq(values).map((value) => ({
    id: value,
    name: translateBlockId(value) || value,
    iconHint: value,
    iconLookupMode: membership.items.has(value) || itemLikeBlocks.has(value) ? "item_first" : "default",
    type: detectValueType(value, membership),
    customCollections: customCollections
      .filter((collection) => collection.values.includes(value))
      .map((collection) => collection.name),
  }));
}
