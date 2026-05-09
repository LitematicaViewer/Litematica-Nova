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

export type ThemeName = "metro10" | "minecraft";

export function normalizeTheme(value: unknown): ThemeName {
  return value === "minecraft" ? "minecraft" : "metro10";
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
