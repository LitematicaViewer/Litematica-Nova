import { useEffect, useState } from "react";

import {
  loadUserConfigMigratingLocalStorage,
  normalizeMaterialListWindowBehavior,
  openAssetManagerWindow as openAssetManagerDesktopWindow,
} from "../../../src/business/facade";
import {
  applyThemeStylesheet,
  currentThemeId,
  subscribeToThemeChanges,
  themeClassName,
} from "../../shell/themeRuntime";
import { AssetManagerContent } from "./AssetManagerContent";

/**
 * Opens the game asset manager according to the user's configured child-window behavior.
 */
export async function openAssetManagerWithWindowBehavior(showOverlay: () => void): Promise<void> {
  const info = await loadUserConfigMigratingLocalStorage().catch(() => null);
  const behavior = normalizeMaterialListWindowBehavior(info?.config.material_list_window_behavior);
  if (behavior === "independent_window") {
    try {
      await openAssetManagerDesktopWindow();
      return;
    } catch {
      // Browser preview cannot create a desktop window, so fall back to the in-window dialog.
    }
  }
  showOverlay();
}

export function AssetManagerDialog({ theme, onClose }: { theme: string; onClose: () => void }) {
  return (
    <div className="dialog-overlay">
      <section
        className={["dialog-content", "subwindow-frame", "subwindow-frame-wide", "asset-manager-window", themeClassName(theme)].filter(Boolean).join(" ")}
        role="dialog"
        aria-modal="true"
        aria-label="游戏资源管理"
        onClick={(event) => event.stopPropagation()}
      >
        <AssetManagerContent theme={theme} onClose={onClose} />
      </section>
    </div>
  );
}

/**
 * Standalone game asset manager window.
 */
export function AssetManagerWindow() {
  const [themeId, setThemeId] = useState(currentThemeId());

  useEffect(() => {
    applyThemeStylesheet(themeId);
  }, [themeId]);

  useEffect(() => {
    setThemeId(currentThemeId());
    return subscribeToThemeChanges(setThemeId);
  }, []);

  return (
    <main className={["subwindow-standalone-page", "asset-manager-window", themeClassName(themeId)].filter(Boolean).join(" ")}>
      <AssetManagerContent theme={themeId} />
    </main>
  );
}
