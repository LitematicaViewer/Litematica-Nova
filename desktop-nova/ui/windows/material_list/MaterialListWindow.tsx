import { useEffect, useState } from "react";

import { loadStructureStats, StatsData } from "../../../src/business/facade";
import { listenEvent } from "../../../src/platform/events";
import {
    applyThemeStylesheet,
    currentThemeId,
    subscribeToThemeChanges,
    themeClassName
} from "../../shell/themeRuntime";
import { MaterialListContent } from "../main/pages/statistics/StatisticsPage";

const materialListOpenFileEvent = "material-list-open-file";

const initialFileFromUrl = () => {
    try {
        return new URLSearchParams(window.location.search).get("file") || "";
    } catch {
        return "";
    }
};

/**
 * Standalone material-list window that follows the main shell theme.
 */
export function MaterialListWindow() {
    const [themeId, setThemeId] = useState(currentThemeId());
    const [currentFile, setCurrentFile] = useState(initialFileFromUrl());
    const [data, setData] = useState<StatsData | null>(null);
    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState("");

    useEffect(() => {
        applyThemeStylesheet(themeId);
    }, [themeId]);

    useEffect(() => {
        setThemeId(currentThemeId());
        return subscribeToThemeChanges(setThemeId);
    }, []);

    useEffect(() => {
        const unlistenPromise = listenEvent<string | null>(materialListOpenFileEvent, (event) => {
            setCurrentFile(event.payload || "");
        }).catch(() => undefined);
        return () => {
            unlistenPromise.then((unlisten) => unlisten?.());
        };
    }, []);

    useEffect(() => {
        let cancelled = false;
        if (!currentFile) {
            setData(null);
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
    }, [currentFile]);

    return (
        <main className={["material-list-window", themeClassName(themeId)].filter(Boolean).join(" ")}>
            {!currentFile ? (
                <div className="material-list-window-state">请先从主窗口选择一个投影文件并打开材料列表。</div>
            ) : isLoading ? (
                <div className="material-list-window-state">正在分析材料列表...</div>
            ) : error ? (
                <pre className="material-list-window-state material-list-window-error">{error}</pre>
            ) : data ? (
                <MaterialListContent key={currentFile} data={data} currentFile={currentFile} standalone />
            ) : (
                <div className="material-list-window-state">暂无材料数据。</div>
            )}
        </main>
    );
}
