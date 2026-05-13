import { useEffect, useState } from "react";

import {
  applyThemeStylesheet,
  currentThemeId,
  subscribeToThemeChanges,
  themeClassName,
} from "../../shell/themeRuntime";
import { LocalLibraryFoldersPanel } from "./LocalLibraryFoldersContent";

/**
 * Renders the standalone desktop window used to manage mounted local-library folders.
 */
export function LocalLibraryFoldersWindow() {
  const [themeId, setThemeId] = useState(currentThemeId());

  useEffect(() => {
    applyThemeStylesheet(themeId);
  }, [themeId]);

  useEffect(() => {
    setThemeId(currentThemeId());
    return subscribeToThemeChanges(setThemeId);
  }, []);

  return (
    <main className={["subwindow-standalone-page", themeClassName(themeId)].filter(Boolean).join(" ")}>
      <LocalLibraryFoldersPanel />
    </main>
  );
}