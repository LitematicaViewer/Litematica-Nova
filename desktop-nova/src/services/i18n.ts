import { readActiveLanguageResource } from "./gameResources";

let langMap: Record<string, string> = {};

/**
 * Loads the active Minecraft language map into the in-memory translation cache.
 */
export async function initI18n(force = false): Promise<void> {
  if (!force && Object.keys(langMap).length > 0) return;
  try {
    const raw = await readActiveLanguageResource();
    langMap = JSON.parse(raw);
  } catch (e) {
    console.warn("Failed to load active language resource", e);
  }
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
