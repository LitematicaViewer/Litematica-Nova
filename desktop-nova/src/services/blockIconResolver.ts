import { readImageBase64 } from "../platform/files";

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

async function loadIcon(blockId: string): Promise<string | null> {
  if (!blockId) return null;
  const paths = ["block", "item", "pack-in"];
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
