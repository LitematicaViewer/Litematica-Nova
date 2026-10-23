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

function resolveCollectionToken(token: string, collections: EnumeratorCollection[]): EnumeratorCollection | null {
  const lookup = normalizeLookupKey(token);
  if (!lookup) return null;
  return collections.find((collection) =>
    normalizeLookupKey(collection.id) === lookup ||
    normalizeLookupKey(collection.name) === lookup ||
    normalizeLookupKey(collection.symbol) === lookup,
  ) || null;
}

function validateWildcardToken(token: string): void {
  const lookup = normalizeLookupKey(token);
  const firstWildcardIndex = lookup.indexOf("*");
  const lastWildcardIndex = lookup.lastIndexOf("*");
  if (firstWildcardIndex < 0) return;
  if (firstWildcardIndex !== 0 && firstWildcardIndex !== lookup.length - 1) {
    throw new Error(t("native.enumerator.wildcardEdge", { token }));
  }
  if (lastWildcardIndex !== firstWildcardIndex && lastWildcardIndex !== lookup.length - 1) {
    throw new Error(t("native.enumerator.wildcardEdge", { token }));
  }
}

function isWildcardCollectionToken(token: string): boolean {
  return normalizeLookupKey(token).includes("*");
}

function resolveWildcardCollections(token: string, collections: EnumeratorCollection[]): EnumeratorCollection[] {
  const pattern = normalizeLookupKey(token);
  validateWildcardToken(pattern);
  const firstWildcardIndex = pattern.indexOf("*");
  const prefix = pattern.startsWith("*") ? "" : pattern.slice(0, firstWildcardIndex);
  const suffix = pattern.endsWith("*") ? "" : pattern.slice(pattern.lastIndexOf("*") + 1);
  return collections.filter((collection) => {
    const name = normalizeLookupKey(collection.name);
    return name.startsWith(prefix) && name.endsWith(suffix) && name.length >= prefix.length + suffix.length;
  });
}

function tokenizeExpression(expression: string): string[] {
  const tokens: string[] = [];
  let buffer = "";
  const flush = () => {
    const value = buffer.trim();
    if (value) tokens.push(value);
    buffer = "";
  };

  for (const char of expression) {
    if (char === "(" || char === ")" || char === "∪" || char === "|" || char === "∩" || char === "&" || char === "-") {
      flush();
      tokens.push(char);
      continue;
    }
    buffer += char;
  }
  flush();
  return tokens;
}

function validateWildcardParentheses(tokens: string[]): void {
  tokens.forEach((token, index) => {
    if (!isWildcardCollectionToken(token)) return;
    if (tokens[index - 1] !== "(" || tokens[index + 1] !== ")") {
      throw new Error(t("native.enumerator.wildcardGroup", { token }));
    }
  });
}

function precedence(operator: string): number {
  if (operator === "∩" || operator === "&") return 2;
  if (operator === "∪" || operator === "|" || operator === "-") return 1;
  return 0;
}

function toRpn(tokens: string[]): string[] {
  const output: string[] = [];
  const operators: string[] = [];
  for (const token of tokens) {
    if (token === "(") {
      operators.push(token);
      continue;
    }
    if (token === ")") {
      while (operators.length && operators[operators.length - 1] !== "(") {
        output.push(operators.pop()!);
      }
      if (!operators.length) throw new Error(t("native.enumerator.parentheses"));
      operators.pop();
      continue;
    }
    if (token === "∪" || token === "|" || token === "∩" || token === "&" || token === "-") {
      while (operators.length && precedence(operators[operators.length - 1]) >= precedence(token)) {
        output.push(operators.pop()!);
      }
      operators.push(token);
      continue;
    }
    output.push(token);
  }
  while (operators.length) {
    const operator = operators.pop()!;
    if (operator === "(") throw new Error(t("native.enumerator.parentheses"));
    output.push(operator);
  }
  return output;
}

function applyOperator(left: Set<string>, right: Set<string>, operator: string): Set<string> {
  if (operator === "∪" || operator === "|") return new Set([...left, ...right]);
  if (operator === "∩" || operator === "&") return new Set([...left].filter((value) => right.has(value)));
  if (operator === "-") return new Set([...left].filter((value) => !right.has(value)));
  return new Set(left);
}

/**
 * Evaluates an enumerator collection expression, including parenthesized name wildcards.
 *
 * @param expression Collection names, operators, and wildcard expressions to evaluate.
 * @param collections Collections available to the expression.
 * @returns The resolved values, referenced collection IDs, and any parse error.
 */
export function evaluateEnumeratorExpression(expression: string, collections: EnumeratorCollection[]): { values: string[]; dependencies: string[]; error: string } {
  const trimmed = String(expression || "").trim();
  if (!trimmed) return { values: [], dependencies: [], error: "" };
  try {
    const tokens = tokenizeExpression(trimmed);
    validateWildcardParentheses(tokens);
    const rpn = toRpn(tokens);
    const stack: Set<string>[] = [];
    const dependencies: string[] = [];

    for (const token of rpn) {
      if (token === "∪" || token === "|" || token === "∩" || token === "&" || token === "-") {
        const right = stack.pop();
        const left = stack.pop();
        if (!left || !right) throw new Error(t("native.enumerator.incomplete"));
        stack.push(applyOperator(left, right, token));
        continue;
      }
      if (isWildcardCollectionToken(token)) {
        const matchedCollections = resolveWildcardCollections(token, collections);
        if (!matchedCollections.length) throw new Error(t("native.enumerator.noMatch", { token }));
        dependencies.push(...matchedCollections.map((collection) => collection.id));
        stack.push(new Set(matchedCollections.flatMap((collection) => collection.values)));
        continue;
      }
      const collection = resolveCollectionToken(token, collections);
      if (!collection) throw new Error(t("native.enumerator.notFound", { token }));
      dependencies.push(collection.id);
      stack.push(new Set(collection.values));
    }

    if (stack.length !== 1) throw new Error(t("native.enumerator.incomplete"));
    return {
      values: [...stack[0]],
      dependencies: uniq(dependencies),
      error: "",
    };
  } catch (error: any) {
    return {
      values: [],
      dependencies: [],
      error: String(error?.message || error || t("native.enumerator.parseFailure")),
    };
  }
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
