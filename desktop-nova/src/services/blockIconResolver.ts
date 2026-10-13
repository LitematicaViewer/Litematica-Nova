import { readImageBase64 } from "../platform/files";
import {
  getActiveIconSearchRoots,
  getActiveResourceFingerprint,
  readIconFromRoot,
  type BlockIconSlot,
  type IconSearchRoot,
} from "./gameResources";

const iconCache = new Map<string, Promise<string | null>>();

// Only registry aliases are accepted. A slab is never represented by planks.
import catalogData from "../../../pack-in/materials/minecraft-26.3.json";
import iconManifest from "../../../pack-in/materials/item-icons-26.3.json";
const verifiedIcons = iconManifest.entries as Record<string, { path: string }>;
const catalog = catalogData as { items: Record<string, unknown>; blocks: Record<string, { item_id: string }> };

/** Bundled icon folder, used as the last resort when the registry has no entry. */
const BUNDLED_ICON_ROOT = "pack-in/item-icons-26.3";

export function iconCandidates(blockId: string): string[] {
  const id = blockId.includes(":") ? blockId : `minecraft:${blockId}`;
  if (!id.startsWith("minecraft:")) return [];
  const item = catalog.items[id] ? id : catalog.blocks[id]?.item_id;
  return item && verifiedIcons[item] ? [item.slice("minecraft:".length)] : [];
}

/**
 * Reads one icon from the bundled pack-in folder.
 *
 * This is the pre-registry behaviour and stays as the fallback so icons keep
 * rendering even when the registry holds no usable entry.
 */
async function loadBundledIcon(candidate: string): Promise<string | null> {
  try {
    const dataUrl = await readImageBase64(`${BUNDLED_ICON_ROOT}/${candidate}.png`);
    return dataUrl || null;
  } catch {
    return null;
  }
}

/**
 * Resolves an icon through the game-resource registry first, then the bundled pack.
 *
 * The registry returns the search roots of the *activated* icon resource for the
 * requested slot (material list vs layering), which is what makes multi-version
 * icon packs take effect. `invalidateBlockIconCache()` must be called after the
 * active resource changes.
 */
async function loadIcon(blockId: string, slot: BlockIconSlot = "material_list"): Promise<string | null> {
  if (!blockId) return null;
  const candidates = iconCandidates(blockId);
  if (!candidates.length) return null;

  let roots: IconSearchRoot[] = [];
  try {
    roots = await getActiveIconSearchRoots(slot);
  } catch {
    roots = [];
  }

  for (const root of roots) {
    for (const candidate of candidates) {
      const relativePath = candidate.endsWith(".png") ? candidate : `${candidate}.png`;
      try {
        const dataUrl = await readIconFromRoot(root, relativePath);
        if (dataUrl) return dataUrl;
      } catch {
        // Try the next candidate or root.
      }
    }
  }

  for (const candidate of candidates) {
    const dataUrl = await loadBundledIcon(candidate);
    if (dataUrl) return dataUrl;
  }
  return null;
}

function cacheKey(blockId: string, slot: BlockIconSlot): string {
  return `${slot}:${blockId}`;
}

/**
 * Loads and caches the data URL for a Minecraft block or item icon.
 */
export function getBlockIconDataUrl(
  blockId: string,
  slot: BlockIconSlot = "material_list",
): Promise<string | null> {
  const key = cacheKey(blockId, slot);
  if (!iconCache.has(key)) {
    iconCache.set(key, loadIcon(blockId, slot));
  }
  return iconCache.get(key)!;
}

/** Drops all cached icons so the next lookup re-reads the active resource. */
export function invalidateBlockIconCache(): void {
  iconCache.clear();
}

/**
 * Fingerprint of the activated icon resources.
 *
 * Callers can compare it to a previously captured value to decide whether the
 * cache needs dropping after the user activates another icon pack.
 */
export async function getActiveIconFingerprint(): Promise<string> {
  try {
    return await getActiveResourceFingerprint();
  } catch {
    return "";
  }
}
