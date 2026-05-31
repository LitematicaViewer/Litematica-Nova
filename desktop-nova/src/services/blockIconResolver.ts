import { loadEnumeratorCollections } from "./enumeratorService";
import {
  getActiveIconSearchRoots,
  listGameResourceRegistry,
  readIconFromRoot,
  type BlockIconSlot,
  type GameResourceEntry,
  type IconSearchRoot,
} from "./gameResources";

const iconCache = new Map<string, Promise<string | null>>();
let materialIconMembershipPromise: Promise<{ items: Set<string>; itemLikeBlocks: Set<string> }> | null = null;
let activeIconLayoutPromise: Promise<ActiveIconLayout> | null = null;
type IconLookupMode = "default" | "item_first";
type MaterialIconBranch = "item_like" | "block";
type MaterialIconMembership = { items: Set<string>; itemLikeBlocks: Set<string> };
type IconCategoryFolder = "block_itemLike" | "item" | "block_2d" | "block_icon";

interface IconPackLocation {
  baseRoot: IconSearchRoot;
  pack: string;
}

interface ActiveIconLayout {
  material: IconPackLocation | null;
  item: IconPackLocation | null;
  block2d: IconPackLocation | null;
  itemLikeBaseRoot: IconSearchRoot | null;
}

function normalizeBlockId(blockId: string): string {
  const normalized = String(blockId || "").trim().toLowerCase();
  if (!normalized) return "";
  if (normalized.startsWith("e/")) return normalized;
  return normalized.includes(":") ? normalized : `minecraft:${normalized}`;
}

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

function normalizeLookupKey(value: string): string {
  return String(value || "").trim().toLowerCase();
}

function isItemLikeBlockCollection(collection: { id: string; name: string }): boolean {
  const normalizedId = normalizeLookupKey(collection.id);
  const normalizedName = normalizeLookupKey(collection.name);
  return normalizedId === "custom:itemlike_block" || normalizedName === "itemlike_block";
}

async function loadMaterialIconMembership(): Promise<MaterialIconMembership> {
  if (!materialIconMembershipPromise) {
    materialIconMembershipPromise = loadEnumeratorCollections()
      .then((collections) => ({
        items: new Set((collections.find((collection) => collection.id === "base:dv-items")?.values || []).map(normalizeBlockId)),
        itemLikeBlocks: new Set(
          collections
            .filter((collection) => isItemLikeBlockCollection(collection))
            .flatMap((collection) => collection.values)
            .map(normalizeBlockId),
        ),
      }))
      .catch(() => ({ items: new Set<string>(), itemLikeBlocks: new Set<string>() }));
  }
  return materialIconMembershipPromise;
}

function splitPackLocation(root: IconSearchRoot): IconPackLocation | null {
  const normalized = String(root.root || "").replace(/\\/g, "/").replace(/\/+$/, "");
  const match = normalized.match(/^(.*)\/(block_itemLike|item|block_2d|block_icon)\/([^/]+)$/i);
  if (!match) return null;
  const prefix = match[1].replace(/\/+$/, "");
  const pack = String(match[3] || "").trim();
  if (!prefix || !pack) return null;
  return {
    baseRoot: { ...root, root: prefix },
    pack,
  };
}

function resolveEntryRoot(entry: GameResourceEntry | null | undefined): IconSearchRoot | null {
  if (!entry) return null;
  if (entry.source === "external" && entry.root_path) {
    return { source: "absolute", root: entry.root_path, label: entry.label };
  }
  if (entry.root_relpath) {
    return { source: "app_data", root: entry.root_relpath, label: entry.label };
  }
  return null;
}

async function loadActiveIconLayout(): Promise<ActiveIconLayout> {
  if (!activeIconLayoutPromise) {
    activeIconLayoutPromise = (async () => {
      await Promise.all([
        getActiveIconSearchRoots("material_list"),
        getActiveIconSearchRoots("layering"),
      ]);
      const snapshot = await listGameResourceRegistry();
      const material = splitPackLocation(resolveEntryRoot(snapshot.active_material_list_icons) || { source: "app_data", root: "", label: "" });
      const item = splitPackLocation(resolveEntryRoot(snapshot.active_layering_item_icons) || { source: "app_data", root: "", label: "" });
      const block2d = splitPackLocation(resolveEntryRoot(snapshot.active_layering_block_icons) || { source: "app_data", root: "", label: "" });
      return {
        material,
        item,
        block2d,
        itemLikeBaseRoot: item?.baseRoot || block2d?.baseRoot || material?.baseRoot || null,
      };
    })();
  }
  return activeIconLayoutPromise;
}

function buildStorageRelativePath(folder: IconCategoryFolder, pack: string, candidate: string): string | null {
  const normalizedPack = String(pack || "").trim();
  if (!normalizedPack) return null;
  return `${folder}/${normalizedPack}/${candidate}.png`;
}

function pushCandidatePath(
  requests: Array<{ root: IconSearchRoot; relativePath: string }>,
  root: IconSearchRoot | null,
  folder: IconCategoryFolder,
  pack: string,
  candidate: string,
): void {
  if (!root) return;
  const relativePath = buildStorageRelativePath(folder, pack, candidate);
  if (!relativePath) return;
  requests.push({ root, relativePath });
}

function materialSlotRequests(
  layout: ActiveIconLayout,
  candidate: string,
  branch: MaterialIconBranch,
): Array<{ root: IconSearchRoot; relativePath: string }> {
  const requests: Array<{ root: IconSearchRoot; relativePath: string }> = [];
  if (branch === "item_like") {
    pushCandidatePath(requests, layout.itemLikeBaseRoot, "block_itemLike", "initial", candidate);
    pushCandidatePath(requests, layout.item?.baseRoot || null, "item", layout.item?.pack || "", candidate);
    pushCandidatePath(requests, layout.block2d?.baseRoot || null, "block_2d", layout.block2d?.pack || "", candidate);
    return requests;
  }
  pushCandidatePath(requests, layout.material?.baseRoot || null, "block_icon", layout.material?.pack || "", candidate);
  pushCandidatePath(requests, layout.block2d?.baseRoot || null, "block_2d", layout.block2d?.pack || "", candidate);
  return requests;
}

function layeringCanvasRequests(
  layout: ActiveIconLayout,
  candidate: string,
): Array<{ root: IconSearchRoot; relativePath: string }> {
  const requests: Array<{ root: IconSearchRoot; relativePath: string }> = [];
  pushCandidatePath(requests, layout.block2d?.baseRoot || null, "block_2d", layout.block2d?.pack || "", candidate);
  return requests;
}

async function resolveMaterialIconBranch(blockId: string, lookupMode: IconLookupMode): Promise<MaterialIconBranch> {
  if (lookupMode === "item_first") return "item_like";
  const normalized = normalizeBlockId(blockId);
  const membership = await loadMaterialIconMembership();
  return membership.items.has(normalized) || membership.itemLikeBlocks.has(normalized) ? "item_like" : "block";
}

async function loadIcon(blockId: string, slot: BlockIconSlot, lookupMode: IconLookupMode): Promise<string | null> {
  if (!blockId) return null;
  const layout = await loadActiveIconLayout();

  if (slot === "layering") {
    for (const candidate of iconCandidates(blockId)) {
      for (const request of layeringCanvasRequests(layout, candidate)) {
        try {
          const dataUrl = await readIconFromRoot(request.root, request.relativePath);
          if (dataUrl) return dataUrl;
        } catch {
          // Try the next candidate.
        }
      }
    }
    return null;
  }

  const branch = await resolveMaterialIconBranch(blockId, lookupMode);
  for (const candidate of iconCandidates(blockId)) {
    for (const request of materialSlotRequests(layout, candidate, branch)) {
      try {
        const dataUrl = await readIconFromRoot(request.root, request.relativePath);
        if (dataUrl) return dataUrl;
      } catch {
        // Try the next candidate.
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
  materialIconMembershipPromise = null;
  activeIconLayoutPromise = null;
}

/**
 * Loads and caches the data URL for a Minecraft block or item icon.
 */
export function getBlockIconDataUrl(blockId: string, slot: BlockIconSlot = "material_list", lookupMode: IconLookupMode = "default"): Promise<string | null> {
  const cacheKey = `${slot}:${lookupMode}:${blockId}`;
  if (!iconCache.has(cacheKey)) {
    iconCache.set(cacheKey, loadIcon(blockId, slot, lookupMode));
  }
  return iconCache.get(cacheKey)!;
}
