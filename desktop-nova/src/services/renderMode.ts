import { t } from "./i18n";
export type DisplayMode = "normal" | "fast_experimental" | "standard" | "full";
export type RenderBuildMode = "prebuilt" | "realtime";

export interface V5RenderModeConfig {
  backend?: unknown;
  shaderpack_root?: unknown;
  shaderpack_name?: unknown;
  fabric_mods_dir?: unknown;
  resource_packs?: readonly unknown[] | null;
}

export const RENDER_DISPLAY_MODE_KEY = "lba.render.displayMode";
export const RENDER_BUILD_MODE_KEY = "lba.render.buildMode";

export const DISPLAY_MODE_OPTIONS: Array<{ label: string; value: DisplayMode }> = [
  { get label() { return t("render.mode.fast"); }, value: "fast_experimental" },
  { get label() { return t("render.mode.standard"); }, value: "standard" },
  { get label() { return t("native.mode.full"); }, value: "full" },
];

export const RENDER_BUILD_MODE_OPTIONS: Array<{ label: string; value: RenderBuildMode }> = [
  { get label() { return t("native.mode.prebuilt"); }, value: "prebuilt" },
  { get label() { return t("native.mode.realtime"); }, value: "realtime" },
];

export function normalizeRenderBuildMode(value: unknown): RenderBuildMode {
  return value === "realtime" ? "realtime" : "prebuilt";
}

export function loadRenderBuildMode(): RenderBuildMode {
  return normalizeRenderBuildMode(localStorage.getItem(RENDER_BUILD_MODE_KEY));
}

export function saveRenderBuildMode(value: string): RenderBuildMode {
  const buildMode = normalizeRenderBuildMode(value);
  localStorage.setItem(RENDER_BUILD_MODE_KEY, buildMode);
  return buildMode;
}

export function normalizeDisplayMode(value: unknown): DisplayMode {
  if (value === "fast" || value === "fast_experimental") return "fast_experimental";
  if (value === "normal" || value === "standard") return "standard";
  return "full";
}

function hasText(value: unknown): boolean {
  return typeof value === "string" && value.trim().length > 0;
}

/**
 * V5-only inputs cannot be represented by the legacy normal/fast render paths.
 */
export function requiresV5FullMode(config: V5RenderModeConfig | null | undefined): boolean {
  if (!config) return false;
  const backend = typeof config.backend === "string" ? config.backend.trim().toLowerCase() : "auto";
  return (
    (backend !== "" && backend !== "auto")
    || hasText(config.shaderpack_root)
    || hasText(config.shaderpack_name)
    || hasText(config.fabric_mods_dir)
    || (Array.isArray(config.resource_packs) && config.resource_packs.some(hasText))
  );
}

export function resolveV5DisplayMode(value: unknown, config: V5RenderModeConfig | null | undefined): DisplayMode {
  // User-selected product modes are authoritative. V5 options are validated by
  // the selected native backend, never a silent Standard/Fast -> Full switch.
  void config;
  return normalizeDisplayMode(value);
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
