import { readFallbackLanguageResource, readActiveLanguageResource } from "./gameResources";

let langMap: Record<string, string> = {};

/**
 * Loads the active language resource from the game-resource registry.
 *
 * The registry resolves the activated `language` entry (builtin by default, or a
 * locally imported / downloaded pack), so switching language packs takes effect
 * after `invalidateI18nCache()` + `initI18n()`.
 */
export async function initI18n() {
  try {
    const raw = await readActiveLanguageResource();
    langMap = JSON.parse(raw);
    return;
  } catch (e) {
    console.warn("Failed to load the active language resource", e);
  }
  // Fall back to the bundled language pack so the UI still renders text.
  try {
    const raw = await readFallbackLanguageResource();
    langMap = JSON.parse(raw);
  } catch (fallbackError) {
    console.warn("Failed to load the fallback language resource", fallbackError);
  }
}

/** Drops the cached language map so the next `initI18n()` re-reads the active pack. */
export function invalidateI18nCache(): void {
  langMap = {};
}

export function translateBlockId(fullId: string): string {
  const displayId = fullId.replace("minecraft:", "");
  return langMap[`block.minecraft.${displayId}`] || langMap[`item.minecraft.${displayId}`] || displayId;
}

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
