import catalogData from "../../../pack-in/materials/minecraft-26.3.json";

const catalog = catalogData as {
  items: Record<string, unknown>;
  blocks: Record<string, { item_id: string; creative_item_id?: string; potted_item?: string }>;
};

/** Registry-backed inventory identity. Never use this to calculate materials or rewrite block states. */
export function creativeItemId(source: string): string | null {
  const rawId = source.trim().split(/[\[{]/, 1)[0];
  const id = rawId.includes(":") ? rawId : `minecraft:${rawId}`;
  const block = catalog.blocks[id];
  const item = block?.creative_item_id || block?.potted_item || (catalog.items[id] ? id : block?.item_id);
  return item && item !== "minecraft:air" && catalog.items[item] ? item : null;
}

/** Keep the actual source value for placement; aliases only share their displayed inventory slot. */
export function creativeInventoryValues(values: string[]): string[] {
  const result = new Map<string, string>();
  for (const source of values) {
    const item = creativeItemId(source);
    if (!item) continue;
    // Item NBT/components distinguish real item variants; orientation/half block states do not.
    const nbt = source.includes("{") ? source.slice(source.indexOf("{")) : "";
    const bracket = source.match(/\[[^\]]*\]/)?.[0] || "";
    const components = bracket.includes(":") ? bracket : "";
    const data = `${components}${nbt}`;
    const key = `${item}${data}`;
    const previous = result.get(key);
    if (!previous || source === item) result.set(key, source);
  }
  return [...result.values()];
}
