import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import { closeApplicationWindow } from "../../../src/platform/nativeWindowGeometry";

import { loadUserConfigMigratingLocalStorage, normalizeMaterialListWindowBehavior, openAssetManagerWindow as openAssetManagerDesktopWindow } from "../../../src/business/facade";
import { listenEvent } from "../../../src/platform/events";
import {
  applyThemeStylesheet,
  currentThemeId,
  subscribeToThemeChanges,
  themeClassName,
} from "../../shell/themeRuntime";
import { AssetManagerContent } from "./AssetManagerContent";

/** Opens the resource manager using the existing auxiliary-window preference. */
export async function openAssetManagerWithWindowBehavior(showOverlay: () => void): Promise<void> {
  const info = await loadUserConfigMigratingLocalStorage().catch(() => null);
  if (normalizeMaterialListWindowBehavior(info?.config.material_list_window_behavior) === "independent_window") {
    try { await openAssetManagerDesktopWindow(); return; } catch { /* In-window mode remains reachable. */ }
  }
  showOverlay();
}
export function AssetManagerDialog({ theme, onClose }: { theme: string; onClose: () => void }) {
  return <div className="dialog-overlay"><section
    className={["dialog-content", "subwindow-frame", "subwindow-frame-wide", "asset-manager-window", themeClassName(theme)].filter(Boolean).join(" ")}
    role="dialog" aria-modal="true" aria-label="游戏资源管理" onClick={(event) => event.stopPropagation()}>
    <AssetManagerContent onClose={onClose} />
  </section></div>;
}

/**
 * Standalone game-resource manager window; follows the main shell theme.
 */
export function AssetManagerWindow() {
  const [themeId, setThemeId] = useState(currentThemeId);
  const [openedSection, setOpenedSection] = useState("");

  const handleClose = useCallback(() => {
    void closeApplicationWindow();
  }, []);

  useLayoutEffect(() => {
    applyThemeStylesheet(themeId);
  }, [themeId]);

  useEffect(() => {
    setThemeId(currentThemeId());
    return subscribeToThemeChanges(setThemeId);
  }, []);

  useEffect(() => {
    const unlistenPromise = listenEvent<string | null>("asset-manager-open-section", (event) => {
      setOpenedSection(event.payload || "");
    }).catch(() => undefined);
    return () => {
      unlistenPromise.then((unlisten) => unlisten?.());
    };
  }, []);

  return (
    <div className={["asset-manager-root", themeClassName(themeId)].filter(Boolean).join(" ")}>
      <AssetManagerContent key={openedSection} onClose={handleClose} />
    </div>
  );
}
