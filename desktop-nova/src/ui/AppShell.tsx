import React, { useEffect, useState } from "react";
import { loadDatabases } from "../business/facade";
import { initI18n } from "../business/facade";
import { HomePage } from "./pages/HomePage";
import { LibraryPage } from "./pages/LibraryPage";
import { PropertiesPage } from "./pages/PropertiesPage";
import { StatisticsPage } from "./pages/StatisticsPage";
import { FlakePage } from "./pages/FlakePage";
import { ReplacePage } from "./pages/ReplacePage";
import { GeneratePage } from "./pages/GeneratePage";
import { RenderPage } from "./pages/RenderPage";
import { SettingsPage } from "./pages/SettingsPage";
import { hideEmbeddedViewer, showEmbeddedViewer } from "../business/facade";
import { loadUserConfigMigratingLocalStorage, normalizeTheme, saveThemeConfig } from "../business/facade";
import {
  announceThemeChange,
  applyThemeStylesheet,
  normalizeThemeId,
  themeClassName,
  themeResourceKey,
  webDefaultThemeId,
} from "../../shell/themeRuntime";

function NavIcon({ name }: { name: string }) {
  return (
    <div className="nova-nav-icon">
      {name === "home" && <svg width="18" height="18" viewBox="0 0 18 18"><path d="M2.5 8.2 9 2.4l6.5 5.8v7.3h-4.2v-4.6H6.7v4.6H2.5z" fill="currentColor" /></svg>}
      {name === "gallery" && <svg width="18" height="18" viewBox="0 0 18 18"><rect x="2.5" y="3.5" width="13" height="11" rx="2" fill="none" stroke="currentColor" /><path d="M4.5 12.5 7.2 9.7l2.1 2 1.8-1.9 2.4 2.7" fill="none" stroke="currentColor" /></svg>}
      {name === "properties" && <svg width="18" height="18" viewBox="0 0 18 18"><path d="M5 2.5h5.5L14 6v9.5H5z" fill="none" stroke="currentColor" /><path d="M7 8h4M7 11h4" stroke="currentColor" /></svg>}
      {name === "statistics" && <svg width="18" height="18" viewBox="0 0 18 18"><path d="M4 14V9m5 5V4m5 10V7" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg>}
      {name === "flake" && <svg width="18" height="18" viewBox="0 0 18 18"><path d="M9 2.5v13M3.5 6l11 6M14.5 6l-11 6" stroke="currentColor" strokeLinecap="round" /></svg>}
      {name === "render" && <svg width="18" height="18" viewBox="0 0 18 18"><path d="M9 2.5 3 5.8v6.4l6 3.3 6-3.3V5.8z" fill="none" stroke="currentColor" /><path d="M3 5.8 9 9l6-3.2M9 9v6.5" stroke="currentColor" /></svg>}
      {name === "replace" && <svg width="18" height="18" viewBox="0 0 18 18"><path d="M5 6h8l-2-2M13 12H5l2 2" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" /></svg>}
      {name === "ui_debug" && <svg width="18" height="18" viewBox="0 0 18 18"><rect x="3.5" y="3.5" width="11" height="11" rx="1.5" fill="none" stroke="currentColor" /><path d="M6 7h6M6 10h4" stroke="currentColor" /></svg>}
      {name === "options" && <svg width="18" height="18" viewBox="0 0 18 18"><circle cx="9" cy="9" r="2.2" fill="none" stroke="currentColor" /><path d="M9 2.5v2m0 9v2M2.5 9h2m9 0h2M5 5l1.4 1.4m5.2 5.2L13 13m0-8-1.4 1.4m-5.2 5.2L5 13" stroke="currentColor" strokeLinecap="round" /></svg>}
    </div>
  );
}

export function App() {
  const [currentFile, setCurrentFile] = useState<string>("");
  const [theme, setThemeState] = useState(webDefaultThemeId);
  const [route, setRoute] = useState("home");
  const [expanded, setExpanded] = useState(true);

  useEffect(() => {
    const normalized = normalizeThemeId(theme);
    document.body.className = themeClassName(normalized);
    document.documentElement.dataset.theme = themeResourceKey(normalized);
    applyThemeStylesheet(normalized);
    announceThemeChange(normalized);
  }, [theme]);

  const setTheme = (value: string) => {
    const next = normalizeThemeId(normalizeTheme(value));
    setThemeState(next);
    saveThemeConfig(next).catch(() => undefined);
  };

  useEffect(() => {
    loadDatabases();
    initI18n();
    loadUserConfigMigratingLocalStorage()
      .then((info) => setThemeState(normalizeThemeId(normalizeTheme(info.config.theme))))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    const mayOwnEmbeddedViewer = route === "properties" || route === "render";
    if (!mayOwnEmbeddedViewer) {
      console.log(`[LBA_EMBED_VIEWER] route_active=${route} action=hide reason=route_change`);
      hideEmbeddedViewer().catch(() => undefined);
      return;
    }
    console.log(`[LBA_EMBED_VIEWER] route_active=${route} action=show reason=route_change`);
    showEmbeddedViewer().catch(() => undefined);
  }, [route]);

  const topPages: Record<string, { name: string; icon: string; comp: React.FC<any> }> = {
    home: { name: "主页", icon: "home", comp: HomePage },
    library: { name: "投影库", icon: "gallery", comp: LibraryPage },
    properties: { name: "属性", icon: "properties", comp: PropertiesPage },
    statistics: { name: "统计", icon: "statistics", comp: StatisticsPage },
    flake: { name: "分层", icon: "flake", comp: FlakePage },
    render: { name: "渲染", icon: "render", comp: RenderPage },
    replace: { name: "替换", icon: "replace", comp: ReplacePage },
    generate: { name: "生成", icon: "ui_debug", comp: GeneratePage },
  };

  const bottomPages: Record<string, { name: string; icon: string; comp: React.FC<any> }> = {
    settings: { name: "选项", icon: "options", comp: SettingsPage },
  };

  const allPages = { ...topPages, ...bottomPages };
  const Page = allPages[route]?.comp || HomePage;

  return (
    <div className={`app-container nova-desktop-spec ${themeClassName(theme)}`}>
      <div className={`sidebar ${expanded ? "expanded" : "collapsed"} ${expanded ? "" : "sidebar-collapsed"}`}>
        <div className="hamburger-area">
          <button
            className="nav-expand"
            type="button"
            onClick={() => setExpanded(!expanded)}
            title={expanded ? "收起侧栏" : "展开侧栏"}
          >
            {expanded ? "‹" : "›"}
          </button>
        </div>

        <div className="sidebar-item-container sidebar-main">
          {Object.entries(topPages).map(([key, value]) => {
            const active = route === key;
            return (
              <button
                key={key}
                className={`sidebar-item nav-item ${active ? "active nav-item-active" : ""}`}
                onClick={() => setRoute(key)}
                title={!expanded ? value.name : ""}
                type="button"
              >
                <div className="sidebar-icon">
                  <NavIcon name={value.icon} />
                </div>
                {expanded && <div className="sidebar-text nav-label">{value.name}</div>}
              </button>
            );
          })}
        </div>

        <div className="sidebar-item-container sidebar-bottom">
          {Object.entries(bottomPages).map(([key, value]) => {
            const active = route === key;
            return (
              <button
                key={key}
                className={`sidebar-item nav-item ${active ? "active nav-item-active" : ""}`}
                onClick={() => setRoute(key)}
                title={!expanded ? value.name : ""}
                type="button"
              >
                <div className="sidebar-icon">
                  <NavIcon name={value.icon} />
                </div>
                {expanded && <div className="sidebar-text nav-label">{value.name}</div>}
              </button>
            );
          })}
        </div>
      </div>
      <div className="main-content">
        <div className="page-content">
          <Page currentFile={currentFile} setCurrentFile={setCurrentFile} setRoute={setRoute} activeRoute={route} theme={theme} setTheme={setTheme} />
        </div>
      </div>
    </div>
  );
}

