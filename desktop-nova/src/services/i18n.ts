import { readLanguageResource } from "./gameResources";
import { emitEvent, listenEvent } from "../platform/events";
import { getUserConfig, saveUserConfig, type UserConfigInfo } from "../platform/appData";
import { getDatabaseBlockName } from "./blockstateDb";
import * as app from "./locales/app";
import * as editor from "./locales/editor";
import * as library from "./locales/library";
import * as native from "./locales/native";
import * as settings from "./locales/settings";
import * as irisSettings from "./locales/irisSettings";
import * as worldExport from "./locales/worldExport";
import * as layerView from "./locales/layerView";
import * as freeCamera from "./locales/freeCamera";

export type UiLocale = "zh-CN" | "en-US";
export type UiLanguagePreference = UiLocale | "system";
export type UiParams = Record<string, string | number | { key: string; params?: UiParams }>;
export type UiMessage = string | { key: string; params?: UiParams };
const dictionaries: Record<UiLocale, Record<string, string>> = {
  "zh-CN": { ...editor.zhCN, ...library.zhCN, ...native.zhCN, ...settings.zhCN, ...irisSettings.zhCN, ...worldExport.zhCN, ...layerView.zhCN, ...freeCamera.zhCN, ...app.zhCN },
  "en-US": { ...editor.enUS, ...library.enUS, ...native.enUS, ...settings.enUS, ...irisSettings.enUS, ...worldExport.enUS, ...layerView.enUS, ...freeCamera.enUS, ...app.enUS },
};
const storageKey = "nova.ui.language";
const languageSubscribers = new Set<() => void>();
let languageRevision = 0;
function cachedPreference(): UiLanguagePreference {
  try { return normalizeUiLanguage(localStorage.getItem(storageKey)); } catch { return "zh-CN"; }
}
export function normalizeUiLanguage(value: unknown): UiLanguagePreference {
  return value === "en-US" || value === "system" ? value : "zh-CN";
}
export function resolveUiLocale(value: UiLanguagePreference): UiLocale {
  return value === "system" ? (typeof navigator !== "undefined" && (navigator.language || "").toLowerCase().startsWith("zh") ? "zh-CN" : "en-US") : value;
}
let preference = cachedPreference();
let locale = resolveUiLocale(preference);
let languageLoading: Promise<void> | null = null;
let listenersStarted = false;
let saveQueue: Promise<unknown> = Promise.resolve();
let pendingPreference: UiLanguagePreference | undefined;
let resourcePackSelection: string | undefined;
export const getUiLocale = (): UiLocale => locale;
export const getUiLanguagePreference = (): UiLanguagePreference => preference;
export const getI18nRevision = (): number => languageRevision;
const notify = () => { languageRevision++; for (const callback of languageSubscribers) callback(); };
export function subscribeI18nChanges(callback: () => void): () => void {
  languageSubscribers.add(callback);
  return () => { languageSubscribers.delete(callback); };
}
function applyLanguage(value: unknown) {
  const next = normalizeUiLanguage(value), resolved = resolveUiLocale(next);
  const changed = next !== preference || resolved !== locale;
  preference = next; locale = resolved;
  try { localStorage.setItem(storageKey, preference); } catch { /* Native events still synchronize windows. */ }
  if (typeof document !== "undefined") document.documentElement.lang = locale;
  if (changed) { invalidateI18nCache(); notify(); void initI18n(); }
}
export function initializeUiLanguage(loadConfig: () => Promise<UserConfigInfo> = getUserConfig): Promise<void> {
  if (!languageLoading) languageLoading = (async () => {
    if (!listenersStarted) {
      listenersStarted = true;
      await Promise.all([
        listenEvent<UserConfigInfo>("user-config-changed", event => {
          const config = event.payload.config;
          const packs = JSON.stringify(config.v5_compatibility?.resource_packs ?? []);
          if (packs !== resourcePackSelection) {
            resourcePackSelection = packs; invalidateI18nCache(); void initI18n();
          }
          const saved = normalizeUiLanguage(config.ui_language);
          if (pendingPreference !== undefined && saved !== pendingPreference) return;
          pendingPreference = undefined; applyLanguage(saved);
        }).catch(console.warn),
        listenEvent<{ preference: UiLanguagePreference }>("nova-ui-language-changed", event => { pendingPreference = event.payload.preference; applyLanguage(event.payload.preference); }).catch(console.warn),
      ]);
      window.addEventListener("storage", event => { if (event.key === storageKey) applyLanguage(event.newValue); });
      window.addEventListener("languagechange", () => {
        if (preference !== "system") return;
        applyLanguage("system");
        void saveUserConfig({ ui_language: "system", ui_locale: locale }).catch(console.warn);
      });
    }
    try {
      const {config} = await loadConfig();
      resourcePackSelection = JSON.stringify(config.v5_compatibility?.resource_packs ?? []);
      applyLanguage(config.ui_language);
      if (config.ui_language === "system" && config.ui_locale !== locale) await saveUserConfig({ui_language:"system",ui_locale:locale});
    }
    catch (error) { console.warn("UI language configuration:", error); applyLanguage(preference); }
  })();
  return languageLoading;
}
export async function setUiLanguage(value: UiLanguagePreference): Promise<void> {
  const next = normalizeUiLanguage(value);
  pendingPreference = next; applyLanguage(next);
  void emitEvent("nova-ui-language-changed", { preference: next }).catch(console.warn);
  const resolved = locale;
  const saving = saveQueue.catch(() => undefined).then(() => saveUserConfig({ ui_language: next, ui_locale: resolved }));
  saveQueue = saving;
  await saving;
}
export function t(key: string, params: UiParams = {}, depth = 0): string {
  const dict = dictionaries[locale];
  const plural = typeof params.count === "number" ? new Intl.PluralRules(locale).select(params.count) : "";
  const selected = plural && dict[`${key}.${plural}`] ? `${key}.${plural}` : key;
  const template = dict[selected] ?? dictionaries["zh-CN"][selected] ?? dict["common.unavailable"];
  return template.replace(/\{([A-Za-z]\w*)\}/g, (placeholder, name: string) => {
    const value = params[name];
    return value === undefined ? placeholder : typeof value === "number" ? new Intl.NumberFormat(locale).format(value) : typeof value === "object" ? localizeMessage(value, depth + 1) : name === "error" && depth < 3 ? localizeMessage(value, depth + 1) : value;
  });
}
export const message = (key: string, params?: UiParams): Exclude<UiMessage, string> => ({ key, params });
export const formatNumber = (value: number, options?: Intl.NumberFormatOptions) => new Intl.NumberFormat(locale, options).format(value);
export function formatDate(value: Date | string | number, options?: Intl.DateTimeFormatOptions): string {
  const date = value instanceof Date ? value : new Date(value);
  return Number.isNaN(date.getTime()) ? String(value) : new Intl.DateTimeFormat(locale, options).format(date);
}
const messageTemplates = Object.entries(dictionaries["zh-CN"]).flatMap(([key, chinese]) => [chinese, dictionaries["en-US"][key]].filter(Boolean).map(template => {
  const names: string[] = [];
  const pattern = template.split(/(\{[A-Za-z]\w*\})/g).map(piece => {
    if (/^\{[A-Za-z]\w*\}$/.test(piece)) { names.push(piece.slice(1, -1)); return "([\\s\\S]+?)"; }
    return piece.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }).join("");
  return { key, template, names, regex: names.length ? new RegExp(`^${pattern}$`) : null };
}));
const literalKeys = new Map(messageTemplates.filter(row => !row.names.length).map(row => [row.template, row.key]));
/** Only for status/error display, never for user-provided names or business IDs. */
export function localizeMessage(value: unknown, depth = 0): string {
  if (value && typeof value === "object" && "key" in value && typeof value.key === "string") return t(value.key, (value as Exclude<UiMessage, string>).params, depth);
  const raw = value instanceof Error ? value.message : String(value ?? "");
  const plain = raw.replace(/^Error:\s*/, "");
  const key = literalKeys.get(plain); if (key) return t(key, {}, depth);
  for (const row of messageTemplates) {
    const match = row.regex?.exec(plain);
    if (match) return t(row.key, Object.fromEntries(row.names.map((name, index) => [name, match[index + 1]])), depth);
  }
  return raw;
}

let langMap: Record<string, string> = {};
let englishMap: Record<string, string> = {};
let loading: Promise<void> | null = null;
let languageGeneration = 0;
let resourceListenerStarted = false;
/** Snapshot for exports; names only, never part of renderer resource identity. */
export const getMinecraftTranslations = (): Record<string, string> => ({ ...englishMap, ...langMap });
export function invalidateI18nCache(): void { langMap = {}; englishMap = {}; loading = null; languageGeneration++; }
export async function initI18n(force = false) {
  if (!resourceListenerStarted) {
    resourceListenerStarted = true;
    void listenEvent("resource-language-changed", async () => { invalidateI18nCache(); await initI18n(); }).catch(() => { resourceListenerStarted = false; });
  }
  if (force) invalidateI18nCache();
  if (!loading) {
    const generation = languageGeneration;
    const wanted = locale;
    loading = (async () => {
      const englishResource = readLanguageResource("en_us");
      const [selected, english] = await Promise.all([wanted === "zh-CN" ? readLanguageResource("zh_cn") : englishResource, englishResource]);
      if (generation !== languageGeneration) return;
      langMap = JSON.parse(selected); englishMap = JSON.parse(english); notify();
    })().catch(error => { if (generation === languageGeneration) loading = null; console.warn("Minecraft names:", error); });
  }
  await loading;
}
function resourceName(id: string): string {
  const [namespace, path] = id.includes(":") ? id.split(":", 2) : ["minecraft", id];
  const keys = [`block.${namespace}.${path}`, `item.${namespace}.${path}`, `entity.${namespace}.${path}`];
  return keys.map(key => langMap[key]).find(Boolean) || keys.map(key => englishMap[key]).find(Boolean) || "";
}
export function resolveBlockDisplayName(fullId: string, fallback = ""): string {
  return resourceName(fullId) || (locale === "zh-CN" ? getDatabaseBlockName(fullId) || fallback : "") || fullId.replace(/^minecraft:/, "");
}
export function translateBlockId(fullId: string): string { return resolveBlockDisplayName(fullId); }
export const translateEntityId = translateBlockId;
export function translateBuildingType(value: string): string { return dictionaries[locale][`building.${value}`] ? t(`building.${value}`) : value; }
