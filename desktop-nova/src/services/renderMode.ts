export type DisplayMode = "normal" | "fast_experimental" | "full";

export const RENDER_DISPLAY_MODE_KEY = "lba.render.displayMode";

export const DISPLAY_MODE_OPTIONS: Array<{ label: string; value: DisplayMode }> = [
  { label: "普通模式", value: "normal" },
  { label: "快速模式（实验）", value: "fast_experimental" },
  { label: "完整模式", value: "full" },
];

export function normalizeDisplayMode(value: unknown): DisplayMode {
  if (value === "fast_experimental" || value === "full") return value;
  return "normal";
}

export function loadDisplayMode(): DisplayMode {
  const displayMode = normalizeDisplayMode(localStorage.getItem(RENDER_DISPLAY_MODE_KEY));
  console.log("[LBA_RENDER_MODE]", { loaded_from_cache: true, display_mode: displayMode });
  return displayMode;
}

export function saveDisplayMode(value: string): DisplayMode {
  const displayMode = normalizeDisplayMode(value);
  localStorage.setItem(RENDER_DISPLAY_MODE_KEY, displayMode);
  console.log("[LBA_RENDER_MODE]", { saved_to_cache: true, display_mode: displayMode });
  return displayMode;
}
