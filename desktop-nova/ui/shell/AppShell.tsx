import { Button } from "../components/ui/button";
import { ScrollArea } from "../components/ui/scroll-area";
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Menu, Home, Library, FileText, BarChart3, Layers, Box, Replace, Wand2, Settings, LayoutDashboard, type LucideIcon } from "lucide-react";
import { listenEvent } from "../../src/platform/events";
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
import { loadUserConfigMigratingLocalStorage, normalizeShowUiTestPage, saveThemeConfig, saveColorModeConfig } from "../../src/business/facade";
import {
  announceThemeChange,
  announceColorModeChange,
  applyThemeStylesheet,
  normalizeThemeId,
  themeClassName,
  currentThemeId,
  currentColorMode,
  subscribeToThemeChanges,
} from "./themeRuntime";
import { projectionLibraryImportedEvent } from "../windows/libraryEvents";
import { UpdateDialog } from "../windows/main/UpdateDialog";
import { readUpdateDraft, takeUpdateDraft, useUpdateDraft } from "../windows/main/updateRecovery";

const navigationIcons: Record<string, LucideIcon> = {
  hamburger: Menu, home: Home, gallery: Library, properties: FileText,
  statistics: BarChart3, flake: Layers, render: Box, replace: Replace,
  generate: Wand2, options: Settings, ui_debug: LayoutDashboard,
};

function NavIcon({ name }: { name: string }) {
  const Icon = navigationIcons[name] || Box;
  return <Icon className="nav-icon" aria-hidden="true" focusable="false" />;
}

/**
 * Renders the Nova desktop shell and keeps page logic behind the UI boundary.
 */
export function App() {
  const [currentFile, setCurrentFile] = useState<string>(() => readUpdateDraft<{currentFile: string}>("shell")?.currentFile || "");
  const [theme, setThemeState] = useState(currentThemeId);
  const themeRef = useRef(theme);
  const [colorMode, setColorModeState] = useState(currentColorMode);
  const colorModeRef = useRef(colorMode);
  const themeSaveQueue = useRef<Promise<unknown>>(Promise.resolve());
  const [themeSaveError, setThemeSaveError] = useState("");
  const [route, setRoute] = useState(() => readUpdateDraft<{route: string}>("shell")?.route || "home");
  useUpdateDraft("shell", { currentFile, route });
  useEffect(() => { takeUpdateDraft("shell"); }, []);
  const [expanded, setExpanded] = useState(true);
  const [showUiTestPage, setShowUiTestPage] = useState(true);

  useLayoutEffect(() => {
    applyThemeStylesheet(theme);
  }, [theme]);

  useEffect(() => {
    const acceptTheme = (next: ReturnType<typeof currentThemeId>) => {
      if (themeRef.current === next) return;
      themeRef.current = next;
      setThemeState(next);
      // Only the main shell writes native config. Serialize rapid switches so
      // a slow earlier save cannot overwrite the final selection on disk.
      themeSaveQueue.current = themeSaveQueue.current
        .then(() => saveThemeConfig(next))
        .then(() => setThemeSaveError(""))
        .catch((error) => setThemeSaveError(`主题已切换，但保存失败：${String(error)}`));
    };
    const acceptColorMode = (next: ReturnType<typeof currentColorMode>) => {
      if (colorModeRef.current === next) return;
      colorModeRef.current = next;
      setColorModeState(next);
      themeSaveQueue.current = themeSaveQueue.current
        .then(() => saveColorModeConfig(next))
        .then(() => setThemeSaveError(""))
        .catch((error) => setThemeSaveError(`外观已切换，但保存失败：${String(error)}`));
    };
    const unsubscribe = subscribeToThemeChanges(acceptTheme, acceptColorMode);
    acceptTheme(currentThemeId());
    acceptColorMode(currentColorMode());
    return unsubscribe;
  }, []);

  const setTheme = (value: string) => {
    announceThemeChange(value);
  };

  useEffect(() => {
    loadDatabases();
    initI18n();
    loadUserConfigMigratingLocalStorage()
      .then((info) => {
        setShowUiTestPage(normalizeShowUiTestPage(info.config.show_ui_test_page));
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    const unlistenPromise = listenEvent<{ path?: string }>(projectionLibraryImportedEvent, (event) => {
      if (event.payload?.path) {
        setCurrentFile(event.payload.path);
      }
    }).catch(() => undefined);
    return () => {
      unlistenPromise.then((unlisten) => unlisten?.());
    };
  }, []);

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
        <Button variant="ghost"
          className="nav-expand"
          type="button"
          onClick={() => setExpanded(!expanded)}
          title={expanded ? "收起侧栏" : "展开侧栏"}
        >
          <NavIcon name="hamburger" />
          {expanded ? <strong>Litematica-Nova</strong> : null}
        </Button>

        <ScrollArea className="nav-scroll-area"><nav className="nav-list" aria-label="主导航">
          {Object.entries(topPages).map(([key, value]) => {
            const active = route === key;
            return (
              <Button variant="ghost"
                key={key}
                className={`sidebar-item nav-item ${active ? "active nav-item-active" : ""}`}
                onClick={() => setRoute(key)}
                title={!expanded ? value.name : ""}
                type="button"
                aria-pressed={active}
                aria-label={value.name}
              >
                <NavIcon name={value.icon} />
                {expanded && <span className="nav-label">{value.name}</span>}
              </Button>
            );
          })}
        </nav></ScrollArea>

        <nav className="nav-list nav-list-bottom" aria-label="选项">
          <UpdateDialog />
          {Object.entries(bottomPages).map(([key, value]) => {
            const active = route === key;
            return (
              <Button variant="ghost"
                key={key}
                className={`sidebar-item nav-item ${active ? "active nav-item-active" : ""}`}
                onClick={() => setRoute(key)}
                title={!expanded ? value.name : ""}
                type="button"
                aria-pressed={active}
                aria-label={value.name}
              >
                <NavIcon name={value.icon} />
                {expanded && <span className="nav-label">{value.name}</span>}
              </Button>
            );
          })}
        </nav>
      </aside>
      <main className="content">
        {themeSaveError && <p role="alert">{themeSaveError}</p>}
        <Page currentFile={currentFile} setCurrentFile={setCurrentFile} setRoute={setRoute} activeRoute={route} theme={theme} setTheme={setTheme} colorMode={colorMode} setColorMode={announceColorModeChange} setShowUiTestPage={setShowUiTestPage} />
      </main>
    </div>
  );
}

