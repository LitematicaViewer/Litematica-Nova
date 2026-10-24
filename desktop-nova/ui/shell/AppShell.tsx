import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../src/services/i18n";
import { useI18n } from "../i18n";
import { Button } from "../components/ui/button";
import { ScrollArea } from "../components/ui/scroll-area";
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { getApplicationVersion } from "../../src/platform/appData";
import { invoke } from "../../src/platform/tauri";
import { addOrUpdateRecord, loadLibrary } from "../../src/business/projectionLibrary";
import { listenEvent } from "../../src/platform/events";
import { initI18n, loadStructuralBlocks, createRuntimeProjectionCollection, displayPathText, type EnumeratorCollection } from "../../src/business/facade";
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
import { loadUserConfigMigratingLocalStorage, normalizeDebugMode, saveThemeConfig, saveColorModeConfig } from "../../src/business/facade";
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
import { FormatImportDialog } from "../windows/main/FormatImportDialog";
import { UpdateDialog } from "../windows/main/UpdateDialog";
import { readUpdateDraft, takeUpdateDraft, useUpdateDraft, useApplicationCloseRecovery } from "../windows/main/updateRecovery";
import { WorkbenchNavigation, isWorkbenchRoute } from "./WorkbenchNavigation";
import { CompositionPage } from "../windows/main/pages/composition/CompositionPage";
import { NavIcon } from "./NavIcon";

import { EnumeratorDialog, openEnumeratorWithWindowBehavior } from "../windows/enumerator";
import "../styles/enumerator.css";

/**
 * Renders the Nova desktop shell and keeps page logic behind the UI boundary.
 */
export function App() {
  useI18n();
  const closeRecovery = useApplicationCloseRecovery();
  const [currentFile, setCurrentFile] = useState<string>(() => readUpdateDraft<{currentFile: string}>("shell")?.currentFile || "");
  const [theme, setThemeState] = useState(currentThemeId);
  const themeRef = useRef(theme);
  const [colorMode, setColorModeState] = useState(currentColorMode);
  const colorModeRef = useRef(colorMode);
  const themeSaveQueue = useRef<Promise<unknown>>(Promise.resolve());
  const [themeSaveError, setThemeSaveError] = useState<UiMessage>("");
  const [route, setRoute] = useState(() => readUpdateDraft<{route: string}>("shell")?.route || "home");
  const [projectDirty, setProjectDirty] = useState(false);
  const [formatImportOpen, setFormatImportOpen] = useState(false);
  const [clipboardImportOpen, setClipboardImportOpen] = useState(false);
  const [compositionId, setCompositionId] = useState("");
  const lastWorkbenchRoute = useRef("properties");
  if (isWorkbenchRoute(route)) lastWorkbenchRoute.current = route;
  const workbench = isWorkbenchRoute(route);
  useEffect(() => { setProjectDirty(false); }, [currentFile]);
  useUpdateDraft("shell", { currentFile, route });
  useEffect(() => { takeUpdateDraft("shell"); }, []);
  useEffect(() => {
    let active = true;
    let imports = Promise.resolve();
    const receive = async () => {
      if (!active) return;
      const paths = await invoke<string[]>("application_take_open_files");
      if (active && paths.length) {
        setCurrentFile(paths[paths.length - 1]); setRoute("properties");
        // Keep every requested file in the library; the latest request becomes
        // the single active projection without silently dropping earlier ones.
        imports = imports.catch(console.error).then(async () => {
          let library = await loadLibrary();
          for (const path of paths) { library = await addOrUpdateRecord(library, path); }
        });
        void imports.catch(console.error);
      }
    };
    const subscribed = listenEvent("nova-open-projection", () => { void receive().catch(console.error); });
    void subscribed.then(receive).catch(console.error);
    return () => { active = false; void subscribed.then((off) => off()); };
  }, []);
  const [showEnumerator, setShowEnumerator] = useState(false);
  const [enumeratorRuntime, setEnumeratorRuntime] = useState<EnumeratorCollection | null>(null);
  const [enumeratorError, setEnumeratorError] = useState("");
  useEffect(() => {
    let cancelled = false;
    setEnumeratorRuntime(null); setEnumeratorError("");
    if (showEnumerator && currentFile) {
      void loadStructuralBlocks(currentFile).then((blocks) => {
        if (!cancelled) setEnumeratorRuntime(createRuntimeProjectionCollection(currentFile, blocks));
      }).catch((error) => { if (!cancelled) setEnumeratorError(String(error)); });
    }
    return () => { cancelled = true; };
  }, [showEnumerator, currentFile]);
  const [expanded, setExpanded] = useState(true);
  const [debugMode, setDebugMode] = useState(false);
  const [buildVersion, setBuildVersion] = useState("");
  useEffect(() => { void getApplicationVersion().then(setBuildVersion).catch(console.error); }, []);

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
        .catch((error) => setThemeSaveError(message("shell.themeSaveFailed", { error: String(error) })));
    };
    const acceptColorMode = (next: ReturnType<typeof currentColorMode>) => {
      if (colorModeRef.current === next) return;
      colorModeRef.current = next;
      setColorModeState(next);
      themeSaveQueue.current = themeSaveQueue.current
        .then(() => saveColorModeConfig(next))
        .then(() => setThemeSaveError(""))
        .catch((error) => setThemeSaveError(message("shell.appearanceSaveFailed", { error: String(error) })));
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
    initI18n();
    loadUserConfigMigratingLocalStorage()
      .then((info) => {
        setDebugMode(normalizeDebugMode(info.config.debug_mode));
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
    if (!debugMode) setShowEnumerator(false);
    if (!debugMode && route === "ui_test") {
      setRoute("settings");
    }
  }, [debugMode, route]);

  useEffect(() => {
    const listening = listenEvent<{ enabled: boolean }>("nova-debug-mode-changed", event => {
      setDebugMode(normalizeDebugMode(event.payload?.enabled));
    });
    return () => { void listening.then(off => off()); };
  }, []);

  const topPages: Record<string, { name: string; icon: string; comp: React.FC<any> }> = {
    home: { name: t("shell.home"), icon: "home", comp: HomePage },
    library: { name: t("shell.library"), icon: "gallery", comp: LibraryPage },
    properties: { name: t("shell.properties"), icon: "properties", comp: PropertiesPage },
    statistics: { name: t("shell.statistics"), icon: "statistics", comp: StatisticsPage },
    flake: { name: t("shell.layers"), icon: "flake", comp: FlakePage },
    render: { name: t("shell.render"), icon: "render", comp: RenderPage },
    replace: { name: t("shell.replace"), icon: "replace", comp: ReplacePage },
    composition: { name: getUiLocale() === "en-US" ? "Compose / Split" : "组合/拆分", icon: "properties", comp: CompositionPage },
    generate: { name: t("shell.generate"), icon: "generate", comp: GeneratePage },
  };

  const bottomPages: Record<string, { name: string; icon: string; comp: React.FC<any> }> = {
    ...(debugMode ? { ui_test: { name: t("shell.uiTest"), icon: "ui_debug", comp: UiTestPage } } : {}),
    settings: { name: t("shell.settings"), icon: "options", comp: SettingsPage },
  };

  const navigationPages = {
    home: topPages.home, library: topPages.library,
    workbench: { name: getUiLocale() === "en-US" ? "Workbench" : "工作台", icon: "render" },
    generate: topPages.generate,
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
          title={expanded ? t("shell.collapseSidebar") : t("shell.expandSidebar")}
        >
          <NavIcon name="hamburger" theme={theme} />
          <strong>Litematica-Nova</strong>
        </Button>

        <ScrollArea className="nav-scroll-area"><nav className="nav-list" aria-label={t("shell.navigation")}>
          {Object.entries(navigationPages).map(([key, value]) => {
            const active = key === "workbench" ? workbench : route === key;
            return (
              <Button variant="ghost"
                key={key}
                className={`sidebar-item nav-item ${active ? "active nav-item-active" : ""}`}
                onClick={() => setRoute(key === "workbench" ? lastWorkbenchRoute.current : key)}
                title={!expanded ? value.name : ""}
                type="button"
                aria-pressed={active}
                aria-label={value.name}
              >
                <NavIcon name={value.icon} theme={theme} />
                <span className="nav-label">{value.name}</span>
              </Button>
            );
          })}
        </nav></ScrollArea>

        {currentFile && <div className="workspace-project" title={currentFile}>
          <span>{getUiLocale() === "en-US" ? "Current project" : "当前项目"}</span>
          <strong>{currentFile.split(/[\\/]/).pop()}</strong>
          {projectDirty && <span>{t("properties.unsaved")}</span>}
        </div>}
        <nav className="nav-list nav-list-bottom" aria-label={t("shell.settings")}>
          {debugMode && <Button variant="ghost" className="sidebar-item nav-item" type="button" aria-label={t("shell.enumerator")} title={!expanded ? t("shell.enumerator") : ""}
            onClick={() => openEnumeratorWithWindowBehavior(currentFile, () => setShowEnumerator(true)).catch((error) => setEnumeratorError(String(error)))}>
            <NavIcon name="enumerator" theme={theme} /><span className="nav-label">{t("shell.enumerator")}</span>
          </Button>}
          <UpdateDialog theme={theme} />
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
                <NavIcon name={value.icon} theme={theme} />
                <span className="nav-label">{value.name}</span>
              </Button>
            );
          })}
        </nav>
      </aside>
      <main className="content">
        {closeRecovery.error && <div role="alert"><p>{localizeMessage(closeRecovery.error)}</p><Button onClick={() => void closeRecovery.retry()}>{t("shell.retryClose")}</Button></div>}
        {themeSaveError && <p role="alert">{localizeMessage(themeSaveError)}</p>}
        {enumeratorError && <p role="alert">{t("shell.enumeratorContext")}{displayPathText(localizeMessage(enumeratorError))}</p>}
        {workbench && <WorkbenchNavigation route={route} onNavigate={setRoute} />}
        <div className="workspace-page">
        <Page onImportClipboard={() => setClipboardImportOpen(true)} initialProjectId={compositionId} setCompositionId={setCompositionId} onDirtyChange={setProjectDirty} presentationBlocked={showEnumerator || formatImportOpen} currentFile={currentFile} setCurrentFile={setCurrentFile} setRoute={setRoute} activeRoute={route} theme={theme} setTheme={setTheme} colorMode={colorMode} setColorMode={announceColorModeChange} debugMode={debugMode} setDebugMode={setDebugMode} buildVersion={buildVersion} />
        </div>
      </main>
      {clipboardImportOpen ? <FormatImportDialog clipboardMode onVisibilityChange={setFormatImportOpen} onReady={path => { setCurrentFile(path); setRoute("properties"); setClipboardImportOpen(false); }} onCancel={() => setClipboardImportOpen(false)} /> : currentFile && <FormatImportDialog onVisibilityChange={setFormatImportOpen} sourcePath={currentFile} onReady={setCurrentFile} onCancel={() => { setCurrentFile(""); setRoute("library"); }} />}
      {showEnumerator && <EnumeratorDialog theme={theme} currentFile={currentFile} runtimeCollection={enumeratorRuntime} onClose={() => setShowEnumerator(false)} />}
    </div>
  );
}

