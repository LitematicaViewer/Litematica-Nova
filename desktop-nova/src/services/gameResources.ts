import { emitEvent } from "../platform/events";
import {
  copyFileToDirectory,
  getPathInfo,
  readImageBase64,
  readWorkspaceFile,
  getUserConfigFilePath,
  readUserConfigFile,
  writeUserConfigFile,
  type PathInfo,
} from "./backend";

export type GameResourceKind = "language" | "block_icon" | "item_icon" | "game_data";
export type BlockIconSlot = "material_list" | "layering";
export type GameResourceSource = "builtin" | "imported" | "external" | "github";
export type RemoteLanguageSource = "github/InventivetalentDev";

export interface GameResourceEntry {
  id: string;
  kind: GameResourceKind;
  label: string;
  source: GameResourceSource;
  installed_at?: string;
  language?: string;
  branch?: string;
  version?: string;
  file_relpath?: string;
  root_relpath?: string;
  root_path?: string;
  data_relpath?: string;
  i18n_relpath?: string;
  active?: boolean;
  active_material_list?: boolean;
  active_layering?: boolean;
  builtin?: boolean;
}

export interface GameResourceIndex {
  schema_version: 1;
  kind: GameResourceKind;
  entries: GameResourceEntry[];
}

export interface IconSearchRoot {
  source: "workspace" | "app_data" | "absolute";
  root: string;
  label: string;
}

export interface GameResourceSnapshot {
  config_dir: string;
  entries: Record<GameResourceKind, GameResourceEntry[]>;
  active_language: GameResourceEntry;
  active_material_list_icons: GameResourceEntry;
  active_layering_block_icons: GameResourceEntry;
  active_layering_item_icons: GameResourceEntry;
  active_game_data: GameResourceEntry;
}

export interface GameResourceHealthRow {
  label: string;
  ok: boolean;
  detail: string;
}

interface GithubBranchRow {
  name?: string;
}

interface GithubContentRow {
  type?: string;
  name?: string;
}

const INDEX_PATHS: Record<GameResourceKind, string> = {
  language: "minecraft-assets/language/installed.json",
  block_icon: "minecraft-assets/block_icon/installed.json",
  item_icon: "minecraft-assets/item/installed.json",
  game_data: "minecraft-assets/game-data/installed.json",
};

const INITIAL_LANGUAGE_RELPATH = "minecraft-assets/language/initial/zh_cn.json";
const BUNDLED_LANGUAGE_RELPATH = "pack-in/lang/zh_cn.json";
const GITHUB_LANGUAGE_SOURCE: RemoteLanguageSource = "github/InventivetalentDev";
const GITHUB_API_BASE = "https://api.github.com/repos/InventivetalentDev/minecraft-assets";
const GITHUB_RAW_BASE = "https://raw.githubusercontent.com/InventivetalentDev/minecraft-assets";
const LANGUAGE_ALLOWED_PREFIXES = [
  "block.",
  "effect.",
  "enchantment.",
  "entity.",
  "item.",
] as const;

const BUILTIN_LANGUAGE: GameResourceEntry = {
  id: "builtin:language:zh_cn",
  kind: "language",
  label: "内建 zh_cn",
  source: "builtin",
  language: "zh_cn",
  file_relpath: INITIAL_LANGUAGE_RELPATH,
  active: true,
  builtin: true,
};

const BUILTIN_BLOCK_ICON: GameResourceEntry = {
  id: "builtin:block_icon:nova",
  kind: "block_icon",
  label: "内建 Nova 方块图标",
  source: "builtin",
  root_relpath: "block",
  active_material_list: true,
  active_layering: true,
  builtin: true,
};

const BUILTIN_ITEM_ICON: GameResourceEntry = {
  id: "builtin:item_icon:nova",
  kind: "item_icon",
  label: "内建 Nova 物品图标",
  source: "builtin",
  root_relpath: "item",
  active_layering: true,
  builtin: true,
};

const BUILTIN_GAME_DATA: GameResourceEntry = {
  id: "builtin:game_data:26.1",
  kind: "game_data",
  label: "内建 BlockState 26.1",
  source: "builtin",
  version: "26.1",
  data_relpath: "data/minecraft_blockstates/26.1.json",
  i18n_relpath: "data/minecraft_blockstates/26.1.zh_cn.json",
  active: true,
  builtin: true,
};

function nowId(): string {
  return new Date().toISOString().replace(/[-:.TZ]/g, "");
}

function baseName(path: string): string {
  return path.split(/[\\/]+/).filter(Boolean).pop() || "resource.dat";
}

function stripExtension(name: string): string {
  return name.replace(/\.[^.]+$/, "");
}

function normalizeResourceIdPart(value: string): string {
  const normalized = value.trim().toLowerCase().replace(/[^a-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "");
  return normalized || "resource";
}

function folderFromFilePath(path: string): string {
  const index = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return index >= 0 ? path.slice(0, index) : path;
}

function joinPath(root: string, relative: string): string {
  const separator = root.includes("\\") ? "\\" : "/";
  return `${root.replace(/[\\/]+$/, "")}${separator}${relative.replace(/^[\\/]+/, "").replace(/\//g, separator)}`;
}

function isUserConfigRelativePath(path: string): boolean {
  return /^(minecraft-assets|legacy)\//.test(path);
}

function isAllowedLanguageBranch(value: string): boolean {
  const branch = value.trim();
  if (!branch) return false;
  return /^1\.\d+(?:\.\d+)?$/.test(branch) || /^\d{2}\.\d+(?:\.\d+)?$/.test(branch);
}

function languageBranchSortKey(value: string): number[] {
  return value
    .split(".")
    .map((part) => Number.parseInt(part, 10))
    .map((part) => (Number.isFinite(part) ? part : -1));
}

function compareBranchesDesc(left: string, right: string): number {
  const leftKey = languageBranchSortKey(left);
  const rightKey = languageBranchSortKey(right);
  const size = Math.max(leftKey.length, rightKey.length, 3);
  for (let index = 0; index < size; index += 1) {
    const diff = (rightKey[index] ?? -1) - (leftKey[index] ?? -1);
    if (diff !== 0) return diff;
  }
  return right.localeCompare(left);
}

function isAllowedLanguageCode(value: string): boolean {
  return /^[a-z]{2,3}(?:_[a-z0-9]{2,8}){0,2}$/.test(value.trim().toLowerCase());
}

function filterLanguageMapObject(payload: Record<string, unknown>): Record<string, string> {
  const next: Record<string, string> = {};
  for (const [key, value] of Object.entries(payload)) {
    if (typeof value !== "string") continue;
    if (!LANGUAGE_ALLOWED_PREFIXES.some((prefix) => key.startsWith(prefix))) continue;
    next[key] = value;
  }
  return next;
}

function normalizeLanguageJsonText(rawText: string): string {
  const parsed = JSON.parse(rawText) as unknown;
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("language payload must be a JSON object");
  }
  const filtered = filterLanguageMapObject(parsed as Record<string, unknown>);
  if (Object.keys(filtered).length === 0) {
    throw new Error("language payload does not contain supported Minecraft translation keys");
  }
  return JSON.stringify(filtered, null, 2);
}

function parseLanguageListPayload(payload: unknown): string[] {
  const languages = new Set<string>();
  const push = (candidate: unknown) => {
    if (typeof candidate !== "string") return;
    const normalized = candidate.trim().toLowerCase();
    if (isAllowedLanguageCode(normalized)) languages.add(normalized);
  };
  if (Array.isArray(payload)) {
    for (const item of payload) {
      if (typeof item === "string") {
        push(item);
        continue;
      }
      if (!item || typeof item !== "object") continue;
      const row = item as Record<string, unknown>;
      push(row.code);
      push(row.lang);
      push(row.id);
      push(row.name);
    }
  } else if (payload && typeof payload === "object") {
    for (const [key, value] of Object.entries(payload as Record<string, unknown>)) {
      push(key);
      push(value);
      if (!value || typeof value !== "object") continue;
      const row = value as Record<string, unknown>;
      push(row.code);
      push(row.lang);
      push(row.id);
      push(row.name);
    }
  }
  return Array.from(languages).sort((left, right) => left.localeCompare(right));
}

function parseBranchNamesFromHtml(html: string): string[] {
  const matches = html.matchAll(/\/tree\/([^"/?#]+)"/g);
  const branches = new Set<string>();
  for (const match of matches) {
    const branch = decodeURIComponent(match[1] || "").trim();
    if (isAllowedLanguageBranch(branch)) branches.add(branch);
  }
  return Array.from(branches).sort(compareBranchesDesc);
}

function parseLanguageCodesFromHtml(html: string): string[] {
  const matches = html.matchAll(/([a-z0-9_]+)\.json/gi);
  const languages = new Set<string>();
  for (const match of matches) {
    const language = (match[1] || "").trim().toLowerCase();
    if (isAllowedLanguageCode(language)) languages.add(language);
  }
  return Array.from(languages).sort((left, right) => left.localeCompare(right));
}

async function fetchJson<T>(url: string): Promise<T> {
  const response = await fetch(url, {
    headers: {
      Accept: "application/json",
    },
  });
  if (!response.ok) {
    throw new Error(`request failed (${response.status}) ${url}`);
  }
  return await response.json() as T;
}

async function fetchText(url: string): Promise<string> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`request failed (${response.status}) ${url}`);
  }
  return await response.text();
}

async function appDataDirFor(relativeDir: string): Promise<string> {
  const marker = await getUserConfigFilePath(`${relativeDir}/.resource-dir`);
  return folderFromFilePath(marker);
}

async function ensureInitialLanguageSeeded(): Promise<void> {
  try {
    await readUserConfigFile(INITIAL_LANGUAGE_RELPATH);
    return;
  } catch {
    // fall through and seed from bundled resource
  }
  const bundled = await readWorkspaceFile(BUNDLED_LANGUAGE_RELPATH);
  await writeUserConfigFile(INITIAL_LANGUAGE_RELPATH, normalizeLanguageJsonText(bundled));
}

async function readIndex(kind: GameResourceKind): Promise<GameResourceIndex> {
  try {
    const raw = await readUserConfigFile(INDEX_PATHS[kind]);
    const parsed = JSON.parse(raw) as Partial<GameResourceIndex>;
    const entries = Array.isArray(parsed.entries) ? parsed.entries : [];
    return {
      schema_version: 1,
      kind,
      entries: entries
        .filter((entry): entry is GameResourceEntry => !!entry && typeof entry.id === "string")
        .map((entry) => ({ ...entry, kind })),
    };
  } catch {
    return { schema_version: 1, kind, entries: [] };
  }
}

async function writeIndex(kind: GameResourceKind, entries: GameResourceEntry[]): Promise<void> {
  const index: GameResourceIndex = {
    schema_version: 1,
    kind,
    entries: entries.map((entry) => ({ ...entry, kind, builtin: false })),
  };
  await writeUserConfigFile(INDEX_PATHS[kind], JSON.stringify(index, null, 2));
}

function builtinEntry(kind: GameResourceKind): GameResourceEntry {
  switch (kind) {
    case "language": return { ...BUILTIN_LANGUAGE };
    case "block_icon": return { ...BUILTIN_BLOCK_ICON };
    case "item_icon": return { ...BUILTIN_ITEM_ICON };
    case "game_data": return { ...BUILTIN_GAME_DATA };
  }
}

function withBuiltinActivation(kind: GameResourceKind, entries: GameResourceEntry[]): GameResourceEntry[] {
  const builtin = builtinEntry(kind);
  if (kind === "language" || kind === "game_data") {
    builtin.active = !entries.some((entry) => entry.active);
  } else if (kind === "block_icon") {
    builtin.active_material_list = !entries.some((entry) => entry.active_material_list);
    builtin.active_layering = !entries.some((entry) => entry.active_layering);
  } else if (kind === "item_icon") {
    builtin.active_layering = !entries.some((entry) => entry.active_layering);
  }
  return [builtin, ...entries];
}

function activeEntry(kind: GameResourceKind, entries: GameResourceEntry[], slot?: BlockIconSlot): GameResourceEntry {
  if (kind === "block_icon") {
    const key = slot === "layering" ? "active_layering" : "active_material_list";
    return entries.find((entry) => !!entry[key]) || BUILTIN_BLOCK_ICON;
  }
  if (kind === "item_icon") {
    return entries.find((entry) => entry.active_layering) || BUILTIN_ITEM_ICON;
  }
  return entries.find((entry) => entry.active) || builtinEntry(kind);
}

async function emitResourceChange(kind: GameResourceKind): Promise<void> {
  await emitEvent("game-resources-changed", { kind }).catch(() => undefined);
  if (kind === "language") await emitEvent("resource-language-changed", {}).catch(() => undefined);
  if (kind === "block_icon" || kind === "item_icon") await emitEvent("resource-block-icons-changed", {}).catch(() => undefined);
  if (kind === "game_data") await emitEvent("resource-game-data-changed", {}).catch(() => undefined);
}

async function pathInfoForRoot(root: IconSearchRoot): Promise<PathInfo> {
  if (root.source === "workspace" || root.source === "absolute") {
    return getPathInfo(root.root);
  }
  const absolute = await appDataDirFor(root.root);
  return getPathInfo(absolute);
}

async function readEntryFile(entry: GameResourceEntry, relpath: string | undefined): Promise<string> {
  if (!relpath) throw new Error(`resource ${entry.id} does not define a file path`);
  if (entry.source === "builtin") {
    return isUserConfigRelativePath(relpath) ? readUserConfigFile(relpath) : readWorkspaceFile(relpath);
  }
  return readUserConfigFile(relpath);
}

export async function readFallbackLanguageResource(): Promise<string> {
  await ensureInitialLanguageSeeded();
  return readUserConfigFile(INITIAL_LANGUAGE_RELPATH);
}

export async function listRemoteMinecraftLanguageBranches(source: RemoteLanguageSource = GITHUB_LANGUAGE_SOURCE): Promise<string[]> {
  if (source !== GITHUB_LANGUAGE_SOURCE) throw new Error(`unsupported language source: ${source}`);
  const branches = new Set<string>();
  try {
    let page = 1;
    while (true) {
      const rows = await fetchJson<GithubBranchRow[]>(`${GITHUB_API_BASE}/branches?per_page=100&page=${page}`);
      if (!Array.isArray(rows) || rows.length === 0) break;
      for (const row of rows) {
        const branch = typeof row?.name === "string" ? row.name.trim() : "";
        if (isAllowedLanguageBranch(branch)) branches.add(branch);
      }
      if (rows.length < 100) break;
      page += 1;
    }
  } catch {
    // fall through to HTML fallback
  }
  if (branches.size === 0) {
    try {
      const html = await fetchText("https://github.com/InventivetalentDev/minecraft-assets/branches");
      for (const branch of parseBranchNamesFromHtml(html)) branches.add(branch);
    } catch {
      // handled below
    }
  }
  const output = Array.from(branches).sort(compareBranchesDesc);
  if (output.length === 0) {
    throw new Error("无法获取 GitHub 语言版本分支列表。");
  }
  return output;
}

export async function listRemoteMinecraftLanguages(branch: string, source: RemoteLanguageSource = GITHUB_LANGUAGE_SOURCE): Promise<string[]> {
  if (source !== GITHUB_LANGUAGE_SOURCE) throw new Error(`unsupported language source: ${source}`);
  const cleanBranch = branch.trim();
  if (!isAllowedLanguageBranch(cleanBranch)) {
    throw new Error(`invalid language branch: ${branch}`);
  }
  try {
    const listPayload = await fetchJson<unknown>(`${GITHUB_RAW_BASE}/${cleanBranch}/assets/minecraft/lang/_list.json`);
    const parsed = parseLanguageListPayload(listPayload);
    if (parsed.length > 0) return parsed;
  } catch {
    // fall through
  }
  try {
    const rows = await fetchJson<GithubContentRow[]>(`${GITHUB_API_BASE}/contents/assets/minecraft/lang?ref=${encodeURIComponent(cleanBranch)}`);
    const languages = rows
      .map((row) => (row.type === "file" && typeof row.name === "string" ? row.name : ""))
      .filter((name) => name.toLowerCase().endsWith(".json"))
      .map((name) => name.slice(0, -5).toLowerCase())
      .filter((name, index, array) => isAllowedLanguageCode(name) && array.indexOf(name) === index)
      .sort((left, right) => left.localeCompare(right));
    if (languages.length > 0) return languages;
  } catch {
    // fall through
  }
  try {
    const html = await fetchText(`https://github.com/InventivetalentDev/minecraft-assets/tree/${encodeURIComponent(cleanBranch)}/assets/minecraft/lang`);
    const languages = parseLanguageCodesFromHtml(html);
    if (languages.length > 0) return languages;
  } catch {
    // handled below
  }
  throw new Error(`无法获取 ${cleanBranch} 的语言列表。`);
}

export async function downloadRemoteMinecraftLanguage(branch: string, language: string, source: RemoteLanguageSource = GITHUB_LANGUAGE_SOURCE): Promise<GameResourceSnapshot> {
  if (source !== GITHUB_LANGUAGE_SOURCE) throw new Error(`unsupported language source: ${source}`);
  const cleanBranch = branch.trim();
  const cleanLanguage = language.trim().toLowerCase();
  if (!isAllowedLanguageBranch(cleanBranch)) {
    throw new Error(`invalid language branch: ${branch}`);
  }
  if (!isAllowedLanguageCode(cleanLanguage)) {
    throw new Error(`invalid language code: ${language}`);
  }
  const rawText = await fetchText(`${GITHUB_RAW_BASE}/${cleanBranch}/assets/minecraft/lang/${cleanLanguage}.json`);
  const normalized = normalizeLanguageJsonText(rawText);
  const fileRelpath = `minecraft-assets/language/github/InventivetalentDev/${cleanBranch}/${cleanLanguage}.json`;
  await writeUserConfigFile(fileRelpath, normalized);
  const entry: GameResourceEntry = {
    id: `github:language:${normalizeResourceIdPart(cleanBranch)}:${cleanLanguage}`,
    kind: "language",
    label: `GitHub ${cleanBranch} / ${cleanLanguage}`,
    source: "github",
    branch: cleanBranch,
    language: cleanLanguage,
    file_relpath: fileRelpath,
    active: true,
    installed_at: new Date().toISOString(),
  };
  await registerGameResource(entry);
  return activateGameResource("language", entry.id);
}

/**
 * Lists installed game resources and appends the immutable built-in fallback entry for every kind.
 */
export async function listGameResourceRegistry(): Promise<GameResourceSnapshot> {
  await ensureInitialLanguageSeeded();
  const [configPath, language, blockIcon, itemIcon, gameData] = await Promise.all([
    getUserConfigFilePath(".resource-root"),
    readIndex("language"),
    readIndex("block_icon"),
    readIndex("item_icon"),
    readIndex("game_data"),
  ]);
  const config_dir = folderFromFilePath(configPath);
  const entries = {
    language: withBuiltinActivation("language", language.entries),
    block_icon: withBuiltinActivation("block_icon", blockIcon.entries),
    item_icon: withBuiltinActivation("item_icon", itemIcon.entries),
    game_data: withBuiltinActivation("game_data", gameData.entries),
  };
  return {
    config_dir,
    entries,
    active_language: activeEntry("language", entries.language),
    active_material_list_icons: activeEntry("block_icon", entries.block_icon, "material_list"),
    active_layering_block_icons: activeEntry("block_icon", entries.block_icon, "layering"),
    active_layering_item_icons: activeEntry("item_icon", entries.item_icon),
    active_game_data: activeEntry("game_data", entries.game_data),
  };
}

/**
 * Registers or replaces an installed game resource entry.
 */
export async function registerGameResource(entry: GameResourceEntry): Promise<GameResourceSnapshot> {
  const index = await readIndex(entry.kind);
  const cleanEntry: GameResourceEntry = {
    ...entry,
    id: entry.id || `${entry.source}:${entry.kind}:${nowId()}`,
    label: entry.label || entry.id,
    installed_at: entry.installed_at || new Date().toISOString(),
    builtin: false,
  };
  const next = index.entries.filter((item) => item.id !== cleanEntry.id);
  next.push(cleanEntry);
  await writeIndex(entry.kind, next);
  await emitResourceChange(entry.kind);
  return listGameResourceRegistry();
}

/**
 * Activates one resource in its global or slot-specific active position.
 */
export async function activateGameResource(kind: GameResourceKind, id: string, slot?: BlockIconSlot): Promise<GameResourceSnapshot> {
  const index = await readIndex(kind);
  const next = index.entries.map((entry) => {
    if (kind === "language" || kind === "game_data") {
      return { ...entry, active: entry.id === id };
    }
    if (kind === "block_icon") {
      if (slot === "layering") return { ...entry, active_layering: entry.id === id };
      return { ...entry, active_material_list: entry.id === id };
    }
    return { ...entry, active_layering: entry.id === id };
  });
  const selectingBuiltin = id.startsWith("builtin:");
  await writeIndex(kind, selectingBuiltin ? next.map((entry) => {
    if (kind === "language" || kind === "game_data") return { ...entry, active: false };
    if (kind === "block_icon") {
      return slot === "layering" ? { ...entry, active_layering: false } : { ...entry, active_material_list: false };
    }
    return { ...entry, active_layering: false };
  }) : next);
  await emitResourceChange(kind);
  return listGameResourceRegistry();
}

/**
 * Removes a resource from the installed index. Built-in fallback resources are immutable.
 */
export async function deleteGameResource(kind: GameResourceKind, id: string): Promise<GameResourceSnapshot> {
  if (id.startsWith("builtin:")) return listGameResourceRegistry();
  const index = await readIndex(kind);
  await writeIndex(kind, index.entries.filter((entry) => entry.id !== id));
  await emitResourceChange(kind);
  return listGameResourceRegistry();
}

/**
 * Imports a Minecraft language JSON file into the user resource directory and activates it.
 */
export async function importLanguageResource(sourcePath: string, language?: string): Promise<GameResourceSnapshot> {
  const sourceName = baseName(sourcePath);
  const lang = normalizeResourceIdPart(language || stripExtension(sourceName));
  const folder = `minecraft-assets/language/imported/${lang}-${nowId()}`;
  const targetDir = await appDataDirFor(folder);
  const copied = await copyFileToDirectory(sourcePath, targetDir, sourceName, true);
  const entry: GameResourceEntry = {
    id: `imported:language:${lang}:${nowId()}`,
    kind: "language",
    label: `导入语言 ${lang}`,
    source: "imported",
    language: lang,
    file_relpath: `${folder}/${baseName(copied.target_path)}`,
    active: true,
  };
  await registerGameResource(entry);
  return activateGameResource("language", entry.id);
}

/**
 * Registers an external block or item icon directory for the requested slot.
 */
export async function registerExternalIconDirectory(kind: "block_icon" | "item_icon", rootPath: string, slot: BlockIconSlot): Promise<GameResourceSnapshot> {
  const label = `${kind === "block_icon" ? "外部方块图标" : "外部物品图标"} ${baseName(rootPath)}`;
  const entry: GameResourceEntry = {
    id: `external:${kind}:${normalizeResourceIdPart(baseName(rootPath))}:${nowId()}`,
    kind,
    label,
    source: "external",
    root_path: rootPath,
    active_material_list: kind === "block_icon" && slot === "material_list",
    active_layering: slot === "layering",
  };
  await registerGameResource(entry);
  if (kind === "block_icon") return activateGameResource(kind, entry.id, slot);
  return activateGameResource(kind, entry.id, "layering");
}

/**
 * Imports BlockState database JSON files into the user resource directory and activates them.
 */
export async function importGameDataResource(dataPath: string, i18nPath?: string, version?: string): Promise<GameResourceSnapshot> {
  const dataName = baseName(dataPath);
  const dataVersion = normalizeResourceIdPart(version || stripExtension(dataName).replace(/\.zh_cn$/i, ""));
  const folder = `minecraft-assets/game-data/imported/${dataVersion}-${nowId()}`;
  const targetDir = await appDataDirFor(folder);
  const copiedData = await copyFileToDirectory(dataPath, targetDir, dataName, true);
  let i18nRelpath = "";
  if (i18nPath) {
    const copiedI18n = await copyFileToDirectory(i18nPath, targetDir, baseName(i18nPath), true);
    i18nRelpath = `${folder}/${baseName(copiedI18n.target_path)}`;
  }
  const entry: GameResourceEntry = {
    id: `imported:game_data:${dataVersion}:${nowId()}`,
    kind: "game_data",
    label: `导入 BlockState ${dataVersion}`,
    source: "imported",
    version: dataVersion,
    data_relpath: `${folder}/${baseName(copiedData.target_path)}`,
    i18n_relpath: i18nRelpath,
    active: true,
  };
  await registerGameResource(entry);
  return activateGameResource("game_data", entry.id);
}

/**
 * Reads the currently active language JSON text, falling back to the bundled zh_cn map.
 */
export async function readActiveLanguageResource(): Promise<string> {
  await ensureInitialLanguageSeeded();
  const snapshot = await listGameResourceRegistry();
  return readEntryFile(snapshot.active_language, snapshot.active_language.file_relpath);
}

/**
 * Reads the currently active BlockState database JSON texts.
 */
export async function readActiveGameDataResource(): Promise<{ dbText: string; i18nText: string; entry: GameResourceEntry }> {
  const snapshot = await listGameResourceRegistry();
  const entry = snapshot.active_game_data;
  const dbText = await readEntryFile(entry, entry.data_relpath || BUILTIN_GAME_DATA.data_relpath);
  let i18nText = "{}";
  if (entry.i18n_relpath) {
    i18nText = await readEntryFile(entry, entry.i18n_relpath);
  } else {
    i18nText = await readWorkspaceFile(BUILTIN_GAME_DATA.i18n_relpath!);
  }
  return { dbText, i18nText, entry };
}

/**
 * Resolves icon search roots for the requested consumer slot.
 */
export async function getActiveIconSearchRoots(slot: BlockIconSlot = "material_list"): Promise<IconSearchRoot[]> {
  const snapshot = await listGameResourceRegistry();
  const roots: IconSearchRoot[] = [];
  const blockEntry = slot === "layering" ? snapshot.active_layering_block_icons : snapshot.active_material_list_icons;
  if (blockEntry.source === "external" && blockEntry.root_path) {
    roots.push({ source: "absolute", root: blockEntry.root_path, label: blockEntry.label });
  } else if (!blockEntry.builtin && blockEntry.root_relpath) {
    roots.push({ source: "app_data", root: blockEntry.root_relpath, label: blockEntry.label });
  }
  if (slot === "layering") {
    const itemEntry = snapshot.active_layering_item_icons;
    if (itemEntry.source === "external" && itemEntry.root_path) {
      roots.push({ source: "absolute", root: itemEntry.root_path, label: itemEntry.label });
    } else if (!itemEntry.builtin && itemEntry.root_relpath) {
      roots.push({ source: "app_data", root: itemEntry.root_relpath, label: itemEntry.label });
    }
  }
  roots.push(
    { source: "workspace", root: "block", label: "内建 block/" },
    { source: "workspace", root: "item", label: "内建 item/" },
    { source: "workspace", root: "pack-in", label: "内建 pack-in/" },
  );
  return roots;
}

/**
 * Reads an icon from a resolved icon root.
 */
export async function readIconFromRoot(root: IconSearchRoot, relativePath: string): Promise<string> {
  if (root.source === "workspace") return readImageBase64(`${root.root}/${relativePath}`);
  if (root.source === "absolute") return readImageBase64(joinPath(root.root, relativePath));
  const absoluteRoot = await appDataDirFor(root.root);
  return readImageBase64(joinPath(absoluteRoot, relativePath));
}

/**
 * Builds a compact fingerprint of current active resource choices for cache keys.
 */
export async function getActiveResourceFingerprint(): Promise<string> {
  const snapshot = await listGameResourceRegistry();
  return [
    snapshot.active_language.id,
    snapshot.active_material_list_icons.id,
    snapshot.active_layering_block_icons.id,
    snapshot.active_layering_item_icons.id,
    snapshot.active_game_data.id,
  ].join("|");
}

/**
 * Performs a lightweight health check against currently active game resources.
 */
export async function checkGameResourceHealth(): Promise<GameResourceHealthRow[]> {
  const snapshot = await listGameResourceRegistry();
  const rows: GameResourceHealthRow[] = [];
  try {
    JSON.parse(await readActiveLanguageResource());
    rows.push({
      label: `语言：${snapshot.active_language.label}`,
      ok: true,
      detail: snapshot.active_language.file_relpath || snapshot.active_language.root_path || "",
    });
  } catch (error) {
    rows.push({ label: `语言：${snapshot.active_language.label}`, ok: false, detail: String(error) });
  }
  try {
    const active = await readActiveGameDataResource();
    JSON.parse(active.dbText);
    JSON.parse(active.i18nText);
    rows.push({ label: `BlockState：${snapshot.active_game_data.label}`, ok: true, detail: active.entry.data_relpath || "" });
  } catch (error) {
    rows.push({ label: `BlockState：${snapshot.active_game_data.label}`, ok: false, detail: String(error) });
  }
  const roots = await getActiveIconSearchRoots("material_list");
  for (const root of roots.slice(0, 1)) {
    try {
      const info = await pathInfoForRoot(root);
      rows.push({ label: `材料列表图标：${root.label}`, ok: info.exists && info.is_dir, detail: info.normalized });
    } catch (error) {
      rows.push({ label: `材料列表图标：${root.label}`, ok: false, detail: String(error) });
    }
  }
  return rows;
}