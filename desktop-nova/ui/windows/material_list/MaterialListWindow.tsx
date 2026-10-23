import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../src/services/i18n";
import { useI18n } from "../../i18n";
import { Profiler, useEffect, useLayoutEffect, useMemo, useState, useSyncExternalStore } from "react";

import {
    findRenderOperationForSource,
    hasInventoryMaterialSemantics,
    getApplicationRenderState,
    loadStructureStats,
    mapStructureStats,
    recordStatisticsCommit,
    refreshStatisticsNames,
    getI18nRevision,
    subscribeI18nChanges,
    subscribeApplicationRender,
    StatsData,
} from "../../../src/business/facade";
import { listenEvent } from "../../../src/platform/events";
import {
    applyThemeStylesheet,
    currentThemeId,
    subscribeToThemeChanges,
    themeClassName
} from "../../shell/themeRuntime";
import { MaterialListContent } from "../main/pages/statistics/StatisticsPage";
import { Button } from "../../components/ui/button";

const materialListOpenFileEvent = "material-list-open-file";

const initialFileFromUrl = () => {
    try {
        return new URLSearchParams(window.location.search).get("file") || "";
    } catch {
        return "";
    }
};

/**
 * Standalone material export settings that follow the main shell theme.
 */
export function MaterialListWindow() {
  useI18n();
    const languageRevision = useSyncExternalStore(subscribeI18nChanges,getI18nRevision,getI18nRevision);
    const applicationRender = useSyncExternalStore(
        subscribeApplicationRender,
        getApplicationRenderState,
        getApplicationRenderState,
    );
    const [themeId, setThemeId] = useState(currentThemeId);
    const [currentFile, setCurrentFile] = useState(initialFileFromUrl());
    const [viewMode, setViewMode] = useState<"details" | "export">(() => new URLSearchParams(window.location.search).get("viewMode") === "export" ? "export" : "details");
    const [data, setData] = useState<StatsData | null>(null);
    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState<UiMessage>("");
    const [reloadRevision, setReloadRevision] = useState(0);
    const namedData = useMemo(() => data ? refreshStatisticsNames(data) : null,[data,languageRevision]);
    const renderOperation = useMemo(
        () => findRenderOperationForSource(currentFile, applicationRender),
        [applicationRender.operations, currentFile],
    );
    const applicationAnalysis = hasInventoryMaterialSemantics(renderOperation?.analysis) ? renderOperation!.analysis : null;
    const applicationAnalysisKey = applicationAnalysis ? renderOperation?.operationId ?? "analysis" : "";
    useLayoutEffect(() => {
        applyThemeStylesheet(themeId);
    }, [themeId]);

    useEffect(() => {
        setThemeId(currentThemeId());
        return subscribeToThemeChanges(setThemeId);
    }, []);
    useEffect(() => {
        document.title = t(viewMode === "details" ? "editor.materialDetails" : "editor.exportMaterials");
    }, [viewMode, languageRevision]);

    useEffect(() => {
        const unlistenPromise = listenEvent<string | null>(materialListOpenFileEvent, (event) => {
            setCurrentFile(event.payload || "");
        }).catch(() => undefined);
        return () => {
            unlistenPromise.then((unlisten) => unlisten?.());
        };
    }, []);

    useEffect(() => {
        const pending = listenEvent<string>("material-list-open-mode", (event) => setViewMode(event.payload === "export" ? "export" : "details")).catch(() => undefined);
        return () => { void pending.then((unlisten) => unlisten?.()); };
    }, []);

    useEffect(() => {
        let cancelled = false;
        if (viewMode === "details") {
            // Task snapshots are authoritative and remain usable without the
            // original schematic or a live Viewer. Do not reanalyse on open.
            setIsLoading(false);
            setError("");
            return;
        }
        if (!currentFile) {
            setData(null);
            setError("");
            setIsLoading(false);
            return;
        }
        if (applicationAnalysis) {
            setData(mapStructureStats(applicationAnalysis));
            setError("");
            setIsLoading(false);
            return;
        }
        setIsLoading(true);
        setError("");
        loadStructureStats(currentFile)
            .then((next) => {
                if (!cancelled) setData(next);
            })
            .catch((err) => {
                if (!cancelled) {
                    setData(null);
                    setError(String(err));
                }
            })
            .finally(() => {
                if (!cancelled) setIsLoading(false);
            });
        return () => {
            cancelled = true;
        };
    }, [currentFile, applicationAnalysisKey, reloadRevision, viewMode]);

    return (
        <main className={["material-list-window", themeClassName(themeId)].filter(Boolean).join(" ")}>
            {viewMode === "details" ? (
                <Profiler id="material-list" onRender={recordStatisticsCommit}><MaterialListContent key={currentFile} data={namedData} currentFile={currentFile} viewMode="details" onViewModeChange={setViewMode} standalone /></Profiler>
            ) : !currentFile ? (
                <div className="material-list-window-state">{t("library.materialSelectFirst")}</div>
            ) : isLoading ? (
                <div className="material-list-window-state">{t("library.materialsLoading")}</div>
            ) : error ? (
                <div className="material-list-window-state material-list-window-error"><pre role="alert">{localizeMessage(error)}</pre><Button onClick={() => setReloadRevision(value => value + 1)}>{t("stockpile.retry")}</Button></div>
            ) : namedData ? (
                <Profiler id="material-list" onRender={recordStatisticsCommit}><MaterialListContent key={currentFile} data={namedData} currentFile={currentFile} viewMode={viewMode} onViewModeChange={setViewMode} standalone /></Profiler>
            ) : (
                <div className="material-list-window-state">{t("library.noMaterials")}</div>
            )}
        </main>
    );
}
