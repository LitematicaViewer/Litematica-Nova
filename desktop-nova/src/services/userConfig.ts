import {
  getUserConfig,
  saveUserConfig,
  UserConfigInfo,
} from "./backend";
import { normalizeDisplayMode, DisplayMode } from "./renderMode";

const THEME_KEY = "theme";
const RENDER_MODE_KEY = "lba.render.displayMode";
const PREVIEW_MODE_KEY = "lba.preview.displayMode";
const MIGRATION_KEY = "lba.appdataConfigMigrated.v1";
const DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES = [
  "理论版本 = version(L)",
  "生存不可达 = any(L & survival_impossible)",
] as const;

export type ThemeName = "WebDefault" | "Bootstrap5" | "Metro10" | "Minecraft";
export type MaterialListWindowBehavior = "independent_window" | "main_window_overlay";
const DEFAULT_LOCAL_LIBRARY_TAIL_PATH_COUNT = 3;

export function normalizeShowUiTestPage(value: unknown): boolean {
  return value !== false;
}

export function normalizeTheme(value: unknown): ThemeName {
  const key = String(value || "").trim().toLowerCase();
  if (key === "bootstrap5") return "Bootstrap5";
  if (key === "metro10") return "Metro10";
  if (key === "minecraft") return "Minecraft";
  return "WebDefault";
}

/**
 * Normalizes the material list window behavior stored in user config.
 */
export function normalizeMaterialListWindowBehavior(value: unknown): MaterialListWindowBehavior {
  return String(value || "").trim() === "main_window_overlay" ? "main_window_overlay" : "independent_window";
}

export function normalizeLocalLibraryTailPathCount(value: unknown): number {
  const numeric = Math.floor(Number(value));
  return Number.isFinite(numeric) && numeric >= 1 ? numeric : DEFAULT_LOCAL_LIBRARY_TAIL_PATH_COUNT;
}

export function normalizeStatisticsEnumeratorInfoRules(value: unknown): string[] {
  if (!Array.isArray(value)) return [...DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES];
  const normalized = value
    .map((item) => String(item || "").trim())
    .filter(Boolean);
  return normalized.length > 0 ? normalized : [...DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES];
}

export async function loadUserConfigMigratingLocalStorage(): Promise<UserConfigInfo> {
  const info = await getUserConfig();
  if (localStorage.getItem(MIGRATION_KEY) !== "1") {
    const patch: Record<string, string> = {};
    const oldTheme = localStorage.getItem(THEME_KEY);
    const oldRenderMode = localStorage.getItem(RENDER_MODE_KEY);
    const oldPreviewMode = localStorage.getItem(PREVIEW_MODE_KEY);
    if (oldTheme) patch.theme = normalizeTheme(oldTheme);
    if (oldRenderMode) patch.render_display_mode = normalizeDisplayMode(oldRenderMode);
    if (oldPreviewMode) patch.preview_mode = normalizeDisplayMode(oldPreviewMode);
    localStorage.setItem(MIGRATION_KEY, "1");
    if (Object.keys(patch).length > 0) {
      return await saveUserConfig(patch);
    }
  }
  return info;
}

export async function saveThemeConfig(theme: string): Promise<UserConfigInfo> {
  return await saveUserConfig({ theme: normalizeTheme(theme) });
}

export async function saveRenderDisplayModeConfig(mode: string): Promise<UserConfigInfo> {
  return await saveUserConfig({ render_display_mode: normalizeDisplayMode(mode) });
}

export async function savePreviewModeConfig(mode: string): Promise<UserConfigInfo> {
  return await saveUserConfig({ preview_mode: normalizeDisplayMode(mode) as DisplayMode });
}

/**
 * Persists how the material list should open from main-window pages.
 */
export async function saveMaterialListWindowBehaviorConfig(behavior: string): Promise<UserConfigInfo> {
  return await saveUserConfig({ material_list_window_behavior: normalizeMaterialListWindowBehavior(behavior) });
}

/**
 * Persists how many trailing path segments should remain visible in local-library folder labels.
 */
export async function saveLocalLibraryTailPathCountConfig(count: number): Promise<UserConfigInfo> {
  return await saveUserConfig({ local_library_tail_path_count: normalizeLocalLibraryTailPathCount(count) });
}

export async function saveShowUiTestPageConfig(show: boolean): Promise<UserConfigInfo> {
  return await saveUserConfig({ show_ui_test_page: normalizeShowUiTestPage(show) });
}

export async function saveStatisticsEnumeratorInfoRulesConfig(rules: string[]): Promise<UserConfigInfo> {
  return await saveUserConfig({
    statistics_enumerator_info_rules: normalizeStatisticsEnumeratorInfoRules(rules),
  });
}

export { DEFAULT_STATISTICS_ENUMERATOR_INFO_RULES };
