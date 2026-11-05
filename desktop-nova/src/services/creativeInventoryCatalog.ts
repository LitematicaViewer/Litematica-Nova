import { getGameDataContext, readCreativeInventoryCatalog, type CreativeInventoryCatalog, type CreativeInventoryQuery } from "../platform/files";
import { t, type UiLocale } from "./i18n";
import bundled from "./creativeInventoryBundled.json";
declare const __NOVA_CREATIVE_CATALOG_BINDING__: string | null;

type Collections = NonNullable<CreativeInventoryQuery["extraCollections"]>;
export type CreativeCatalogSource = { version?: string; contentId?: string; resourceIdentity?: string; extraCollections?: Collections };
const cache = new Map<string, Promise<CreativeInventoryCatalog>>();
export const invalidateCreativeInventoryCatalog = () => cache.clear();

// Registry + exact inventory.json placement intersection, not a guessed
// version range or an item list. Unknown/imported identities use native data.
export function bundledCreativeGroups(contentId?: string, version?: string): string[] {
  if (!contentId || !version || __NOVA_CREATIVE_CATALOG_BINDING__ !== "nova-catalog-v1:" + contentId) return [];
  const record = Object.entries(bundled.versionsByMask).find(([, versions]) => versions.includes(version));
  return record ? bundled.categories.filter((_, index) => (Number(record[0]) & (1 << index)) !== 0) : [];
}
// Two locales × (base or projection-specific). A new identity replaces the
// previous record rather than accumulating every file the user has opened.
type Stored = {key: string; data: CreativeInventoryCatalog};
let database: Promise<IDBDatabase | null> | undefined;
function catalogStore() {
  return database ??= new Promise(resolve => {
    if (typeof indexedDB === "undefined") { resolve(null); return; }
    const open = indexedDB.open("nova-creative-catalog", 1);
    open.onupgradeneeded = () => open.result.createObjectStore("snapshots");
    open.onsuccess = () => resolve(open.result);
    open.onerror = open.onblocked = () => resolve(null);
  });
}
async function storedCatalog(slot: string, write?: Stored): Promise<Stored | undefined> {
  const db = await catalogStore();
  if (!db) return;
  return new Promise((resolve, reject) => {
    const tx = db.transaction("snapshots", write ? "readwrite" : "readonly");
    const store = tx.objectStore("snapshots");
    const request = write ? store.put(write, slot) : store.get(slot);
    tx.oncomplete = () => resolve(write ? undefined : request.result as Stored | undefined);
    tx.onabort = tx.onerror = () => reject(tx.error);
  });
}
export async function prepareCreativeInventoryCatalog(source: CreativeCatalogSource, language: UiLocale, onIdentity?: (contentId: string, version: string) => void): Promise<CreativeInventoryCatalog> {
  const context = await getGameDataContext();
  if (source.contentId && source.contentId !== context.content_id) throw new Error(t("flake.creativeResourceChanged"));
  const version = source.version || context.game_version;
  const contentId = context.content_id;
  onIdentity?.(contentId, version);
  const extraCollections = source.extraCollections ?? [];
  const key = JSON.stringify([contentId, version, language, source.resourceIdentity, extraCollections]);
  const previous = cache.get(key);
  if (previous) return previous;
  const pending = (async () => {
    const slot = `${language}:${extraCollections.length ? "projection" : "base"}`;
    const stored = await storedCatalog(slot).catch(() => undefined);
    if (stored?.key === key && stored.data?.contentId === contentId && stored.data.version === version
      && Array.isArray(stored.data.items) && Array.isArray(stored.data.categories)) return stored.data;
    const result = await readCreativeInventoryCatalog({version, contentId, language, extraCollections});
    if (result.contentId !== contentId || result.version !== version) throw new Error(t("flake.creativeResourceChanged"));
    void storedCatalog(slot, {key, data:result}).catch(() => undefined);
    return result;
  })().catch(error => {
    if (cache.get(key) === pending) cache.delete(key);
    throw error;
  });
  if (cache.size >= 4) cache.delete(cache.keys().next().value!);
  cache.set(key, pending);
  return pending;
}

export function filterCreativeItems(data: CreativeInventoryCatalog, category: string, search: string) {
  const group = data.categories.find(value => value.id === category);
  const query = search.trim().toLowerCase();
  return (group?.indices ?? []).map(index => data.items[index]).filter(item => !query
    || [item.id, item.name, item.sourceName, item.itemId, item.blockState].some(value => value.toLowerCase().includes(query)));
}
