import { readActiveLanguageResource, readFallbackLanguageResource } from "./gameResources";

let langMap: Record<string, string> = {};

function parseLanguageMap(raw: string): Record<string, string> {
  const parsed = JSON.parse(raw) as unknown;
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
  const next: Record<string, string> = {};
  for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
    if (typeof value === "string") next[key] = value;
  }
  return next;
}

/**
 * Loads the active Minecraft language map into the in-memory translation cache.
 */
export async function initI18n(force = false): Promise<void> {
  if (!force && Object.keys(langMap).length > 0) return;
  let fallbackMap: Record<string, string> = {};
  try {
    fallbackMap = parseLanguageMap(await readFallbackLanguageResource());
  } catch (error) {
    console.warn("Failed to load fallback language resource", error);
  }
  let activeMap: Record<string, string> = {};
  try {
    activeMap = parseLanguageMap(await readActiveLanguageResource());
  } catch (error) {
    console.warn("Failed to load active language resource", error);
  }
  langMap = { ...fallbackMap, ...activeMap };
}

/**
 * Clears the language cache so the next translation load re-resolves active resources.
 */
export function invalidateI18nCache(): void {
  langMap = {};
}

/**
 * Translates a Minecraft block or item id with the active language map.
 */
export function translateBlockId(fullId: string): string {
  const displayId = fullId.replace("minecraft:", "");
  return langMap[`block.minecraft.${displayId}`] || langMap[`item.minecraft.${displayId}`] || displayId;
}

/**
 * Translates the project's coarse building type label.
 */
export function translateBuildingType(typeStr: string): string {
  switch (typeStr) {
    case "building": return "建筑";
    case "pixel_art": return "像素画";
    case "redstone": return "红石机器";
    case "mixed": return "混合结构";
    case "unknown": return "未知";
    default: return typeStr;
  }
}