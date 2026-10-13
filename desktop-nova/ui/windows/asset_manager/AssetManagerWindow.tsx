import { useEffect, useLayoutEffect, useState } from "react";

import { listenEvent } from "../../../src/platform/events";
import {
  applyThemeStylesheet,
  currentThemeId,
  subscribeToThemeChanges,
  themeClassName,
} from "../../shell/themeRuntime";
import { AssetManagerContent } from "./AssetManagerContent";

/**
 * Standalone game-resource manager window; follows the main shell theme.
 */
export function AssetManagerWindow() {
  const [themeId, setThemeId] = useState(currentThemeId);
  const [openedSection, setOpenedSection] = useState("");

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
      <AssetManagerContent key={openedSection} />
    </div>
  );
}
