import { useEffect, useLayoutEffect, useState } from "react";
import { t } from "../../../src/services/i18n";
import { listenEvent } from "../../../src/platform/events";
import { useI18n } from "../../i18n";
import { applyThemeStylesheet, currentThemeId, subscribeToThemeChanges, themeClassName } from "../../shell/themeRuntime";
import { MaterialCatalogDialog } from "../main/pages/statistics/MaterialCatalogDialog";
import "../main/pages/statistics/materials.css";

function initialFileFromUrl(): string {
  return new URLSearchParams(window.location.search).get("file") || "";
}

/** Existing single-instance native window; the content never mutates a task. */
export function MaterialListWindow() {
  const { revision } = useI18n();
  const [themeId, setThemeId] = useState(currentThemeId);
  const [currentFile, setCurrentFile] = useState(initialFileFromUrl);
  useLayoutEffect(() => { applyThemeStylesheet(themeId); }, [themeId]);
  useEffect(() => subscribeToThemeChanges(setThemeId), []);
  useEffect(() => { document.title = t("statistics.catalogTitle"); }, [revision]);
  useEffect(() => {
    const pending = listenEvent<string | null>("material-list-open-file", event => setCurrentFile(event.payload || ""));
    return () => { void pending.then(stop => stop()); };
  }, []);
  return <main className={["material-list-window", themeClassName(themeId)].filter(Boolean).join(" ")}>
    {currentFile ? <MaterialCatalogDialog key={currentFile} currentFile={currentFile} /> : <div className="material-list-window-state">{t("library.materialSelectFirst")}</div>}
  </main>;
}
