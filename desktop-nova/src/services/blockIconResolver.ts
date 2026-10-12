import { readImageBase64 } from "../platform/files";

const iconCache = new Map<string, Promise<string | null>>();

// Only registry aliases are accepted. A slab is never represented by planks.
import catalogData from "../../../pack-in/materials/minecraft-26.3.json";
import iconManifest from "../../../pack-in/materials/item-icons-26.3.json";
const verifiedIcons = iconManifest.entries as Record<string, { path: string }>;
const catalog = catalogData as { items: Record<string, unknown>; blocks: Record<string, { item_id: string }> };

export function iconCandidates(blockId: string): string[] {
  const id = blockId.includes(":") ? blockId : `minecraft:${blockId}`;
  if (!id.startsWith("minecraft:")) return [];
  const item = catalog.items[id] ? id : catalog.blocks[id]?.item_id;
  return item && verifiedIcons[item] ? [item.slice("minecraft:".length)] : [];
}

async function loadIcon(blockId: string): Promise<string | null> {
  if (!blockId) return null;
  const paths = ["pack-in/item-icons-26.3"];
  for (const folder of paths) {
    for (const candidate of iconCandidates(blockId)) {
      try {
        const dataUrl = await readImageBase64(`${folder}/${candidate}.png`);
        if (dataUrl) return dataUrl;
      } catch {
        // Try the next candidate.
      }
    }
  }
  return null;
}

/**
 * Loads and caches the data URL for a Minecraft block or item icon.
 */
export function getBlockIconDataUrl(blockId: string): Promise<string | null> {
  if (!iconCache.has(blockId)) {
    iconCache.set(blockId, loadIcon(blockId));
  }
  return iconCache.get(blockId)!;
}
