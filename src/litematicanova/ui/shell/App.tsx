import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ChangeEvent, SyntheticEvent } from "react";

import {
    FlakePage,
    HomePage,
    LibraryPage,
    OptionsPage,
    PlaceholderPage,
    PropertiesPage,
    RenderPage,
    StatisticsPage,
    UiTestPage
} from "../windows/main";
import { emptyMetrics, loadedMetrics, navItems } from "./mockData";
import {
    announceThemeChange,
    applyThemeStylesheet,
    currentThemeId,
    normalizeThemeId,
    themeClassName,
    themeResourceKey,
    webDefaultThemeId
} from "./themeRuntime";
import type { ActiveFile, MetricRow, PageKey, Snapshot } from "./types";

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
    "replace"
];

const bottomNavKeys: PageKey[] = ["ui_test", "options"];

/**
 * React replica of the original PyQt shell using system-native form controls and layout CSS only.
 */
export default function App() {
    const [activePage, setActivePage] = useState<PageKey>("home");
    const [sidebarExpanded, setSidebarExpanded] = useState(true);
    const [activeFile, setActiveFile] = useState<ActiveFile | null>(null);
    const [snapshotTheme, setSnapshotTheme] = useState(currentThemeId());
    const fileInputRef = useRef<HTMLInputElement | null>(null);

    useEffect(() => {
        invoke<Snapshot>("get_app_snapshot")
            .then((snapshot) => {
                if (snapshot.theme) {
                    setSnapshotTheme(normalizeThemeId(snapshot.theme));
                }
                if (snapshot.active_file) {
                    setActiveFile({
                        name: snapshot.active_file.split(/[\\/]/).pop() || snapshot.active_file,
                        path: snapshot.active_file,
                        size: "-"
                    });
                }
            })
            .catch(() => {
                setSnapshotTheme(webDefaultThemeId);
            });
    }, []);

    useEffect(() => {
        applyThemeStylesheet(snapshotTheme);
        announceThemeChange(snapshotTheme);
    }, [snapshotTheme]);

    const metrics = useMemo<MetricRow[]>(
        () => (activeFile ? loadedMetrics : emptyMetrics),
        [activeFile]
    );

    const triggerFileSelect = () => {
        fileInputRef.current?.click();
    };

    const handleFileSelected = (event: ChangeEvent<HTMLInputElement>) => {
        const file = event.target.files?.[0];
        if (!file) {
            return;
        }
        setActiveFile({
            name: file.name,
            path: file.name,
            size: formatBytes(file.size)
        });
        setActivePage("properties");
        event.target.value = "";
    };

    const handleThemeChange = (themeId: string) => {
        setSnapshotTheme(normalizeThemeId(themeId));
    };

    const openMaterialListWindow = (filePath?: string | null) => {
        invoke("open_material_list_window", { activeFile: filePath || null }).catch((error) => {
            console.error("Failed to open material list window", error);
        });
    };

    return (
        <div className={["app-shell", themeClassName(snapshotTheme)].filter(Boolean).join(" ")}>
            <input
                ref={fileInputRef}
                className="visually-hidden"
                type="file"
                accept=".litematic"
                onChange={handleFileSelected}
            />
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
                {activePage === "home" ? (
                    <HomePage
                        onOpenFile={triggerFileSelect}
                        onOpenStatistics={() => setActivePage("statistics")}
                        onOpenRender={() => setActivePage("render")}
                    />
                ) : null}
                {activePage === "library" ? <LibraryPage /> : null}
                {activePage === "properties" ? (
                    <PropertiesPage activeFile={activeFile} onOpenFile={triggerFileSelect} />
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
                    />
                ) : null}
                {activePage === "replace" ? <PlaceholderPage title="替换" /> : null}
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

function formatBytes(bytes: number) {
    if (bytes < 1024) {
        return `${bytes} B`;
    }
    if (bytes < 1024 * 1024) {
        return `${(bytes / 1024).toFixed(1)} KB`;
    }
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
