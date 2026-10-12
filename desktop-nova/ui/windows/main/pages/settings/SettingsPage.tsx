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
  getNovaRuntimePathInfo,
  getPathInfo,
  getUserConfig,
  openUserConfigDir,
  openWorkspacePath,
  PathInfo,
  readWorkspaceFile,
  resetUserConfigDir,
  setUserConfigDir,
  UserConfigInfo,
} from "../../../../../src/business/facade";
import { DISPLAY_MODE_OPTIONS, DisplayMode, normalizeDisplayMode } from "../../../../../src/business/facade";
import {
  loadUserConfigMigratingLocalStorage,
  normalizeLocalLibraryTailPathCount,
  normalizeMaterialListWindowBehavior,
  normalizeShowUiTestPage,
  normalizeTheme,
  normalizeColorMode,
  saveLocalLibraryTailPathCountConfig,
  saveMaterialListWindowBehaviorConfig,
  savePreviewModeConfig,
  saveShowUiTestPageConfig,
} from "../../../../../src/business/facade";

import "./settings.css";

const BLOCKSTATE_DB = "data/minecraft_blockstates/26.1.json";
const BLOCKSTATE_ZH = "data/minecraft_blockstates/26.1.zh_cn.json";
const BLOCK_ICON_DIR = "block";
const NOVA_THEME_OPTIONS = [
  { value: "shadcn", label: "shadcn/ui" },
  { value: "8bitcn", label: "8bitcn/ui" },
  { value: "Minecraft", label: "Minecraft" },
];
const MATERIAL_LIST_WINDOW_BEHAVIOR_OPTIONS = [
  { value: "independent_window", label: "独立窗口" },
  { value: "main_window_overlay", label: "主窗口遮罩" },
];
function statusText(ok: boolean | null) {
  if (ok === null) return "未检查";
  return ok ? "OK" : "失败";
}

function PathRow({ label, path, info }: { label: string; path: string; info?: PathInfo | null }) {
  return (
    <div className="form-row">
      <div className="form-label">{label}</div>
      <div className="form-field">
        <Input aria-label={label} className="input settings-grow" value={info?.normalized || path} readOnly />
        <span className="settings-path-state" data-state={info ? (info.exists ? "ok" : "error") : "unchecked"}>
          {info ? (info.exists ? "存在" : "缺失") : "未检查"}
        </span>
      </div>
    </div>
  );
}

function compactFolderPathDemo(path: string, tailCount: number): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  const separator = normalized.includes("\\") ? "\\" : "/";
  const parts = normalized.split(/[\\/]+/).filter(Boolean);
  const safeTailCount = normalizeLocalLibraryTailPathCount(tailCount);
  const hasWindowsDrive = /^[A-Za-z]:/.test(normalized);
  const hasUnixRoot = normalized.startsWith("/");
  const visiblePrefixSegments = hasWindowsDrive || !hasUnixRoot ? 1 : 0;
  if (parts.length <= safeTailCount + visiblePrefixSegments) return normalized;
  const driveOrRoot = hasWindowsDrive ? normalized.match(/^[A-Za-z]:/)?.[0] || "" : hasUnixRoot ? "" : parts[0];
  const tail = parts.slice(-safeTailCount).join(separator);
  return driveOrRoot ? `${driveOrRoot}${separator}...${separator}${tail}` : `${separator}...${separator}${tail}`;
}

export function SettingsPage({ theme, setTheme, colorMode, setColorMode, setShowUiTestPage }: any) {
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [previewMode, setPreviewMode] = useState<DisplayMode>("fast_experimental");
  const [materialListWindowBehavior, setMaterialListWindowBehavior] = useState("independent_window");
  const [localLibraryTailPathCount, setLocalLibraryTailPathCount] = useState(3);
  const [showUiTestPage, setShowUiTestPageState] = useState(true);
  const [userConfig, setUserConfig] = useState<UserConfigInfo | null>(null);
  const [dbInfo, setDbInfo] = useState<PathInfo | null>(null);
  const [zhInfo, setZhInfo] = useState<PathInfo | null>(null);
  const [iconDirInfo, setIconDirInfo] = useState<PathInfo | null>(null);
  const [runtimeOk, setRuntimeOk] = useState<boolean | null>(null);
  const [dbOk, setDbOk] = useState<boolean | null>(null);
  const [iconOk, setIconOk] = useState<boolean | null>(null);
  const [log, setLog] = useState("");
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

  const applyUserConfig = (info: UserConfigInfo, changedConfigDirectory = false) => {
    const showUiTest = normalizeShowUiTestPage(info.config.show_ui_test_page);
    setUserConfig(info);
    setPreviewMode(normalizeDisplayMode(info.config.preview_mode));
    setMaterialListWindowBehavior(normalizeMaterialListWindowBehavior(info.config.material_list_window_behavior));
    setLocalLibraryTailPathCount(normalizeLocalLibraryTailPathCount(info.config.local_library_tail_path_count));
    setShowUiTestPageState(showUiTest);
    setShowUiTestPage?.(showUiTest);
    if (changedConfigDirectory) setTheme(normalizeTheme(info.config.theme));
    if (changedConfigDirectory) setColorMode(normalizeColorMode(info.config.color_mode));
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

  const handleShowUiTestPage = async (checked: boolean) => {
    setShowUiTestPageState(checked);
    setShowUiTestPage?.(checked);
    applyUserConfig(await saveShowUiTestPageConfig(checked));
  };

  const handleLocalLibraryTailPathCount = async (value: string) => {
    const count = normalizeLocalLibraryTailPathCount(value);
    setLocalLibraryTailPathCount(count);
    applyUserConfig(await saveLocalLibraryTailPathCountConfig(count));
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
      const [db, zh, dbText, zhText] = await Promise.all([
        getPathInfo(BLOCKSTATE_DB),
        getPathInfo(BLOCKSTATE_ZH),
        readWorkspaceFile(BLOCKSTATE_DB),
        readWorkspaceFile(BLOCKSTATE_ZH),
      ]);
      setDbInfo(db);
      setZhInfo(zh);
      JSON.parse(dbText);
      JSON.parse(zhText);
      setDbOk(true);
      setLog(`检查 BlockState 数据库：OK\n${db.normalized}\n${zh.normalized}`);
    } catch (err: any) {
      setDbOk(false);
      setLog(`检查 BlockState 数据库失败：${err}`);
    }
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
      setLog(`检查图标库：${ok ? "OK" : "失败"}\n目录：${iconDir.normalized}\ndirt.png=${dirt.exists}\ngrass_block.png=${grass.exists}`);
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
          <Input className="input settings-grow" aria-label="当前配置目录" value={userConfig?.config_dir || ""} readOnly />
        </div>
        <div className="form-row">
          <div className="form-label">默认目录</div>
          <Input className="input settings-grow" aria-label="默认配置目录" value={userConfig?.default_config_dir || ""} readOnly />
        </div>
        <div className="settings-actions">
          <Button className="btn" onClick={chooseConfigDir}>选择目录</Button>
          <Button className="btn" onClick={() => openUserConfigDir()}>打开目录</Button>
          <Button className="btn" onClick={resetConfigDir}>恢复默认目录</Button>
          <Button className="btn" onClick={migrateConfigDir}>迁移当前配置到新目录</Button>
        </div>
        <div className="settings-help-text">
          用户态文件会写入 projection-library/js_library.json、previews/、render/、generation-templates/custom/ 和普通 config。API Key 不写入普通 config。
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
            <Button className="btn" type="button" onClick={() => setShowApiKey((value) => !value)}>
              {showApiKey ? "隐藏" : "显示"}
            </Button>
          </div>
        </div>
        <div className="settings-actions">
          <Button className="btn" onClick={saveAi}>保存 Key</Button>
          <Button className="btn" onClick={clearAiKey}>清除 Key</Button>
          <Button className="btn" onClick={testAi}>测试连接</Button>
        </div>
        {(aiSaveMessage || aiTestStatus) && (
          <pre className="settings-feedback" data-state={aiTestStatus.startsWith("测试失败") ? "error" : "normal"}>
            {[aiSaveMessage, aiTestStatus].filter(Boolean).join("\n")}
          </pre>
        )}
        <div className="settings-help-text">
          API Key 不写入 localStorage、普通 config、prompt、plan 或日志；页面不会回显已保存的真实 Key。
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">测试功能</div>
        <div className="form-row">
          <div className="form-label">子窗口行为</div>
          <div className="form-field">
            <Dropdown ariaLabel="子窗口行为" value={materialListWindowBehavior}
              options={MATERIAL_LIST_WINDOW_BEHAVIOR_OPTIONS} onChange={handleMaterialListWindowBehavior} />
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">显示 UI调试页</div>
          <label className="form-field">
            <Checkbox aria-label="显示 UI 调试页" checked={showUiTestPage} onCheckedChange={(checked) => handleShowUiTestPage(checked === true)} />
            <span>在左侧导航栏显示 UI 测试入口</span>
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
            <span className="settings-muted">本地库文件夹标题压缩时，保留最后 N 级目录</span>
          </div>
        </div>
        <div className="form-row">
          <div className="form-label">演示</div>
          <div className="form-field settings-stack">
            <code className="settings-path-example">
              {"C:\\Users\\LOLcat\\Documents\\Minecraft\\Schematics\\Castle\\Overworld\\North Gate"}
            </code>
            <div>
              {compactFolderPathDemo(
                "C:\\Users\\LOLcat\\Documents\\Minecraft\\Schematics\\Castle\\Overworld\\North Gate",
                localLibraryTailPathCount,
              )}
            </div>
          </div>
        </div>
        <div className="settings-help-text">
          材料列表和 UI 测试页中的演示窗口都会按照这里的子窗口行为决定使用独立窗口还是主窗口遮罩。
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">应用运行能力</div>
        <PathRow label="工作目录" path={workspaceRoot || "..."} info={workspaceRoot ? ({ normalized: workspaceRoot, exists: true } as PathInfo) : null} />
        <div className="settings-actions settings-actions-spaced">
          <Button className="btn" onClick={checkApplicationRuntime}>检查运行能力</Button>
          <span className="settings-status" data-state={runtimeOk === false ? "error" : "normal"}>{statusText(runtimeOk)}</span>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">数据文件</div>
        <PathRow label="BlockState DB" path={BLOCKSTATE_DB} info={dbInfo} />
        <PathRow label="中文翻译 DB" path={BLOCKSTATE_ZH} info={zhInfo} />
        <PathRow label="block 图标库" path={BLOCK_ICON_DIR} info={iconDirInfo} />
        <div className="settings-actions settings-actions-spaced">
          <Button className="btn" onClick={checkDatabase}>检查 BlockState 数据库</Button>
          <Button className="btn" onClick={checkIcons}>检查图标库</Button>
          <Button className="btn" onClick={() => openWorkspacePath("data/minecraft_blockstates")}>打开数据目录</Button>
          <span className="settings-status" data-state={dbOk === false || iconOk === false ? "error" : "normal"}>
            DB: {statusText(dbOk)} / 图标: {statusText(iconOk)}
          </span>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">公开版快速渲染</div>
        <div className="form-row">
          <div className="form-label">预览图生成</div>
          <Dropdown ariaLabel="预览图生成" value={previewMode} options={DISPLAY_MODE_OPTIONS} onChange={handlePreviewMode} />
        </div>
        <div className="settings-help-text">公开 Beta 仅提供独立快速模式，不包含完整模式、光影包或完整模式配置。</div>
      </div>

      <div className="group-box">
        <div className="group-box-title">维护</div>
        <Button className="btn" onClick={async () => setLog(await cleanupLocalTempFiles())}>清理本地临时文件</Button>
      </div>

      {log && (
        <pre className="settings-feedback">{log}</pre>
      )}
    </div>
    </ScrollArea>
  );
}

