import { readActiveLanguageResource, readFallbackLanguageResource, listGameResourceRegistry } from "./gameResources";
import { listenEvent } from "../platform/events";
import { getDatabaseBlockName } from "./blockstateDb";

let langMap: Record<string, string> = {};
let loading: Promise<void> | null = null;
let explicitLanguage = false;
let languageGeneration = 0;
let languageRevision = 0;
const languageSubscribers = new Set<() => void>();

let resourceListenerStarted = false;
export function invalidateI18nCache(): void { langMap = {}; explicitLanguage = false; loading = null; languageGeneration++; }
export const getI18nRevision = (): number => languageRevision;
export function subscribeI18nChanges(callback: () => void): () => void {
  languageSubscribers.add(callback);
  void initI18n();
  return () => { languageSubscribers.delete(callback); };
}
export async function initI18n(force = false) {
  if (!resourceListenerStarted) {
    resourceListenerStarted = true;
    void listenEvent("resource-language-changed", async () => { invalidateI18nCache(); await initI18n(); })
      .catch(() => { resourceListenerStarted = false; });
  }
  if (force) invalidateI18nCache();
  if (!loading) {
    const generation = languageGeneration;
    loading = (async () => {
    const snapshot = await listGameResourceRegistry();
    let custom = !snapshot.active_language.builtin;
    const text = await readActiveLanguageResource().catch(() => { custom = false; return readFallbackLanguageResource(); });
    const next = JSON.parse(text);
    if (generation !== languageGeneration) return;
    langMap = next;
    explicitLanguage = custom;
    languageRevision++;
    for (const callback of languageSubscribers) callback();
    })().catch((error) => { if (generation === languageGeneration) loading = null; console.warn("Chinese names:", error); });
  }
  await loading;
}

/** Core names are fallbacks, not overrides for a selected language resource. */
export function resolveBlockDisplayName(fullId: string, fallback = ""): string {
  const displayId = fullId.replace("minecraft:", "");
  const resourceName = langMap[`block.minecraft.${displayId}`] || langMap[`item.minecraft.${displayId}`];
  const databaseName = getDatabaseBlockName(fullId);
  return (explicitLanguage ? resourceName || databaseName : databaseName || resourceName) || fallback || displayId;
}

export function translateBlockId(fullId: string): string {
  const displayId = fullId.replace("minecraft:", "");
  const resourceName = langMap[`block.minecraft.${displayId}`] || langMap[`item.minecraft.${displayId}`];
  const databaseName = getDatabaseBlockName(fullId);
  return (explicitLanguage ? resourceName || databaseName : databaseName || resourceName) || displayId;
}

export function translateBuildingType(typeStr: string): string {
  switch (typeStr) {
    case "building": return "建筑";
    case "too_small": return "小型结构";
    case "redstone_machine": return "红石机器";
    case "redstone_power": return "红石设施";
    case "mechanical_build": return "机械建筑";
    case "structural_mechanism": return "建筑机关";
    case "pixel_art": return "像素画";
    case "redstone": return "红石机器";
    case "mixed": return "混合结构";
    case "unknown": return "未知";
    default: return typeStr;
  }
}
