import React, { useEffect, useState } from "react";
import type { SyntheticEvent } from "react";
import { loadDatabases } from "../../src/business/facade";
import { initI18n } from "../../src/business/facade";
import { HomePage } from "../windows/main/pages/home/HomePage";
import { LibraryPage } from "../windows/main/pages/library/LibraryPage";
import { PropertiesPage } from "../windows/main/pages/properties/PropertiesPage";
import { StatisticsPage } from "../windows/main/pages/statistics/StatisticsPage";
import { FlakePage } from "../windows/main/pages/flake/FlakePage";
import { ReplacePage } from "../windows/main/pages/replace/ReplacePage";
import { GeneratePage } from "../windows/main/pages/generate/GeneratePage";
import { RenderPage } from "../windows/main/pages/render/RenderPage";
import { SettingsPage } from "../windows/main/pages/settings/SettingsPage";
import { UiTestPage } from "../windows/main/pages/ui-test/UiTestPage";
import { hideEmbeddedViewer, showEmbeddedViewer } from "../../src/business/facade";
import { loadUserConfigMigratingLocalStorage, normalizeShowUiTestPage, normalizeTheme, saveThemeConfig } from "../../src/business/facade";
import {
  announceThemeChange,
  applyThemeStylesheet,
  normalizeThemeId,
  themeClassName,
  themeResourceKey,
  webDefaultThemeId,
} from "./themeRuntime";

type NavIconUrls = Record<string, string>;

const iconNameFromPath = (path: string) => path.split("/").pop()?.replace(/\.[^.]+$/, "") || "";

const shellNavIconUrls = Object.entries(
  import.meta.glob<string>("./resource/icon/nav/*.{svg,png}", {
    eager: true,
    import: "default",
    query: "?url",
  }),
).reduce<NavIconUrls>((icons, [path, url]) => {
  const iconName = iconNameFromPath(path);
  if (iconName) {
    icons[iconName] = url;
  }
  return icons;
}, {});

const themedNavIconUrls = Object.entries(
  import.meta.glob<string>("../themes/*/resource/icon/nav/*.{svg,png}", {
    eager: true,
    import: "default",
    query: "?url",
  }),
).reduce<Record<string, NavIconUrls>>((themes, [path, url]) => {
  const themeKey = path.match(/\.\.\/themes\/([^/]+)\//)?.[1];
  const iconName = iconNameFromPath(path);
  if (themeKey && iconName) {
    themes[themeKey] = themes[themeKey] || {};
    themes[themeKey][iconName] = url;
  }
  return themes;
}, {});

const fallbackNavIconUrl = (iconName: string) =>
  shellNavIconUrls[iconName] ||
  shellNavIconUrls.undefined ||
  themedNavIconUrls.metro10?.[iconName] ||
  themedNavIconUrls.metro10?.undefined ||
  "";

const navIconUrl = (themeId: string, iconName: string) => {
  const normalizedThemeId = normalizeThemeId(themeId);
  const themeKey = themeResourceKey(normalizedThemeId);
  if (themeKey === themeResourceKey(webDefaultThemeId)) {
    return fallbackNavIconUrl(iconName);
  }
  return themedNavIconUrls[themeKey]?.[iconName] || fallbackNavIconUrl(iconName);
};

function handleNavIconError(event: SyntheticEvent<HTMLImageElement>, iconName: string) {
  const fallbackUrl = fallbackNavIconUrl(iconName);
  if (fallbackUrl && event.currentTarget.src !== fallbackUrl) {
    event.currentTarget.src = fallbackUrl;
  }
}

function NavIcon({ name, theme }: { name: string; theme: string }) {
  return (
    <img
      className={`nav-icon nav-icon-${name}`}
      src={navIconUrl(theme, name)}
      alt=""
      aria-hidden="true"
      onError={(event) => handleNavIconError(event, name)}
    />
  );
}

/**
 * Renders the Nova desktop shell and keeps page logic behind the UI boundary.
 */
export function App() {
  const [currentFile, setCurrentFile] = useState<string>("");
  const [theme, setThemeState] = useState(webDefaultThemeId);
  const [route, setRoute] = useState("home");
  const [expanded, setExpanded] = useState(true);
  const [showUiTestPage, setShowUiTestPage] = useState(true);

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
      .then((info) => {
        setThemeState(normalizeThemeId(normalizeTheme(info.config.theme)));
        setShowUiTestPage(normalizeShowUiTestPage(info.config.show_ui_test_page));
      })
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

  useEffect(() => {
    if (!showUiTestPage && route === "ui_test") {
      setRoute("settings");
    }
  }, [showUiTestPage, route]);

  const topPages: Record<string, { name: string; icon: string; comp: React.FC<any> }> = {
    home: { name: "主页", icon: "home", comp: HomePage },
    library: { name: "投影库", icon: "gallery", comp: LibraryPage },
    properties: { name: "属性", icon: "properties", comp: PropertiesPage },
    statistics: { name: "统计", icon: "statistics", comp: StatisticsPage },
    flake: { name: "分层", icon: "flake", comp: FlakePage },
    render: { name: "渲染", icon: "render", comp: RenderPage },
    replace: { name: "替换", icon: "replace", comp: ReplacePage },
    generate: { name: "生成", icon: "generate", comp: GeneratePage },
  };

  const bottomPages: Record<string, { name: string; icon: string; comp: React.FC<any> }> = {
    ...(showUiTestPage ? { ui_test: { name: "UI 测试", icon: "ui_debug", comp: UiTestPage } } : {}),
    settings: { name: "选项", icon: "options", comp: SettingsPage },
  };

  const allPages = { ...topPages, ...bottomPages };
  const Page = allPages[route]?.comp || HomePage;

  return (
    <div className={["app-shell", themeClassName(theme)].filter(Boolean).join(" ")}>
      <aside className={expanded ? "sidebar" : "sidebar sidebar-collapsed"}>
        <button
          className="nav-expand"
          type="button"
          onClick={() => setExpanded(!expanded)}
          title={expanded ? "收起侧栏" : "展开侧栏"}
        >
          <NavIcon name="hamburger" theme={theme} />
          {expanded ? <strong>Litematica Nova</strong> : null}
        </button>

        <nav className="nav-list" aria-label="主导航">
          {Object.entries(topPages).map(([key, value]) => {
            const active = route === key;
            return (
              <button
                key={key}
                className={`sidebar-item nav-item ${active ? "active nav-item-active" : ""}`}
                onClick={() => setRoute(key)}
                title={!expanded ? value.name : ""}
                type="button"
                aria-pressed={active}
              >
                <NavIcon name={value.icon} theme={theme} />
                {expanded && <span className="nav-label">{value.name}</span>}
              </button>
            );
          })}
        </nav>

        <nav className="nav-list nav-list-bottom" aria-label="选项">
          {Object.entries(bottomPages).map(([key, value]) => {
            const active = route === key;
            return (
              <button
                key={key}
                className={`sidebar-item nav-item ${active ? "active nav-item-active" : ""}`}
                onClick={() => setRoute(key)}
                title={!expanded ? value.name : ""}
                type="button"
                aria-pressed={active}
              >
                <NavIcon name={value.icon} theme={theme} />
                {expanded && <span className="nav-label">{value.name}</span>}
              </button>
            );
          })}
        </nav>
      </aside>
      <main className="content">
        <Page currentFile={currentFile} setCurrentFile={setCurrentFile} setRoute={setRoute} activeRoute={route} theme={theme} setTheme={setTheme} setShowUiTestPage={setShowUiTestPage} />
      </main>
    </div>
  );
}

