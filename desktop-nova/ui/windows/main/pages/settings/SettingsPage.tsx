import { displayPathText } from "../../../../../src/business/facade";
import React, { useEffect, useState } from "react";
import { Sun, Moon, Monitor, Check } from "lucide-react";
import { Button } from "../../../../components/ui/button";
import { Input } from "../../../../components/ui/input";
import { Checkbox } from "../../../../components/ui/checkbox";
import { RadioGroup, RadioGroupItem } from "../../../../components/ui/radio-group";
import { Textarea } from "../../../../components/ui/textarea";
import { Dropdown } from "../../../../components/Dropdown";
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
import { queryBlockDatabase } from "../../../../../src/business/projectionLibrary";
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

const BLOCKSTATE_DB = "data/minecraft_blockstates/26.1.json";
const BLOCKSTATE_ZH = "data/minecraft_blockstates/26.1.zh_cn.json";
const BLOCK_ICON_DIR = "block";
/* One entry per theme, from `src/services/themeManifest.ts`. Do not add ids here:
   a theme missing from the manifest cannot be loaded or stored, and the registry
   test fails loudly instead of the picker quietly showing a dead option. */
const NOVA_THEME_OPTIONS = THEMES.map((theme) => ({ value: theme.id, label: theme.label }));
const MATERIAL_LIST_WINDOW_BEHAVIOR_OPTIONS = [
  { value: "independent_window", label: "独立窗口" },
  { value: "main_window_overlay", label: "主窗口遮罩" },
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
  if (ok === null) return "未检查";
  return ok ? "OK" : "失败";
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
  return (
    <div className="form-row">
      <div className="form-label">{label}</div>
      <div className="form-field">
        <Input aria-label={label} className="input settings-grow" value={displayFilePath(info?.normalized || path)} readOnly />
        <span className="settings-path-state" data-state={info ? (info.exists ? "ok" : "error") : "unchecked"}>
          {info ? (info.exists ? "存在" : "缺失") : "未检查"}
        </span>
      </div>
    </div>
  );
}

export function SettingsPage({ theme, setTheme, colorMode, setColorMode, setDebugMode, buildVersion }: any) {
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
  const [log, setLog] = useState("");
  const [exportingLogs, setExportingLogs] = useState(false);
  const [logExportStatus, setLogExportStatus] = useState("");
  const [aiConfig, setAiConfig] = useState<AiPublicConfig | null>(null);
  const [aiProvider, setAiProvider] = useState("mock");
  const [aiBaseUrl, setAiBaseUrl] = useState("https://api.openai.com/v1");
  const [aiModel, setAiModel] = useState("gpt-4.1-mini");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [apiKeyDirty, setApiKeyDirty] = useState(false);
  const [showApiKey, setShowApiKey] = useState(false);
  const [aiStatus, setAiStatus] = useState("未保存");
  const [aiSaveMessage, setAiSaveMessage] = useState("");
  const [aiTestStatus, setAiTestStatus] = useState("");
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
    setAiStatus(config.has_key ? "已保存" : config.key_status === "cleared" ? "已清除" : "未保存");
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
      setLog(`调试模式保存失败：${String(reason)}`);
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
      setLog("Viewer 时间 / 天气修改权限已保存；下一次启动 Viewer 时生效。");
    } catch (err) {
      setViewerControls(previous);
      setLog(`Viewer 修改权限保存失败：${String(err)}`);
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
      setLog("渲染设置已保存，下次打开预览时生效。");
    } catch (err) {
      setLog(`渲染设置保存失败：${String(err)}`);
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
      setLog("图形 API 已保存；仅对新的渲染任务生效，现有预览保持不变。");
    } catch (err) {
      setGraphicsBackend(previous);
      setLog(`图形 API 保存失败：${String(err)}`);
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
        ...(directory ? {} : { filters: [{ name: "Minecraft 光影包", extensions: ["zip"] }] }),
      });
      const path = selectedPaths(selection)[0];
      if (!path) return;
      const parts = splitPackPath(path);
      if (!parts?.root || !parts.name) {
        setLog(`无法从所选路径识别光影包根目录和名称：${displayFilePath(path)}`);
        return;
      }
      setV5Compatibility((current) => ({
        ...current,
        backend: current.backend === "vulkan" ? "iris-gl46" : current.backend,
        shaderpack_root: parts.root,
        shaderpack_name: parts.name,
      }));
      setLog(`已选择光影包：${displayFilePath(path)}\n点击“保存渲染设置”后生效。`);
    } catch (err) {
      setLog(`选择光影包失败：${String(err)}`);
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
    setLog("已清除光影包草稿；点击“保存渲染设置”后生效。");
  };

  const addResourcePacks = async (directory: boolean) => {
    if (v5PickerBusy) return;
    setV5PickerBusy(true);
    try {
      const selection = await openDialog({
        directory,
        multiple: true,
        ...(directory ? {} : { filters: [{ name: "Minecraft 资源包", extensions: ["zip"] }] }),
      });
      const paths = selectedPaths(selection);
      if (paths.length === 0) return;
      setV5Compatibility((current) => ({
        ...current,
        resource_packs: appendPackPaths(current.resource_packs, paths),
      }));
      setLog(`已添加 ${paths.length} 个资源包；新选择的项目位于最高优先级。\n点击“保存渲染设置”后生效。`);
    } catch (err) {
      setLog(`选择资源包失败：${String(err)}`);
    } finally {
      setV5PickerBusy(false);
    }
  };

  const checkApplicationRuntime = async () => {
    try {
      const result = await checkApplicationRuntimeHealth();
      setRuntimeOk(result.ok);
      setLog(
        `检查应用运行能力：${result.ok ? "OK" : "失败"}`
        + `\n\n核心操作：${result.core.available ? "可用" : "不可用"}\n${result.core.message.slice(0, 1200)}`
        + `\n\n预览能力：${result.viewer.available ? "可用" : "不可用"}\n${result.viewer.message.slice(0, 1200)}`,
      );
    } catch (err: any) {
      setRuntimeOk(false);
      setLog(String(err));
    }
  };

  const checkDatabase = async () => {
    try {
      const result = await queryBlockDatabase({});
      if (!result.versions?.length) throw new Error("方块数据未加载，请重试。");
      setDbOk(true);
      setLog(`方块数据正常：${result.versions.length} 个版本`);
    } catch (error) { setDbOk(false); setLog(`方块数据检查失败：${String(error)}`); }
  };

  const checkIcons = async () => {
    try {
      const [iconDir, dirt, grass] = await Promise.all([
        getPathInfo(BLOCK_ICON_DIR),
        getPathInfo("block/dirt.png"),
        getPathInfo("block/grass_block.png"),
      ]);
      setIconDirInfo(iconDir);
      const ok = iconDir.exists && iconDir.is_dir && dirt.exists && grass.exists;
      setIconOk(ok);
      setLog(`检查图标库：${ok ? "OK" : "失败"}\n目录：${displayFilePath(iconDir.normalized)}\ndirt.png=${dirt.exists}\ngrass_block.png=${grass.exists}`);
    } catch (err: any) {
      setIconOk(false);
      setLog(String(err));
    }
  };

  const chooseConfigDir = async () => {
    const selected = await chooseUserConfigDir();
    if (!selected) return;
    applyUserConfig(await setUserConfigDir(selected, false), true);
    setLog(`用户配置目录已切换：${selected}`);
  };

  const migrateConfigDir = async () => {
    const selected = await chooseUserConfigDir();
    if (!selected) return;
    applyUserConfig(await setUserConfigDir(selected, true), true);
    setLog(`用户配置已迁移到：${selected}`);
  };

  const resetConfigDir = async () => {
    applyUserConfig(await resetUserConfigDir(true), true);
    setLog("用户配置目录已恢复默认位置，并迁移当前配置。");
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
    setAiStatus(saved.has_key ? "已保存" : "未保存");
    setAiSaveMessage(typedKey ? "已保存 Key" : saved.has_key ? "保留已保存 Key" : "未保存 Key");
    return saved;
  };

  const saveAi = async () => {
    const saved = await saveAiConfigOnly();
    setAiTestStatus("");
    setLog(`AI 配置已保存。provider=${saved.provider}, model=${saved.model}`);
  };

  const clearAiKey = async () => {
    const next = await aiClearKey();
    setApiKeyInput("");
    setApiKeyDirty(false);
    setAiConfig(next);
    setAiStatus("已清除");
    setAiSaveMessage("已清除 Key");
    setAiTestStatus("");
    setLog("AI API Key 已清除。");
  };

  const testAi = async () => {
    setAiTestStatus("测试中...");
    try {
      const saved = await saveAiConfigOnly();
      const result = await aiTestConnection();
      setAiTestStatus(result.ok ? `测试成功：${result.message}` : `测试失败：${result.message}\nprovider=${saved.provider}\nbase_url=${saved.base_url}\nmodel=${saved.model}`);
    } catch (err: any) {
      setAiTestStatus(`测试失败：${String(err)}\nprovider=${aiProvider}\nbase_url=${aiBaseUrl}\nmodel=${aiModel}`);
    }
  };

  return (
    <>
    <ScrollArea className="settings-scroll-area" aria-label="设置">
    <div className="settings-page">
      <h2 className="settings-title">选项</h2>

      <div className="group-box">
        <div className="group-box-title">外观</div>
        <div className="form-row">
          <div className="form-label">主题</div>
          <div className="form-field">
            <RadioGroup className="settings-theme-options" aria-label="主题"
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
          <div className="form-label">明暗模式</div>
          <RadioGroup className="settings-color-options" aria-label="明暗模式" orientation="horizontal"
            value={normalizeColorMode(colorMode)} onValueChange={setColorMode}>
            {[{ value: "light", label: "浅色", Icon: Sun }, { value: "dark", label: "深色", Icon: Moon }, { value: "system", label: "跟随系统", Icon: Monitor }].map(({ value, label, Icon }) => (
              <RadioGroupItem key={value} value={value}><Icon size={16} aria-hidden="true" />{label}</RadioGroupItem>
            ))}
          </RadioGroup>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">用户配置目录</div>
        <div className="form-row">
          <div className="form-label">当前目录</div>
          <Input className="input settings-grow" aria-label="当前配置目录" value={displayFilePath(userConfig?.config_dir || "")} readOnly />
        </div>
        <div className="form-row">
          <div className="form-label">默认目录</div>
          <Input className="input settings-grow" aria-label="默认配置目录" value={displayFilePath(userConfig?.default_config_dir || "")} readOnly />
        </div>
        <div className="settings-actions">
          <Button onClick={chooseConfigDir}>选择目录</Button>
          <Button onClick={() => openUserConfigDir()}>打开目录</Button>
          <Button onClick={resetConfigDir}>恢复默认目录</Button>
          <Button onClick={migrateConfigDir}>迁移当前配置到新目录</Button>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">AI 设置</div>
        <div className="form-row">
          <div className="form-label">Provider</div>
          <Dropdown ariaLabel="AI Provider" value={aiProvider} onChange={setAiProvider}
            options={[{ value: "mock", label: "Mock" }, { value: "openai_compatible", label: "OpenAI Compatible" }, { value: "gemini_compatible", label: "Gemini Compatible（占位）" }]} />
          <span className="settings-muted" title={aiConfig ? `provider=${aiConfig.provider}` : undefined}>
            Key 状态：{apiKeyDirty ? "本次输入未保存" : aiStatus}
          </span>
        </div>
        <div className="form-row">
          <div className="form-label">Base URL</div>
          <Input className="input settings-grow" aria-label="Base URL" value={aiBaseUrl} onChange={(event) => setAiBaseUrl(event.target.value)} placeholder="https://api.openai.com/v1" />
        </div>
        <div className="form-row">
          <div className="form-label">Model</div>
          <Input className="input settings-model-input" aria-label="AI Model" value={aiModel} onChange={(event) => setAiModel(event.target.value)} placeholder="gpt-4.1-mini" />
        </div>
        <div className="form-row">
          <div className="form-label">API Key</div>
          <div className="form-field">
            <Input
              className="input settings-grow"
              aria-label="API Key"
              type={showApiKey ? "text" : "password"}
              value={apiKeyInput}
              onChange={(event) => {
                setApiKeyInput(event.target.value);
                setApiKeyDirty(true);
                setAiSaveMessage("");
              }}
              placeholder={aiConfig?.has_key ? "已保存；输入新 Key 可替换" : "未保存"}
              autoComplete="off"
            />
            <Button type="button" onClick={() => setShowApiKey((value) => !value)}>
              {showApiKey ? "隐藏" : "显示"}
            </Button>
          </div>
        </div>
        <div className="settings-actions">
          <Button onClick={saveAi}>保存 Key</Button>
          <Button onClick={clearAiKey}>清除 Key</Button>
          <Button onClick={testAi}>测试连接</Button>
        </div>
        {(aiSaveMessage || aiTestStatus) && (
          <pre className="settings-feedback" data-state={aiTestStatus.startsWith("测试失败") ? "error" : "normal"}>
            {[aiSaveMessage, aiTestStatus].filter(Boolean).join("\n")}
          </pre>
        )}
      </div>

      <div className="group-box">
        <div className="group-box-title">界面行为</div>
        <div className="form-row">
          <div className="form-label">子窗口行为</div>
          <div className="form-field">
            <Dropdown ariaLabel="子窗口行为" value={materialListWindowBehavior}
              options={MATERIAL_LIST_WINDOW_BEHAVIOR_OPTIONS} onChange={handleMaterialListWindowBehavior} />
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">调试模式</div>
          <label className="form-field">
            <Checkbox aria-label="调试模式" checked={debugMode} disabled={!userConfig || debugModeSaving} onCheckedChange={(checked) => void handleDebugMode(checked === true)} />
            <span>UI 测试与枚举器</span>
          </label>
        </div>
        <div className="form-row">
          <div className="form-label">保留尾部路径数</div>
          <div className="form-field">
            <Input
              aria-label="保留尾部路径数"
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
        <div className="group-box-title">应用运行能力</div>
        <PathRow label="工作目录" path={workspaceRoot || "..."} info={workspaceRoot ? ({ normalized: workspaceRoot, exists: true } as PathInfo) : null} />
        <div className="settings-actions settings-actions-spaced">
          <Button onClick={checkApplicationRuntime}>检查运行能力</Button>
          <span className="settings-status" data-state={runtimeOk === false ? "error" : "normal"}>{statusText(runtimeOk)}</span>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">数据文件</div>
        <div className="form-row"><div className="form-label">方块数据库</div><div>内置多版本方块库</div></div>
        <PathRow label="block 图标库" path={BLOCK_ICON_DIR} info={iconDirInfo} />
        <div className="settings-actions settings-actions-spaced">
          <Button onClick={() => openAssetManagerWithWindowBehavior(() => setShowAssetManager(true)).catch((error) => setLog(String(error)))}>游戏资源管理</Button>
          <Button onClick={checkDatabase}>检查方块数据</Button>
          <Button onClick={checkIcons}>检查图标库</Button>
          <span className="settings-status" data-state={dbOk === false || iconOk === false ? "error" : "normal"}>
            方块数据: {statusText(dbOk)} / 图标: {statusText(iconOk)}
          </span>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">渲染</div>
        <div className="form-row">
          <div className="form-label">预览图生成</div>
          <div className="form-field">
            <Dropdown ariaLabel="预览图生成" value={previewMode} options={DISPLAY_MODE_OPTIONS} onChange={handlePreviewMode} />
            <Button onClick={() => handlePreviewMode("full")}>重置为完整模式</Button>
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">时间修改</div>
          <label className="form-field">
            <Checkbox
              aria-label="允许修改时间"
              checked={viewerControls.allow_time_modification}
              onCheckedChange={(checked) => saveViewerControls({
                ...viewerControls,
                allow_time_modification: checked === true,
              })}
            />
            <span>允许在 Viewer 中按 T 修改时间</span>
          </label>
        </div>
        <div className="form-row">
          <div className="form-label">天气修改</div>
          <label className="form-field">
            <Checkbox
              aria-label="允许修改天气"
              checked={viewerControls.allow_weather_modification}
              onCheckedChange={(checked) => saveViewerControls({
                ...viewerControls,
                allow_weather_modification: checked === true,
              })}
            />
            <span>允许在 Viewer 中按 Y 修改天气</span>
          </label>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">渲染兼容设置</div>
        <div className="form-row">
          <div className="form-label">图形 API</div>
          <div className="form-field">
            <Dropdown ariaLabel="图形 API" value={graphicsBackend}
              disabled={graphicsBackendSaving || !userConfig}
              onChange={handleGraphicsBackend}
              options={[
                { value: "auto", label: "自动（推荐）" },
                { value: "dx12", label: "DirectX 12（Windows 快速模式）" },
                { value: "vulkan", label: "Vulkan" },
              ]} />
            {graphicsBackendSaving && <span role="status">正在保存…</span>}
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">渲染后端</div>
          <Dropdown ariaLabel="渲染后端" value={v5Compatibility.backend}
            onChange={(value) => setV5Compatibility((current) => ({ ...current, backend: value as V5CompatibilityConfig["backend"] }))}
            options={[{ value: "auto", label: "自动" }, { value: "vulkan", label: "Vulkan" }, { value: "iris-gl46", label: "光影模式" }]} />
        </div>
        <div className="form-row">
          <div className="form-label">光影包文件夹</div>
          <Input
            className="input"
            aria-label="光影包文件夹" value={displayFilePath(v5Compatibility.shaderpack_root)}
            onChange={(event) => setV5Compatibility((current) => ({ ...current, shaderpack_root: event.target.value }))}
            placeholder="例如 D:\\.minecraft\\shaderpacks"
          />
        </div>
        <div className="form-row">
          <div className="form-label">光影包名称</div>
          <div className="form-field v5-pack-picker-row">
            <Input
              className="input"
              aria-label="光影包名称" value={v5Compatibility.shaderpack_name}
              onChange={(event) => setV5Compatibility((current) => ({ ...current, shaderpack_name: event.target.value }))}
              placeholder="根目录下的 zip 文件名或目录名"
            />
            <Button type="button" disabled={v5PickerBusy} onClick={() => chooseShaderpack(false)}>选择 ZIP</Button>
            <Button type="button" disabled={v5PickerBusy} onClick={() => chooseShaderpack(true)}>选择目录</Button>
            <Button type="button" disabled={v5PickerBusy || (!v5Compatibility.shaderpack_root && !v5Compatibility.shaderpack_name)} onClick={clearShaderpack}>清除</Button>
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">光影加载失败时</div>
          <Dropdown ariaLabel="光影加载失败时" value={v5Compatibility.iris_fallback}
            onChange={(value) => setV5Compatibility((current) => ({ ...current, iris_fallback: value as V5CompatibilityConfig["iris_fallback"] }))}
            options={[{ value: "fail", label: "直接报错" }, { value: "vulkan", label: "使用基础渲染" }]} />
        </div>
        <div className="form-row">
          <div className="form-label">Fabric mods 目录</div>
          <Input
            className="input"
            aria-label="Fabric mods 目录" value={displayFilePath(v5Compatibility.fabric_mods_dir)}
            onChange={(event) => setV5Compatibility((current) => ({ ...current, fabric_mods_dir: event.target.value }))}
            placeholder="留空禁用；仅加载 Fabric data-only 资源"
          />
        </div>
        <div className="form-row">
          <div className="form-label">资源包</div>
          <div className="v5-pack-picker-row v5-resource-pack-picker">
            <Textarea
              aria-label="资源包列表"
              className="input v5-resource-pack-list"
              value={v5Compatibility.resource_packs.map(displayFilePath).join("\n")}
              onChange={(event) => setV5Compatibility((current) => ({ ...current, resource_packs: event.target.value.split(/\r?\n/) }))}
              placeholder="每行一个 zip 或目录；按从低到高优先级填写"
            />
            <div className="v5-pack-actions">
              <Button type="button" disabled={v5PickerBusy} onClick={() => addResourcePacks(false)}>添加 ZIP</Button>
              <Button type="button" disabled={v5PickerBusy} onClick={() => addResourcePacks(true)}>添加目录</Button>
              <Button
               
                type="button"
                disabled={v5PickerBusy || v5Compatibility.resource_packs.every((path) => !path.trim())}
                onClick={() => setV5Compatibility((current) => ({ ...current, resource_packs: [] }))}
              >清空</Button>
            </div>
          </div>
        </div>
        <Button onClick={saveV5Compatibility}>保存渲染设置</Button>
      </div>

      <div className="group-box">
        <div className="group-box-title">维护</div>
        {buildVersion && <div className="form-row"><div className="form-label">应用版本</div><div>{buildVersion}</div></div>}
        <Button onClick={async () => setLog(await cleanupLocalTempFiles())}>清理本地临时文件</Button>
        <Button type="button" disabled={exportingLogs} onClick={async () => {
          setExportingLogs(true); setLogExportStatus("");
          try {
            const result = await exportDiagnosticLogs();
            if (result) setLogExportStatus(`日志已导出：${displayFilePath(result.path)}（${result.sessionIds.length} 次启动）`);
          } catch (reason) {
            setLogExportStatus(`日志导出失败：${String(reason)}`);
          } finally { setExportingLogs(false); }
        }}>{exportingLogs ? "正在导出日志…" : "导出日志 ZIP…"}</Button>
        {logExportStatus && <div className="settings-feedback" role="status" aria-live="polite">{displayPathText(logExportStatus)}</div>}
      </div>

      {log && (
        <pre className="settings-feedback">{displayPathText(log)}</pre>
      )}
    </div>
    </ScrollArea>
    {showAssetManager && <AssetManagerDialog theme={theme} onClose={() => setShowAssetManager(false)} />}
    </>
  );
}

