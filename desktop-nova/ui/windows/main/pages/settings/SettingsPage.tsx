import { t, getUiLocale, localizeMessage, message, type UiMessage } from "../../../../../src/services/i18n";
import { useI18n } from "../../../../i18n";
import { displayPathText } from "../../../../../src/business/facade";
import React, { useEffect, useState } from "react";
import { Sun, Moon, Monitor, Check } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Checkbox } from "../../../../components/ui/checkbox";
import { RadioGroup, RadioGroupItem } from "../../../../components/ui/radio-group";
import { Textarea } from "../../../../components/ui/textarea";
import { Dropdown } from "../../../../components/Dropdown";
import { ThemeIcon } from "../../../../components/ThemeIcon";
import { ScrollArea } from "../../../../components/ui/scroll-area";
import {
  AiPublicConfig,
  aiClearKey,
  aiGetConfig,
  aiSaveConfig,
  aiTestConnection,
  checkApplicationRuntimeHealth,
  chooseUserConfigDir,
  cleanupLocalTempFiles,
  exportDiagnosticLogs,
  getNovaRuntimePathInfo,
  getSelectedGameDataContext,
  readActiveGameDataResource,
  getPathInfo,
  getUserConfig,
  openUserConfigDir,
  openDialog,
  openWorkspacePath,
  PathInfo,
  readWorkspaceFile,
  resetUserConfigDir,
  saveUserConfig,
  setUserConfigDir,
  UserConfigInfo,
  V5CompatibilityConfig,
  ViewerControlConfig,
  GraphicsApiPreference,
} from "../../../../../src/business/facade";
import { readGameDataIcon } from "../../../../../src/platform/files";
import { DISPLAY_MODE_OPTIONS, DisplayMode, normalizeDisplayMode } from "../../../../../src/business/facade";
import {
  loadUserConfigMigratingLocalStorage,
  normalizeLocalLibraryTailPathCount,
  normalizeMaterialListWindowBehavior,
  normalizeDebugMode,
  normalizeTheme,
  THEMES,
  normalizeColorMode,
  normalizeGraphicsBackend,
  saveLocalLibraryTailPathCountConfig,
  saveMaterialListWindowBehaviorConfig,
  savePreviewModeConfig,
  saveDebugModeConfig,
  displayFilePath,
  saveGraphicsBackendConfig,
} from "../../../../../src/business/facade";

import { AssetManagerDialog, openAssetManagerWithWindowBehavior } from "../../../asset_manager";
import "../../../../styles/asset-manager.css";
import "./settings.css";
import { IrisSettingsPanel } from "./IrisSettingsPanel";

/* One entry per theme, from `src/services/themeManifest.ts`. Do not add ids here:
   a theme missing from the manifest cannot be loaded or stored, and the registry
   test fails loudly instead of the picker quietly showing a dead option. */
const NOVA_THEME_OPTIONS = THEMES.map((theme) => ({ value: theme.id, label: theme.label }));
const MATERIAL_LIST_WINDOW_BEHAVIOR_OPTIONS = [
  { value: "independent_window", get label() { return t("settings.independentWindow"); } },
  { value: "main_window_overlay", get label() { return t("settings.mainWindowDialog"); } },
];
const DEFAULT_V5_COMPATIBILITY: V5CompatibilityConfig = {
  backend: "auto",
  shaderpack_root: "",
  shaderpack_name: "",
  iris_fallback: "fail",
  fabric_mods_dir: "",
  resource_packs: [],
};
const DEFAULT_VIEWER_CONTROLS: ViewerControlConfig = {
  allow_time_modification: true,
  allow_weather_modification: true,
};

function statusText(ok: boolean | null) {
  if (ok === null) return t("settings.unchecked");
  return ok ? "OK" : t("settings.failed");
}

function trimPackPath(path: string): string {
  let value = path.trim();
  while (value.length > 1 && /[\\/]$/.test(value) && !/^[A-Za-z]:[\\/]$/.test(value)) {
    value = value.slice(0, -1);
  }
  return value;
}

function splitPackPath(path: string): { root: string; name: string } | null {
  const value = trimPackPath(path);
  if (!value) return null;
  const separatorIndex = Math.max(value.lastIndexOf("\\"), value.lastIndexOf("/"));
  if (separatorIndex < 0) return { root: "", name: value };
  const name = value.slice(separatorIndex + 1);
  if (!name) return null;
  let root = value.slice(0, separatorIndex);
  if (!root && value.startsWith("/")) root = "/";
  if (/^[A-Za-z]:$/.test(root)) root += value[separatorIndex];
  return { root, name };
}

function selectedPaths(value: string | string[] | null): string[] {
  if (!value) return [];
  return (Array.isArray(value) ? value : [value]).map(trimPackPath).filter(Boolean);
}

function appendPackPaths(existing: string[], selected: string[]): string[] {
  const uniqueSelected = selected.filter(
    (path, index, paths) => paths.findIndex((candidate) => candidate.replace(/\//g, "\\").toLocaleLowerCase() === path.replace(/\//g, "\\").toLocaleLowerCase()) === index,
  );
  const selectedKeys = new Set(uniqueSelected.map((path) => path.replace(/\//g, "\\").toLocaleLowerCase()));
  return [
    ...existing.map(trimPackPath).filter((path) => path && !selectedKeys.has(path.replace(/\//g, "\\").toLocaleLowerCase())),
    ...uniqueSelected,
  ];
}

function PathRow({ label, path, info }: { label: string; path: string; info?: PathInfo | null }) {
  useI18n();
  return (
    <div className="form-row">
      <div className="form-label">{label}</div>
      <div className="form-field">
        <Input aria-label={label} className="input settings-grow" value={displayFilePath(info?.normalized || path)} readOnly />
        <span className="settings-path-state" data-state={info ? (info.exists ? "ok" : "error") : "unchecked"}>
          {info ? (info.exists ? t("settings.exists") : t("settings.missing")) : t("settings.unchecked")}
        </span>
      </div>
    </div>
  );
}

export function SettingsPage({ theme, setTheme, colorMode, setColorMode, setDebugMode, buildVersion }: any) {
  const language = useI18n();
  const [showAssetManager, setShowAssetManager] = useState(false);
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [previewMode, setPreviewMode] = useState<DisplayMode>("full");
  const [materialListWindowBehavior, setMaterialListWindowBehavior] = useState("independent_window");
  const [localLibraryTailPathCount, setLocalLibraryTailPathCount] = useState(3);
  const [debugMode, setDebugModeState] = useState(false);
  const [debugModeSaving, setDebugModeSaving] = useState(false);
  const [userConfig, setUserConfig] = useState<UserConfigInfo | null>(null);
  const [dbInfo, setDbInfo] = useState<PathInfo | null>(null);
  const [zhInfo, setZhInfo] = useState<PathInfo | null>(null);
  const [iconDirInfo, setIconDirInfo] = useState<PathInfo | null>(null);
  const [runtimeOk, setRuntimeOk] = useState<boolean | null>(null);
  const [dbOk, setDbOk] = useState<boolean | null>(null);
  const [iconOk, setIconOk] = useState<boolean | null>(null);
  const [log, setLog] = useState<UiMessage>("");
  const [exportingLogs, setExportingLogs] = useState(false);
  const [logExportStatus, setLogExportStatus] = useState<UiMessage>("");
  const [aiConfig, setAiConfig] = useState<AiPublicConfig | null>(null);
  const [aiProvider, setAiProvider] = useState("mock");
  const [aiBaseUrl, setAiBaseUrl] = useState("https://api.openai.com/v1");
  const [aiModel, setAiModel] = useState("gpt-4.1-mini");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [apiKeyDirty, setApiKeyDirty] = useState(false);
  const [showApiKey, setShowApiKey] = useState(false);
  const [aiStatus, setAiStatus] = useState<UiMessage>(message("settings.notSaved"));
  const [aiSaveMessage, setAiSaveMessage] = useState<UiMessage>("");
  const [aiTestStatus, setAiTestStatus] = useState<UiMessage>("");
  const [v5Compatibility, setV5Compatibility] = useState<V5CompatibilityConfig>(DEFAULT_V5_COMPATIBILITY);
  const [viewerControls, setViewerControls] = useState<ViewerControlConfig>(DEFAULT_VIEWER_CONTROLS);
  const [graphicsBackend, setGraphicsBackend] = useState<GraphicsApiPreference>("auto");
  const [graphicsBackendSaving, setGraphicsBackendSaving] = useState(false);
  const [v5PickerBusy, setV5PickerBusy] = useState(false);

  const applyUserConfig = (info: UserConfigInfo, changedConfigDirectory = false) => {
    const showUiTest = normalizeDebugMode(info.config.debug_mode);
    setUserConfig(info);
    setPreviewMode(normalizeDisplayMode(info.config.preview_mode));
    setMaterialListWindowBehavior(normalizeMaterialListWindowBehavior(info.config.material_list_window_behavior));
    setLocalLibraryTailPathCount(normalizeLocalLibraryTailPathCount(info.config.local_library_tail_path_count));
    setDebugModeState(showUiTest);
    setDebugMode?.(showUiTest);
    if (changedConfigDirectory) setTheme(normalizeTheme(info.config.theme));
    if (changedConfigDirectory) setColorMode(normalizeColorMode(info.config.color_mode));
    setV5Compatibility(info.config.v5_compatibility || DEFAULT_V5_COMPATIBILITY);
    setViewerControls(info.config.viewer_controls || DEFAULT_VIEWER_CONTROLS);
    setGraphicsBackend(normalizeGraphicsBackend(info.config.graphics_backend));
  };

  const refreshPathInfo = async () => {
    const info = await getNovaRuntimePathInfo();
    setWorkspaceRoot(info.workspaceRoot);
    setDbInfo(info.dbInfo);
    setZhInfo(info.zhInfo);
    setIconDirInfo(info.iconDirInfo);
  };

  const refreshAiConfig = async () => {
    const config = await aiGetConfig();
    setAiConfig(config);
    setAiProvider(config.provider);
    setAiBaseUrl(config.base_url);
    setAiModel(config.model);
    setAiStatus(config.has_key ? message("settings.saved") : config.key_status === "cleared" ? message("settings.cleared") : message("settings.notSaved"));
    setApiKeyInput("");
    setApiKeyDirty(false);
  };

  useEffect(() => {
    refreshPathInfo().catch((err) => setLog(String(err)));
    refreshAiConfig().catch((err) => setLog(String(err)));
    loadUserConfigMigratingLocalStorage().then((info) => applyUserConfig(info)).catch((err) => setLog(String(err)));
  }, []);

  const handleTheme = (value: string) => setTheme(normalizeTheme(value));

  const handlePreviewMode = async (value: string) => {
    const mode = normalizeDisplayMode(value);
    setPreviewMode(mode);
    applyUserConfig(await savePreviewModeConfig(mode));
  };

  const handleMaterialListWindowBehavior = async (value: string) => {
    const behavior = normalizeMaterialListWindowBehavior(value);
    setMaterialListWindowBehavior(behavior);
    applyUserConfig(await saveMaterialListWindowBehaviorConfig(behavior));
  };

  const handleDebugMode = async (checked: boolean) => {
    if (debugModeSaving) return;
    setDebugModeSaving(true);
    try {
      applyUserConfig(await saveDebugModeConfig(checked));
    } catch (reason) {
      setLog(message("settings.debugSaveFailed", { error: String(reason) }));
    } finally { setDebugModeSaving(false); }
  };

  const handleLocalLibraryTailPathCount = async (value: string) => {
    const count = normalizeLocalLibraryTailPathCount(value);
    setLocalLibraryTailPathCount(count);
    applyUserConfig(await saveLocalLibraryTailPathCountConfig(count));
  };

  const saveViewerControls = async (next: ViewerControlConfig) => {
    const previous = viewerControls;
    setViewerControls(next);
    try {
      applyUserConfig(await saveUserConfig({ viewer_controls: next }));
      setLog(message("settings.viewerPermissionsSaved"));
    } catch (err) {
      setViewerControls(previous);
      setLog(message("settings.viewerPermissionsSaveFailed", { error: String(err) }));
    }
  };

  const saveV5Compatibility = async () => {
    try {
      const normalized: V5CompatibilityConfig = {
        ...v5Compatibility,
        shaderpack_root: v5Compatibility.shaderpack_root.trim(),
        shaderpack_name: v5Compatibility.shaderpack_name.trim(),
        fabric_mods_dir: v5Compatibility.fabric_mods_dir.trim(),
        resource_packs: v5Compatibility.resource_packs.map((value) => value.trim()).filter(Boolean),
      };
      const info = await saveUserConfig({ v5_compatibility: normalized });
      applyUserConfig(info);
      setLog(message("settings.renderSettingsSaved"));
    } catch (err) {
      setLog(message("settings.renderSettingsSaveFailed", { error: String(err) }));
    }
  };

  const handleGraphicsBackend = async (value: string) => {
    const next = normalizeGraphicsBackend(value);
    if (graphicsBackendSaving || next === graphicsBackend) return;
    const previous = graphicsBackend;
    setGraphicsBackend(next);
    setGraphicsBackendSaving(true);
    try {
      const info = await saveGraphicsBackendConfig(next);
      setUserConfig(info);
      setGraphicsBackend(normalizeGraphicsBackend(info.config.graphics_backend));
      setLog(message("settings.graphicsApiSaved"));
    } catch (err) {
      setGraphicsBackend(previous);
      setLog(message("settings.graphicsApiSaveFailed", { error: String(err) }));
    } finally {
      setGraphicsBackendSaving(false);
    }
  };

  const chooseShaderpack = async (directory: boolean) => {
    if (v5PickerBusy) return;
    setV5PickerBusy(true);
    try {
      const selection = await openDialog({
        directory,
        multiple: false,
        defaultPath: v5Compatibility.shaderpack_root || undefined,
        ...(directory ? {} : { filters: [{ name: t("settings.shaderpackFilter"), extensions: ["zip"] }] }),
      });
      const path = selectedPaths(selection)[0];
      if (!path) return;
      const parts = splitPackPath(path);
      if (!parts?.root || !parts.name) {
        setLog(message("settings.shaderpackPathInvalid", { path: displayFilePath(path) }));
        return;
      }
      setV5Compatibility((current) => ({
        ...current,
        shaderpack_root: parts.root,
        shaderpack_name: parts.name,
      }));
      setLog(message("settings.shaderpackSelected", { path: displayFilePath(path) }));
    } catch (err) {
      setLog(message("settings.shaderpackSelectionFailed", { error: String(err) }));
    } finally {
      setV5PickerBusy(false);
    }
  };

  const clearShaderpack = () => {
    setV5Compatibility((current) => ({
      ...current,
      backend: current.backend === "iris-gl46" ? "auto" : current.backend,
      shaderpack_root: "",
      shaderpack_name: "",
    }));
    setLog(message("settings.shaderpackCleared"));
  };

  const addResourcePacks = async (directory: boolean) => {
    if (v5PickerBusy) return;
    setV5PickerBusy(true);
    try {
      const selection = await openDialog({
        directory,
        multiple: true,
        ...(directory ? {} : { filters: [{ name: t("settings.resourcePackFilter"), extensions: ["zip"] }] }),
      });
      const paths = selectedPaths(selection);
      if (paths.length === 0) return;
      setV5Compatibility((current) => ({
        ...current,
        resource_packs: appendPackPaths(current.resource_packs, paths),
      }));
      setLog(message("settings.resourcePacksAdded", { count: paths.length }));
    } catch (err) {
      setLog(message("settings.resourcePackSelectionFailed", { error: String(err) }));
    } finally {
      setV5PickerBusy(false);
    }
  };

  const checkApplicationRuntime = async () => {
    try {
      const result = await checkApplicationRuntimeHealth();
      setRuntimeOk(result.ok);
      setLog(message("settings.runtimeReport", {
        overall: result.ok ? "OK" : message("settings.failed"),
        core: message(result.core.available ? "settings.available" : "settings.unavailable"),
        coreDetail: result.core.message.slice(0, 1200),
        viewer: message(result.viewer.available ? "settings.available" : "settings.unavailable"),
        viewerDetail: result.viewer.message.slice(0, 1200),
      }));
    } catch (err: any) {
      setRuntimeOk(false);
      setLog(String(err));
    }
  };

  const checkDatabase = async () => {
    try {
      const selected = await readActiveGameDataResource();
      const data = JSON.parse(selected.dbText) as { blocks?: Record<string, unknown> };
      if (!data.blocks || !Object.keys(data.blocks).length) { setDbOk(false); setLog(message("settings.blockDataNotLoaded")); return; }
      setDbOk(true);
      setLog(message("settings.blockDataOk", { count: 1 }));
    } catch (error) { setDbOk(false); setLog(message("settings.blockDataCheckFailed", { error: String(error) })); }
  };

  const checkIcons = async () => {
    try {
      const context = await getSelectedGameDataContext();
      const [iconDir, dirt, grass] = await Promise.all([
        getPathInfo(context.root),
        readGameDataIcon("minecraft:dirt"),
        readGameDataIcon("minecraft:grass_block"),
      ]);
      setIconDirInfo(iconDir);
      const ok = iconDir.exists && iconDir.is_dir && !!dirt && !!grass;
      setIconOk(ok);
      setLog(message("settings.iconsCheckResult", { status: ok ? "OK" : message("settings.failed"), directory: displayFilePath(iconDir.normalized), dirt: String(!!dirt), grass: String(!!grass) }));
    } catch (err: any) {
      setIconOk(false);
      setLog(String(err));
    }
  };

  const chooseConfigDir = async () => {
    const selected = await chooseUserConfigDir();
    if (!selected) return;
    applyUserConfig(await setUserConfigDir(selected, false), true);
    setLog(message("settings.configDirectorySwitched", { path: selected }));
  };

  const migrateConfigDir = async () => {
    const selected = await chooseUserConfigDir();
    if (!selected) return;
    applyUserConfig(await setUserConfigDir(selected, true), true);
    setLog(message("settings.configMigrated", { path: selected }));
  };

  const resetConfigDir = async () => {
    applyUserConfig(await resetUserConfigDir(true), true);
    setLog(message("settings.configReset"));
  };

  const saveAiConfigOnly = async () => {
    const typedKey = apiKeyInput.trim();
    const saved = await aiSaveConfig({
      provider: aiProvider,
      base_url: aiBaseUrl,
      model: aiModel,
      api_key: typedKey ? apiKeyInput : null,
    });
    setAiConfig(saved);
    setApiKeyDirty(false);
    setAiStatus(saved.has_key ? message("settings.saved") : message("settings.notSaved"));
    setAiSaveMessage(typedKey ? message("settings.keySaved") : saved.has_key ? message("settings.savedKeyRetained") : message("settings.keyNotSaved"));
    return saved;
  };

  const saveAi = async () => {
    const saved = await saveAiConfigOnly();
    setAiTestStatus("");
    setLog(message("settings.aiConfigSaved", { provider: saved.provider, model: saved.model }));
  };

  const clearAiKey = async () => {
    const next = await aiClearKey();
    setApiKeyInput("");
    setApiKeyDirty(false);
    setAiConfig(next);
    setAiStatus(message("settings.cleared"));
    setAiSaveMessage(message("settings.keyCleared"));
    setAiTestStatus("");
    setLog(message("settings.aiKeyCleared"));
  };

  const testAi = async () => {
    setAiTestStatus(message("settings.testingConnection"));
    try {
      const saved = await saveAiConfigOnly();
      const result = await aiTestConnection();
      setAiTestStatus(result.ok ? message("settings.connectionTestOk", { detail: result.message }) : message("settings.connectionTestFailed", { error: result.message, provider: saved.provider, baseUrl: saved.base_url, model: saved.model }));
    } catch (err: any) {
      setAiTestStatus(message("settings.connectionTestFailed", { error: String(err), provider: aiProvider, baseUrl: aiBaseUrl, model: aiModel }));
    }
  };

  return (
    <>
    <ScrollArea className="settings-scroll-area" aria-label={t("settings.settingsAria")}>
    <div className="settings-page">
      <h2 className="settings-title">{t("settings.settingsTitle")}</h2>

      <div className="group-box">
        <div className="group-box-title">{t("settings.appearance")}</div>
        <div className="form-row">
          <label className="form-label" htmlFor="nova-ui-language">{language.t("settings.language")}</label>
          <select className="input" id="nova-ui-language" value={language.preference} onChange={event => { void language.setLanguage(event.target.value as "zh-CN" | "en-US" | "system").catch(reason => setLog(message("settings.language.failed", {error: String(reason)}))); }}>
            <option value="zh-CN">{t("settings.simplifiedChinese")}</option>
            <option value="en-US">English</option>
            <option value="system">{language.t("settings.language.system")}</option>
          </select>
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.theme")}</div>
          <div className="form-field">
            <RadioGroup className="settings-theme-options" aria-label={t("settings.theme")}
              value={normalizeTheme(theme)} onValueChange={handleTheme}>
              {NOVA_THEME_OPTIONS.map((option) => (
                <RadioGroupItem key={option.value} value={option.value} className="settings-theme-option">
                  <span className="settings-theme-swatch" data-theme-preview={option.value} aria-hidden="true"><span /><span /><span /></span>
                  <span>{option.label}</span>
                  <Check data-slot="radio-choice-mark" size={16} aria-hidden="true" />
                </RadioGroupItem>
              ))}
            </RadioGroup>
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.colorMode")}</div>
          <RadioGroup className="settings-color-options" aria-label={t("settings.colorMode")} orientation="horizontal"
            value={normalizeColorMode(colorMode)} onValueChange={setColorMode}>
            {[{ value: "light", label: t("settings.light"), icon: "light", Icon: Sun }, { value: "dark", label: t("settings.dark"), icon: "dark", Icon: Moon }, { value: "system", label: t("settings.systemDefault"), icon: "system", Icon: Monitor }].map(({ value, label, icon, Icon }) => (
              <RadioGroupItem key={value} value={value}><ThemeIcon name={icon} size={16} fallback={Icon} />{label}</RadioGroupItem>
            ))}
          </RadioGroup>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">{t("settings.configDirectory")}</div>
        <div className="form-row">
          <div className="form-label">{t("settings.currentDirectory")}</div>
          <Input className="input settings-grow" aria-label={t("settings.currentConfigDirectoryAria")} value={displayFilePath(userConfig?.config_dir || "")} readOnly />
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.defaultDirectory")}</div>
          <Input className="input settings-grow" aria-label={t("settings.defaultConfigDirectoryAria")} value={displayFilePath(userConfig?.default_config_dir || "")} readOnly />
        </div>
        <div className="settings-actions">
          <Button onClick={chooseConfigDir}>{t("settings.chooseDirectory")}</Button>
          <Button onClick={() => openUserConfigDir()}>{t("settings.openDirectory")}</Button>
          <Button onClick={resetConfigDir}>{t("settings.resetDirectory")}</Button>
          <Button onClick={migrateConfigDir}>{t("settings.migrateDirectory")}</Button>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">{t("settings.aiSettings")}</div>
        <div className="form-row">
          <div className="form-label">{t("settings.aiProvider")}</div>
          <Dropdown ariaLabel={t("settings.aiProvider")} value={aiProvider} onChange={setAiProvider}
            options={[{ value: "mock", label: t("settings.mockProvider") }, { value: "openai_compatible", label: t("settings.openAiProvider") }, { value: "gemini_compatible", label: t("settings.geminiPlaceholder") }]} />
          <span className="settings-muted" title={aiConfig ? `provider=${aiConfig.provider}` : undefined}>
            {t("settings.keyStatus", { status: apiKeyDirty ? message("settings.unsavedKeyInput") : aiStatus })}
          </span>
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.aiBaseUrl")}</div>
          <Input className="input settings-grow" aria-label={t("settings.aiBaseUrl")} value={aiBaseUrl} onChange={(event) => setAiBaseUrl(event.target.value)} placeholder="https://api.openai.com/v1" />
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.aiModel")}</div>
          <Input className="input settings-model-input" aria-label={t("settings.aiModel")} value={aiModel} onChange={(event) => setAiModel(event.target.value)} placeholder="gpt-4.1-mini" />
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.aiKey")}</div>
          <div className="form-field">
            <Input
              className="input settings-grow"
              aria-label={t("settings.aiKey")}
              type={showApiKey ? "text" : "password"}
              value={apiKeyInput}
              onChange={(event) => {
                setApiKeyInput(event.target.value);
                setApiKeyDirty(true);
                setAiSaveMessage("");
              }}
              placeholder={aiConfig?.has_key ? t("settings.replaceSavedKey") : t("settings.notSaved")}
              autoComplete="off"
            />
            <Button type="button" onClick={() => setShowApiKey((value) => !value)}>
              {showApiKey ? t("settings.hide") : t("settings.show")}
            </Button>
          </div>
        </div>
        <div className="settings-actions">
          <Button onClick={saveAi}>{t("settings.saveKey")}</Button>
          <Button onClick={clearAiKey}>{t("settings.clearKey")}</Button>
          <Button onClick={testAi}>{t("settings.testConnection")}</Button>
        </div>
        {(aiSaveMessage || aiTestStatus) && (
          <pre className="settings-feedback" data-state={(typeof aiTestStatus !== "string" && aiTestStatus.key === "settings.connectionTestFailed") ? "error" : "normal"}>
            {[aiSaveMessage, aiTestStatus].filter(Boolean).map(localizeMessage).join("\n")}
          </pre>
        )}
      </div>

      <div className="group-box">
        <div className="group-box-title">{t("settings.interfaceBehavior")}</div>
        <div className="form-row">
          <div className="form-label">{t("settings.childWindowBehavior")}</div>
          <div className="form-field">
            <Dropdown ariaLabel={t("settings.childWindowBehavior")} value={materialListWindowBehavior}
              options={MATERIAL_LIST_WINDOW_BEHAVIOR_OPTIONS} onChange={handleMaterialListWindowBehavior} />
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.debugMode")}</div>
          <label className="form-field">
            <Checkbox aria-label={t("settings.debugMode")} checked={debugMode} disabled={!userConfig || debugModeSaving} onCheckedChange={(checked) => void handleDebugMode(checked === true)} />
            <span>{t("settings.uiTestEnumerator")}</span>
          </label>
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.pathTailSegments")}</div>
          <div className="form-field">
            <Input
              aria-label={t("settings.pathTailSegments")}
              className="input settings-count-input"
              type="number"
              min={1}
              step={1}
              value={localLibraryTailPathCount}
              onChange={(event) => handleLocalLibraryTailPathCount(event.target.value)}
            />
          </div>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">{t("settings.runtimeCapabilities")}</div>
        <PathRow label={t("settings.workingDirectory")} path={workspaceRoot || "..."} info={workspaceRoot ? ({ normalized: workspaceRoot, exists: true } as PathInfo) : null} />
        <div className="settings-actions settings-actions-spaced">
          <Button onClick={checkApplicationRuntime}>{t("settings.checkRuntime")}</Button>
          <span className="settings-status" data-state={runtimeOk === false ? "error" : "normal"}>{statusText(runtimeOk)}</span>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">{t("settings.dataFiles")}</div>
        <div className="form-row"><div className="form-label">{t("settings.blockDatabase")}</div><div>{t("settings.builtinBlockDatabase")}</div></div>
        <PathRow label={t("settings.blockIconLibrary")} path={iconDirInfo?.normalized || "..."} info={iconDirInfo} />
        <div className="settings-actions settings-actions-spaced">
          <Button onClick={() => openAssetManagerWithWindowBehavior(() => setShowAssetManager(true)).catch((error) => setLog(String(error)))}>{t("settings.manageGameResources")}</Button>
          <Button onClick={checkDatabase}>{t("settings.checkBlockData")}</Button>
          <Button onClick={checkIcons}>{t("settings.checkIconLibrary")}</Button>
          <span className="settings-status" data-state={dbOk === false || iconOk === false ? "error" : "normal"}>
            {t("settings.dataHealth", { blocks: statusText(dbOk), icons: statusText(iconOk) })}
          </span>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">{t("settings.rendering")}</div>
        <div className="form-row">
          <div className="form-label">{t("settings.previewGeneration")}</div>
          <div className="form-field">
            <Dropdown ariaLabel={t("settings.previewGeneration")} value={previewMode} options={DISPLAY_MODE_OPTIONS} onChange={handlePreviewMode} />
            <Button onClick={() => handlePreviewMode("full")}>{t("settings.resetFullMode")}</Button>
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.timeModification")}</div>
          <label className="form-field">
            <Checkbox
              aria-label={t("settings.allowTimeModification")}
              checked={viewerControls.allow_time_modification}
              onCheckedChange={(checked) => saveViewerControls({
                ...viewerControls,
                allow_time_modification: checked === true,
              })}
            />
            <span>{t("settings.viewerTimeHint")}</span>
          </label>
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.weatherModification")}</div>
          <label className="form-field">
            <Checkbox
              aria-label={t("settings.allowWeatherModification")}
              checked={viewerControls.allow_weather_modification}
              onCheckedChange={(checked) => saveViewerControls({
                ...viewerControls,
                allow_weather_modification: checked === true,
              })}
            />
            <span>{t("settings.viewerWeatherHint")}</span>
          </label>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">{t("settings.renderCompatibility")}</div>
        <div className="form-row">
          <div className="form-label">{t("settings.graphicsApi")}</div>
          <div className="form-field">
            <Dropdown ariaLabel={t("settings.graphicsApi")} value={graphicsBackend}
              disabled={graphicsBackendSaving || !userConfig}
              onChange={handleGraphicsBackend}
              options={[
                { value: "auto", label: t("settings.automaticRecommended") },
                { value: "dx12", label: t("settings.dx12FastWindows") },
                { value: "vulkan", label: "Vulkan" },
              ]} />
            {graphicsBackendSaving && <span role="status">{t("settings.saving")}</span>}
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.fullBackend")}</div>
          <Dropdown ariaLabel={t("settings.renderBackend")} value={v5Compatibility.backend}
            onChange={(value) => setV5Compatibility((current) => ({ ...current, backend: value as V5CompatibilityConfig["backend"] }))}
            options={[{ value: "auto", label: t("settings.automatic") }, { value: "vulkan", label: t("settings.vulkanShaderpacks") }, { value: "iris-gl46", label: t("settings.glShaderReference") }]} />
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.shaderpackFolder")}</div>
          <Input
            className="input"
            aria-label={t("settings.shaderpackFolder")} value={displayFilePath(v5Compatibility.shaderpack_root)}
            onChange={(event) => setV5Compatibility((current) => ({ ...current, shaderpack_root: event.target.value }))}
            placeholder={t("settings.shaderpackFolderHint")}
          />
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.shaderpackName")}</div>
          <div className="form-field v5-pack-picker-row">
            <Input
              className="input"
              aria-label={t("settings.shaderpackName")} value={v5Compatibility.shaderpack_name}
              onChange={(event) => setV5Compatibility((current) => ({ ...current, shaderpack_name: event.target.value }))}
              placeholder={t("settings.shaderpackNameHint")}
            />
            <Button type="button" disabled={v5PickerBusy} onClick={() => chooseShaderpack(false)}>{t("settings.chooseZip")}</Button>
            <Button type="button" disabled={v5PickerBusy} onClick={() => chooseShaderpack(true)}>{t("settings.chooseDirectory")}</Button>
            <Button type="button" disabled={v5PickerBusy || (!v5Compatibility.shaderpack_root && !v5Compatibility.shaderpack_name)} onClick={clearShaderpack}>{t("settings.clear")}</Button>
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.shaderpackFailure")}</div>
          <Dropdown ariaLabel={t("settings.shaderpackFailure")} value={v5Compatibility.iris_fallback}
            onChange={(value) => setV5Compatibility((current) => ({ ...current, iris_fallback: value as V5CompatibilityConfig["iris_fallback"] }))}
            options={[{ value: "fail", label: t("settings.failImmediately") }, { value: "vulkan", label: t("settings.useBasicRendering") }]} />
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.fabricModsDirectory")}</div>
          <Input
            className="input"
            aria-label={t("settings.fabricModsDirectory")} value={displayFilePath(v5Compatibility.fabric_mods_dir)}
            onChange={(event) => setV5Compatibility((current) => ({ ...current, fabric_mods_dir: event.target.value }))}
            placeholder={t("settings.fabricModsHint")}
          />
        </div>
        <div className="form-row">
          <div className="form-label">{t("settings.resourcePacks")}</div>
          <div className="v5-pack-picker-row v5-resource-pack-picker">
            <Textarea
              aria-label={t("settings.resourcePackList")}
              className="input v5-resource-pack-list"
              value={v5Compatibility.resource_packs.map(displayFilePath).join("\n")}
              onChange={(event) => setV5Compatibility((current) => ({ ...current, resource_packs: event.target.value.split(/\r?\n/) }))}
              placeholder={t("settings.resourcePackListHint")}
            />
            <div className="v5-pack-actions">
              <Button type="button" disabled={v5PickerBusy} onClick={() => addResourcePacks(false)}>{t("settings.addZip")}</Button>
              <Button type="button" disabled={v5PickerBusy} onClick={() => addResourcePacks(true)}>{t("settings.addDirectory")}</Button>
              <Button
               
                type="button"
                disabled={v5PickerBusy || v5Compatibility.resource_packs.every((path) => !path.trim())}
                onClick={() => setV5Compatibility((current) => ({ ...current, resource_packs: [] }))}
              >{t("settings.clearAll")}</Button>
            </div>
          </div>
        </div>
        <Button onClick={saveV5Compatibility}>{t("settings.saveRenderSettings")}</Button>
        <IrisSettingsPanel root={v5Compatibility.shaderpack_root} pack={v5Compatibility.shaderpack_name}
          onSelectPack={(name) => setV5Compatibility((current) => ({ ...current, shaderpack_name: name }))}
          onApplyPack={async (root, name) => {
            const next = { ...v5Compatibility, shaderpack_root: root, shaderpack_name: name };
            applyUserConfig(await saveUserConfig({ v5_compatibility: next }));
          }} />
      </div>

      <div className="group-box">
        <div className="group-box-title">{t("settings.maintenance")}</div>
        {buildVersion && <div className="form-row"><div className="form-label">{t("settings.appVersion")}</div><div>{buildVersion}</div></div>}
        <Button onClick={async () => setLog(await cleanupLocalTempFiles())}>{t("settings.cleanupTemp")}</Button>
        <Button type="button" disabled={exportingLogs} onClick={async () => {
          setExportingLogs(true); setLogExportStatus("");
          try {
            const result = await exportDiagnosticLogs();
            if (result) setLogExportStatus(message("settings.logsExported", { path: displayFilePath(result.path), count: result.sessionIds.length }));
          } catch (reason) {
            setLogExportStatus(message("settings.logsExportFailed", { error: String(reason) }));
          } finally { setExportingLogs(false); }
        }}>{exportingLogs ? t("settings.exportingLogs") : t("settings.exportLogsZip")}</Button>
        {logExportStatus && <div className="settings-feedback" role="status" aria-live="polite">{displayPathText(localizeMessage(logExportStatus))}</div>}
      </div>

      {log && (
        <pre className="settings-feedback">{displayPathText(localizeMessage(log))}</pre>
      )}
    </div>
    </ScrollArea>
    {showAssetManager && <AssetManagerDialog theme={theme} onClose={() => setShowAssetManager(false)} />}
    </>
  );
}

