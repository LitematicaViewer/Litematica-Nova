import { getActiveIconSearchRoots, readIconFromRoot, type BlockIconSlot } from "./gameResources";

const iconCache = new Map<string, Promise<string | null>>();

function iconCandidates(blockId: string): string[] {
  const displayId = blockId.replace("minecraft:", "");
  const candidates = [displayId];
  if (displayId.endsWith("_slab")) {
    candidates.push(displayId.replace("_slab", "_planks"));
  }
  if (displayId.startsWith("potted_")) {
    candidates.push("flower_pot");
  }
  if (displayId.includes("potted") || displayId.includes("flower_pot")) {
    candidates.push("flower_pot");
  }
  return Array.from(new Set(candidates));
}

function iconRelativePaths(candidate: string): string[] {
  return [
    `${candidate}.png`,
    `assets/minecraft/textures/block/${candidate}.png`,
    `assets/minecraft/textures/item/${candidate}.png`,
    `textures/block/${candidate}.png`,
    `textures/item/${candidate}.png`,
    `block/${candidate}.png`,
    `item/${candidate}.png`,
  ];
}

async function loadIcon(blockId: string, slot: BlockIconSlot): Promise<string | null> {
  if (!blockId) return null;
  const roots = await getActiveIconSearchRoots(slot);
  for (const root of roots) {
    for (const candidate of iconCandidates(blockId)) {
      for (const relativePath of iconRelativePaths(candidate)) {
        try {
          const dataUrl = await readIconFromRoot(root, relativePath);
          if (dataUrl) return dataUrl;
        } catch {
          // Try the next candidate.
        }
      }
    }
  }
  return null;
}

/**
 * Clears the icon promise cache after active icon resources change.
 */
export function invalidateBlockIconCache(): void {
  iconCache.clear();
}

/**
 * Loads and caches the data URL for a Minecraft block or item icon.
 */
export function getBlockIconDataUrl(blockId: string, slot: BlockIconSlot = "material_list"): Promise<string | null> {
  const cacheKey = `${slot}:${blockId}`;
  if (!iconCache.has(cacheKey)) {
    iconCache.set(cacheKey, loadIcon(blockId, slot));
  }
  return iconCache.get(cacheKey)!;
}
