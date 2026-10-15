import { listenEvent } from "../platform/events";
import { readImageBase64 } from "../platform/files";
import {
  getActiveIconSearchRoots,
  getActiveResourceFingerprint,
  readIconFromRoot,
  type BlockIconSlot,
  type IconSearchRoot,
} from "./gameResources";
import { loadEnumeratorCollections } from "./enumeratorService";

const iconCache = new Map<string, Promise<string | null>>();

import catalogData from "../../../pack-in/materials/minecraft-26.3.json";
import iconManifest from "../../../pack-in/materials/item-icons-26.3.json";
const verifiedIcons = iconManifest.entries as Record<string, { path: string }>;
const catalog = catalogData as { items: Record<string, unknown>; blocks: Record<string, { item_id: string }> };

/** Bundled icon folder, used as the last resort when the registry has no entry. */
const BUNDLED_ICON_ROOT = "pack-in/item-icons-26.3";

/**
 * Icon candidates for a block, in lookup order.
 *
 * Two of these are literal rules (a slab is never represented by planks, so the
 * plank texture is only a *fallback*; potted plants fall back to the flower pot):
 *
 *   `_slab`    -> `_planks`
 *   `potted_*` -> `flower_pot`
 *
 * Whether a block is expected to have an *item-shaped* icon is not decided here -
 * that membership comes from the enumerator collection rule file
 * `pack-in/enumerator/custom/itemlike_block.json` (see `loadItemLikeBlocks`).
 */
export function iconCandidates(blockId: string): string[] {
  const id = blockId.includes(":") ? blockId : `minecraft:${blockId}`;
  if (!id.startsWith("minecraft:")) return [];
  const item = catalog.items[id] ? id : catalog.blocks[id]?.item_id;
  return item && verifiedIcons[item] ? [item.slice("minecraft:".length)] : [];
}
function literalCandidates(blockId: string): string[] {
  const id = String(blockId || "").trim().replace(/^minecraft:/, "");
  return id && !id.includes(":") && !id.startsWith("E/") ? [id] : [];
}

/**
 * Candidates backed by the registry alias rules.
 *
 * The registry resolves a block to its `item_id` and only aliases present in the
 * bundled icon manifest are accepted, which keeps the bundled fallback honest.
 */
function registryCandidates(blockId: string): string[] {
  const id = blockId.includes(":") ? blockId : `minecraft:${blockId}`;
  if (!id.startsWith("minecraft:")) return [];
  const item = catalog.items[id] ? id : catalog.blocks[id]?.item_id;
  return item && verifiedIcons[item] ? [item.slice("minecraft:".length)] : [];
}

let itemLikeBlocksPromise: Promise<Set<string>> | null = null;

/**
 * Blocks listed as item-shaped by the enumerator rule file
 * (`pack-in/enumerator/custom/itemlike_block.json`, collection id
 * `custom:itemlike_block`). They are looked up through the icon-pack folders that
 * hold item-style artwork instead of the flat `block_2d` layout.
 */
async function loadItemLikeBlocks(): Promise<Set<string>> {
  if (!itemLikeBlocksPromise) {
    itemLikeBlocksPromise = loadEnumeratorCollections()
      .then((collections) => {
        const itemLike = collections.find(
          (collection) =>
            collection.id === "custom:itemlike_block" ||
            collection.name.trim().toLowerCase() === "itemlike_block",
        );
        return new Set((itemLike?.values || []).map((value) => String(value || "").trim().toLowerCase()));
      })
      .catch(() => new Set<string>());
  }
  return itemLikeBlocksPromise;
}

/** Drops the cached enumerator-derived membership (call together with the icon cache). */
export function invalidateItemLikeRuleCache(): void {
  itemLikeBlocksPromise = null;
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
 * Candidate paths inside one registry search root, most specific first.
 *
 * `item_like` blocks (per the enumerator rule file) are tried through the
 * item-style folders first, then the block folders; everything else is the
 * reverse. The flat `<candidate>.png` layout is always last.
 */
function candidatePaths(
  root: IconSearchRoot,
  candidates: string[],
  itemLike: boolean,
): string[] {
  const pack = root.label ? root.label.trim() : "";
  const blockFolders = ["block_2d", "block_icon"];
  const itemFolders = ["block_itemLike", "item"];
  const folders = itemLike
    ? [...itemFolders, ...blockFolders]
    : [...blockFolders, "item"];
  const paths: string[] = candidates.map((candidate) => `${candidate}.png`);
  if (pack) {
    for (const folder of folders) {
      for (const candidate of candidates) {
        paths.push(`${folder}/${pack}/${candidate}.png`);
      }
    }
  }
  for (const candidate of candidates) {
    paths.push(`${candidate}.png`);
  }
  return Array.from(new Set(paths));
}

/**
 * Resolves an icon through the game-resource registry first, then the bundled pack.
 *
 * The registry returns the search roots of the *activated* icon resource for the
 * requested slot (material list vs layering), which is what makes multi-version
 * icon packs take effect. `invalidateBlockIconCache()` must be called after the
 * active resource changes.
 */
async function loadIcon(blockId: string, slot: BlockIconSlot, lookupMode: IconLookupMode): Promise<string | null> {
  if (!blockId) return null;
  const literal = literalCandidates(blockId);
  const alias = registryCandidates(blockId);
  const candidates = Array.from(new Set(slot === "layering" ? literal : lookupMode === "block_first" ? [...literal, ...alias] : [...alias, ...literal]));
  if (!candidates.length) return null;

  const normalized = String(blockId || "").replace("minecraft:", "").trim().toLowerCase();
  let itemLike = slot === "material_list" && lookupMode !== "block_first" && (lookupMode === "item_first" || alias.length > 0);
  if (slot === "material_list" && lookupMode === "default" && !itemLike) {
    const membership = await loadItemLikeBlocks();
    itemLike = membership.has(normalized) || membership.has(`minecraft:${normalized}`);
  }
  let roots: IconSearchRoot[] = [];
  try { roots = await cachedRoots(slot, itemLike ? "item_first" : lookupMode); } catch { roots = []; }

  for (const root of roots) {
    for (const relativePath of candidatePaths(root, candidates, itemLike)) {
      try {
        const dataUrl = await readIconFromRoot(root, relativePath);
        if (dataUrl) return dataUrl;
      } catch {
        // Try the next path or root.
      }
    }
  }

  if (slot === "layering") return null;
  for (const candidate of alias) {
    const dataUrl = await loadBundledIcon(candidate);
    if (dataUrl) return dataUrl;
  }
  return null;
}

export type IconLookupMode = "default" | "item_first" | "block_first";
export interface BlockIconOptions { slot?: BlockIconSlot; lookupMode?: IconLookupMode; minecraftDataVersion?: string; }
const rootCache = new Map<string, Promise<IconSearchRoot[]>>();
let resourceListenerStarted = false;
function cachedRoots(slot: BlockIconSlot, lookupMode: IconLookupMode): Promise<IconSearchRoot[]> {
  const key = `${slot}:${lookupMode}`;
  if (!rootCache.has(key)) rootCache.set(key, getActiveIconSearchRoots(slot, lookupMode === "item_first"));
  return rootCache.get(key)!;
}
function observeResources(): void {
  if (resourceListenerStarted) return;
  resourceListenerStarted = true;
  void listenEvent("game-resources-changed", () => { invalidateBlockIconCache(); invalidateItemLikeRuleCache(); })
    .catch(() => { resourceListenerStarted = false; });
}

/**
 * Loads and caches the data URL for a Minecraft block or item icon.
 */
export function getBlockIconDataUrl(
  blockId: string,
  slotOrOptions: BlockIconSlot | BlockIconOptions = "material_list",
  lookupMode: IconLookupMode = "default",
): Promise<string | null> {
  observeResources();
  const slot = typeof slotOrOptions === "string" ? slotOrOptions : slotOrOptions.slot || "material_list";
  const mode = typeof slotOrOptions === "string" ? lookupMode : slotOrOptions.lookupMode || lookupMode;
  const key = `${slot}:${mode}:${blockId}`;
  if (!iconCache.has(key)) iconCache.set(key, loadIcon(blockId, slot, mode));
  return iconCache.get(key)!;
}

/** Drops all cached icons so the next lookup re-reads the active resource. */
export function invalidateBlockIconCache(): void {
  iconCache.clear();
  rootCache.clear();
  invalidateItemLikeRuleCache();
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
