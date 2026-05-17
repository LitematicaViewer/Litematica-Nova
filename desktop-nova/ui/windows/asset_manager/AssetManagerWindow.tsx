import { useEffect, useState } from "react";

import {
  applyThemeStylesheet,
  currentThemeId,
  subscribeToThemeChanges,
  themeClassName,
} from "../../shell/themeRuntime";
import { AssetManagerContent } from "./AssetManagerContent";

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
