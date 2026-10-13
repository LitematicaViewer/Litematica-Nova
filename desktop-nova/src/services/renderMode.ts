export type DisplayMode = "fast_experimental";
export const RENDER_DISPLAY_MODE_KEY = "lba.render.displayMode";
export const DISPLAY_MODE_OPTIONS: Array<{ label: string; value: DisplayMode }> = [
  { label: "快速模式", value: "fast_experimental" },
];

/** Old full/normal settings never select a private or unavailable implementation. */
export function normalizeDisplayMode(_value: unknown): DisplayMode { return "fast_experimental"; }
export function loadDisplayMode(): DisplayMode { return normalizeDisplayMode(localStorage.getItem(RENDER_DISPLAY_MODE_KEY)); }
export function saveDisplayMode(value: string): DisplayMode {
  const mode = normalizeDisplayMode(value);
  localStorage.setItem(RENDER_DISPLAY_MODE_KEY, mode);
  return mode;
}
