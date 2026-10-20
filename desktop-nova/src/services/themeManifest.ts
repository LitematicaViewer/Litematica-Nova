/**
 * Single source of truth for the Nova theme list.
 *
 * Everything that has to know "which themes exist" reads this module instead of
 * repeating the three names: the runtime (stylesheet preload, body class),
 * the saved-config normalizer, and the settings picker. Adding a theme means
 * adding one entry here plus `ui/themes/<directory>/theme.css`.
 *
 * Two invariants are enforced by `scripts/themes.test.mjs`:
 *   - every `directory` below has a real `ui/themes/<directory>/theme.css`;
 *   - the canonical ids match `normalize_theme` in `src-tauri/src/main.rs`
 *     (the backend rewrites stored config, so a missing id there would turn a
 *     saved theme back into the default without any visible error).
 *
 * `id` is what config files store (case-sensitive, historical: "Minecraft").
 * `key` is the lowercase form used for the stylesheet folder, `data-theme`,
 * and the body class (`theme-<key>`).
 */

export const THEMES = [
  { id: "shadcn", key: "shadcn", label: "shadcn/ui", directory: "shadcn" },
  { id: "8bitcn", key: "8bitcn", label: "8bitcn/ui", directory: "8bitcn" },
  { id: "Minecraft", key: "minecraft", label: "Minecraft", directory: "minecraft" },
  { id: "Metro10", key: "metro10", label: "Metro10", directory: "metro10" },
] as const;

export type ThemeDefinition = (typeof THEMES)[number];
export type ThemeId = ThemeDefinition["id"];

export const DEFAULT_THEME_ID: ThemeId = "shadcn";

const byKey = new Map<string, ThemeId>(THEMES.map((theme) => [theme.key, theme.id]));

/**
 * Canonicalizes a stored or transmitted theme id.
 *
 * Unknown values deliberately fall back to the default: the previous releases
 * shipped other ids (WebDefault, Bootstrap5, QTDefault) and a config written by
 * one of them must still open. A *known* id is matched on its lowercase key, so
 * " minecraft " and "Minecraft" are the same theme. "Metro10" keeps its historical
 * spelling because configs written before it was re-adopted already store it.
 */
export const normalizeThemeId = (value: unknown): ThemeId => {
  const key = String(value ?? "").trim().toLowerCase();
  return byKey.get(key) ?? DEFAULT_THEME_ID;
};

/** The lowercase resource key a canonical id maps to (`Minecraft` -> `minecraft`). */
export const themeKeyOf = (themeId: unknown): string => {
  const id = normalizeThemeId(themeId);
  return THEMES.find((theme) => theme.id === id)?.key ?? THEMES[0].key;
};
