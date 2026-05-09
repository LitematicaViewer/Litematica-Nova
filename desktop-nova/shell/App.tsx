import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import type { SyntheticEvent } from "react";

import {
    FlakePage,
    GeneratePage,
    HomePage,
    LibraryPage,
    OptionsPage,
    PropertiesPage,
    ReplacePage,
    RenderPage,
    StatisticsPage,
    UiTestPage
} from "../windows/main";
import {
    analyzeProjection,
    chooseLitematicFile,
    getUserConfig,
    loadProjectionLibrary,
    metricsFromAnalysis,
    startPopupViewer
} from "../services/novaBackendAdapter";
import type { LibraryRecord, ProjectionAnalysis } from "../services/novaBackendAdapter";
import { navItems } from "./mockData";
import {
    announceThemeChange,
    applyThemeStylesheet,
    currentThemeId,
    normalizeThemeId,
    themeClassName,
    themeResourceKey,
    webDefaultThemeId
} from "./themeRuntime";
import type { ActiveFile, MetricRow, PageKey } from "./types";

type NavIconUrls = Record<string, string>;

const iconNameFromPath = (path: string) => path.split("/").pop()?.replace(/\.[^.]+$/, "") || "";

const mapNavIcons = (modules: Record<string, string>) =>
    Object.entries(modules).reduce<NavIconUrls>((icons, [path, url]) => {
        const iconName = iconNameFromPath(path);
        if (iconName) {
            icons[iconName] = url;
        }
        return icons;
    }, {});

const shellNavIconUrls = mapNavIcons(
    import.meta.glob<string>("./resource/icon/nav/*.{svg,png}", {
        eager: true,
        import: "default",
        query: "?url"
    })
);

const themedNavIconUrls = Object.entries(
    import.meta.glob<string>("../themes/*/resource/icon/nav/*.{svg,png}", {
        eager: true,
        import: "default",
        query: "?url"
    })
).reduce<Record<string, NavIconUrls>>((themes, [path, url]) => {
    const themeKey = path.match(/\.\.\/themes\/([^/]+)\//)?.[1];
    const iconName = iconNameFromPath(path);
    if (themeKey && iconName) {
        themes[themeKey] = themes[themeKey] || {};
        themes[themeKey][iconName] = url;
    }
    return themes;
}, {});

const fallbackNavIconUrl = (iconName: string) => shellNavIconUrls[iconName] || shellNavIconUrls.undefined;

const navIconUrl = (themeId: string, iconName: string) =>
    themedNavIconUrls[themeResourceKey(normalizeThemeId(themeId))]?.[iconName] || fallbackNavIconUrl(iconName);

const handleNavIconError = (event: SyntheticEvent<HTMLImageElement>, iconName: string) => {
    const fallbackUrl = fallbackNavIconUrl(iconName);
    if (fallbackUrl && event.currentTarget.src !== fallbackUrl) {
        event.currentTarget.src = fallbackUrl;
    }
};

const topNavKeys: PageKey[] = [
    "home",
    "library",
    "properties",
    "statistics",
    "flake",
    "render",
    "replace",
    "generate"
];

const bottomNavKeys: PageKey[] = ["ui_test", "options"];

export default function App() {
    const [activePage, setActivePage] = useState<PageKey>("home");
    const [sidebarExpanded, setSidebarExpanded] = useState(true);
    const [activeFile, setActiveFile] = useState<ActiveFile | null>(null);
    const [analysis, setAnalysis] = useState<ProjectionAnalysis | null>(null);
    const [libraryRecords, setLibraryRecords] = useState<LibraryRecord[]>([]);
    const [statusMessage, setStatusMessage] = useState("");
    const [errorMessage, setErrorMessage] = useState("");
    const [snapshotTheme, setSnapshotTheme] = useState(currentThemeId());

    useEffect(() => {
        getUserConfig()
            .then((snapshot) => {
                setSnapshotTheme(normalizeThemeId(snapshot.config.theme));
            })
            .catch(() => {
                setSnapshotTheme(webDefaultThemeId);
            });
        loadProjectionLibrary().then(setLibraryRecords).catch(() => setLibraryRecords([]));
    }, []);

    useEffect(() => {
        applyThemeStylesheet(snapshotTheme);
        announceThemeChange(snapshotTheme);
    }, [snapshotTheme]);

    const metrics = useMemo<MetricRow[]>(
        () => metricsFromAnalysis(analysis),
        [analysis]
    );

    const triggerFileSelect = async () => {
        setErrorMessage("");
        const file = await chooseLitematicFile();
        if (!file) {
            return;
        }
        setActiveFile(file);
        setAnalysis(null);
        setStatusMessage("正在分析选中的 .litematic...");
        try {
            const result = await analyzeProjection(file.path);
            setAnalysis(result);
            setActivePage("properties");
            setStatusMessage("分析完成。");
        } catch (error) {
            setErrorMessage(String(error));
            setStatusMessage("");
            setActivePage("properties");
        }
    };

    const handleThemeChange = (themeId: string) => {
        setSnapshotTheme(normalizeThemeId(themeId));
    };

    const openMaterialListWindow = (filePath?: string | null) => {
        invoke("open_material_list_window", { activeFile: filePath || null }).catch((error) => {
            console.error("Failed to open material list window", error);
        });
    };

    const openNativeViewer = async () => {
        if (!activeFile) {
            setErrorMessage("尚未选择 .litematic 文件。");
            return;
        }
        setErrorMessage("");
        await startPopupViewer(activeFile.path, "full").catch((error) => {
            setErrorMessage(String(error));
        });
    };

    return (
        <div className={["app-shell", themeClassName(snapshotTheme)].filter(Boolean).join(" ")}>
            <aside className={sidebarExpanded ? "sidebar" : "sidebar sidebar-collapsed"}>
                <button
                    className="nav-expand"
                    type="button"
                    title={sidebarExpanded ? "收起侧栏" : "展开侧栏"}
                    onClick={() => setSidebarExpanded((value) => !value)}
                >
                    <img
                        className="nav-icon nav-icon-hamburger"
                        src={navIconUrl(snapshotTheme, "hamburger")}
                        alt=""
                        aria-hidden="true"
                        onError={(event) => handleNavIconError(event, "hamburger")}
                    />
                    {sidebarExpanded ? <strong>Litematica Nova</strong> : null}
                </button>
                <nav className="nav-list" aria-label="主导航">
                    {topNavKeys.map((key) => (
                        <NavButton
                            key={key}
                            pageKey={key}
                            activePage={activePage}
                            expanded={sidebarExpanded}
                            themeId={snapshotTheme}
                            onNavigate={setActivePage}
                        />
                    ))}
                </nav>
                <nav className="nav-list nav-list-bottom" aria-label="调试与选项">
                    {bottomNavKeys.map((key) => (
                        <NavButton
                            key={key}
                            pageKey={key}
                            activePage={activePage}
                            expanded={sidebarExpanded}
                            themeId={snapshotTheme}
                            onNavigate={setActivePage}
                        />
                    ))}
                </nav>
            </aside>
            <main className="content">
                {statusMessage ? <div className="runtime-status">{statusMessage}</div> : null}
                {errorMessage ? <div className="runtime-error">{errorMessage}</div> : null}
                {activePage === "home" ? (
                    <HomePage
                        onOpenFile={triggerFileSelect}
                        onOpenStatistics={() => setActivePage("statistics")}
                        onOpenRender={() => setActivePage("render")}
                    />
                ) : null}
                {activePage === "library" ? (
                    <LibraryPage
                        records={libraryRecords}
                        onSelectRecord={(record) => {
                            setActiveFile({
                                name: record.fileName || record.displayName || record.path.split(/[\\/]/).pop() || record.path,
                                path: record.path,
                                size: "-"
                            });
                            setActivePage("properties");
                        }}
                    />
                ) : null}
                {activePage === "properties" ? (
                    <PropertiesPage
                        activeFile={activeFile}
                        analysis={analysis}
                        onOpenFile={triggerFileSelect}
                        onOpenViewer={openNativeViewer}
                    />
                ) : null}
                {activePage === "statistics" ? (
                    <StatisticsPage
                        activeFile={activeFile}
                        metrics={metrics}
                        onOpenMaterialList={() => openMaterialListWindow(activeFile?.path)}
                    />
                ) : null}
                {activePage === "flake" ? (
                    <FlakePage
                        activeFile={activeFile}
                        onOpenMaterialList={() => openMaterialListWindow(activeFile?.path)}
                    />
                ) : null}
                {activePage === "render" ? (
                    <RenderPage
                        activeFile={activeFile}
                        onOpenMaterialList={() => openMaterialListWindow(activeFile?.path)}
                        onOpenViewer={openNativeViewer}
                    />
                ) : null}
                {activePage === "replace" ? <ReplacePage activeFile={activeFile} /> : null}
                {activePage === "generate" ? <GeneratePage /> : null}
                {activePage === "ui_test" ? <UiTestPage /> : null}
                {activePage === "options" ? <OptionsPage theme={snapshotTheme} onThemeChange={handleThemeChange} /> : null}
            </main>
        </div>
    );
}

function NavButton({
    pageKey,
    activePage,
    expanded,
    themeId,
    onNavigate
}: {
    pageKey: PageKey;
    activePage: PageKey;
    expanded: boolean;
    themeId: string;
    onNavigate: (key: PageKey) => void;
}) {
    const item = navItems.find((candidate) => candidate.key === pageKey);
    if (!item) {
        return null;
    }
    const checked = activePage === pageKey;
    return (
        <button
            className={checked ? "nav-item nav-item-active" : "nav-item"}
            type="button"
            aria-pressed={checked}
            title={expanded ? undefined : item.label}
            onClick={() => onNavigate(pageKey)}
        >
            <img
                className={`nav-icon nav-icon-${item.icon}`}
                src={navIconUrl(themeId, item.icon)}
                alt=""
                aria-hidden="true"
                onError={(event) => handleNavIconError(event, item.icon)}
            />
            {expanded ? <span className="nav-label">{item.label}</span> : null}
        </button>
    );
}
