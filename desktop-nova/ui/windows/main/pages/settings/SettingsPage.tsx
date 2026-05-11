import React, { useEffect, useState } from "react";
import {
  AiPublicConfig,
  aiClearKey,
  aiGetConfig,
  aiSaveConfig,
  aiTestConnection,
  checkBackendHealth,
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
  saveUserConfig,
  setUserConfigDir,
  UserConfigInfo,
} from "../../../../../src/business/facade";
import { DISPLAY_MODE_OPTIONS, DisplayMode, loadDisplayMode, normalizeDisplayMode, saveDisplayMode } from "../../../../../src/business/facade";
import { loadUserConfigMigratingLocalStorage, normalizeTheme, savePreviewModeConfig, saveRenderDisplayModeConfig, saveThemeConfig } from "../../../../../src/business/facade";

const BLOCKSTATE_DB = "data/minecraft_blockstates/26.1.json";
const BLOCKSTATE_ZH = "data/minecraft_blockstates/26.1.zh_cn.json";
const BLOCK_ICON_DIR = "block";
const NOVA_THEME_OPTIONS = [
  { value: "WebDefault", label: "WebDefault" },
  { value: "Bootstrap5", label: "Bootstrap5" },
  { value: "Metro10", label: "Metro10" },
  { value: "Minecraft", label: "Minecraft" },
];

function statusText(ok: boolean | null) {
  if (ok === null) return "未检查";
  return ok ? "OK" : "失败";
}

function PathRow({ label, path, info }: { label: string; path: string; info?: PathInfo | null }) {
  return (
    <div className="form-row">
      <div className="form-label" style={{ width: 150 }}>{label}</div>
      <div className="form-field" style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <input className="input" style={{ flex: 1 }} value={info?.normalized || path} readOnly />
        <span style={{ minWidth: 70, color: info ? (info.exists ? "#8fd18f" : "#ff8888") : "#aaa" }}>
          {info ? (info.exists ? "存在" : "缺失") : "未检查"}
        </span>
      </div>
    </div>
  );
}

export function SettingsPage({ theme, setTheme }: any) {
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [displayMode, setDisplayMode] = useState<DisplayMode>(loadDisplayMode());
  const [previewMode, setPreviewMode] = useState<DisplayMode>("normal");
  const [userConfig, setUserConfig] = useState<UserConfigInfo | null>(null);
  const [coreInfo, setCoreInfo] = useState<PathInfo | null>(null);
  const [viewerInfo, setViewerInfo] = useState<PathInfo | null>(null);
  const [dbInfo, setDbInfo] = useState<PathInfo | null>(null);
  const [zhInfo, setZhInfo] = useState<PathInfo | null>(null);
  const [iconDirInfo, setIconDirInfo] = useState<PathInfo | null>(null);
  const [backendOk, setBackendOk] = useState<boolean | null>(null);
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

  const applyUserConfig = (info: UserConfigInfo) => {
    setUserConfig(info);
    setDisplayMode(normalizeDisplayMode(info.config.render_display_mode));
    setPreviewMode(normalizeDisplayMode(info.config.preview_mode));
    setTheme(normalizeTheme(info.config.theme));
  };

  const refreshPathInfo = async () => {
    const info = await getNovaRuntimePathInfo();
    setWorkspaceRoot(info.workspaceRoot);
    setCoreInfo(info.coreInfo);
    setViewerInfo(info.viewerInfo);
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
    loadUserConfigMigratingLocalStorage().then(applyUserConfig).catch((err) => setLog(String(err)));
  }, []);

  const handleTheme = async (value: string) => {
    const next = normalizeTheme(value);
    setTheme(next);
    applyUserConfig(await saveThemeConfig(next));
  };

  const handleDisplayMode = async (value: string) => {
    const mode = saveDisplayMode(value);
    setDisplayMode(mode);
    applyUserConfig(await saveRenderDisplayModeConfig(mode));
  };

  const handlePreviewMode = async (value: string) => {
    const mode = normalizeDisplayMode(value);
    setPreviewMode(mode);
    applyUserConfig(await savePreviewModeConfig(mode));
  };

  const checkBackend = async () => {
    try {
      const result = await checkBackendHealth();
      setCoreInfo(result.coreInfo);
      setViewerInfo(result.viewerInfo);
      setBackendOk(result.ok);
      setLog(`检查后端：${result.ok ? "OK" : "失败"}\n\nlitematica_core:\n${result.coreHelp.slice(0, 1200)}\n\nnative_viewer:\n${result.viewerHelp.slice(0, 1200)}`);
    } catch (err: any) {
      setBackendOk(false);
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
    applyUserConfig(await setUserConfigDir(selected, false));
    setLog(`用户配置目录已切换：${selected}`);
  };

  const migrateConfigDir = async () => {
    const selected = await chooseUserConfigDir();
    if (!selected) return;
    applyUserConfig(await setUserConfigDir(selected, true));
    setLog(`用户配置已迁移到：${selected}`);
  };

  const resetConfigDir = async () => {
    applyUserConfig(await resetUserConfigDir(true));
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
    <div style={{ display: "flex", flexDirection: "column", gap: 12, height: "100%", overflow: "auto" }}>
      <div style={{ fontSize: "1.3em", fontWeight: "bold" }}>选项</div>

      <div className="group-box">
        <div className="group-box-title">外观</div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>主题</div>
          <div className="form-field">
            <select className="input" value={normalizeTheme(theme)} onChange={(event) => handleTheme(event.target.value)}>
              {NOVA_THEME_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </div>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">用户配置目录</div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>当前目录</div>
          <input className="input" style={{ flex: 1 }} value={userConfig?.config_dir || ""} readOnly />
        </div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>默认目录</div>
          <input className="input" style={{ flex: 1 }} value={userConfig?.default_config_dir || ""} readOnly />
        </div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button className="btn" onClick={chooseConfigDir}>选择目录</button>
          <button className="btn" onClick={() => openUserConfigDir()}>打开目录</button>
          <button className="btn" onClick={resetConfigDir}>恢复默认目录</button>
          <button className="btn" onClick={migrateConfigDir}>迁移当前配置到新目录</button>
        </div>
        <div style={{ opacity: 0.72, fontSize: "0.9em", marginTop: 8 }}>
          用户态文件会写入 projection-library/js_library.json、previews/、render/、generation-templates/custom/ 和普通 config。API Key 不写入普通 config。
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">AI 设置</div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>Provider</div>
          <select className="input" value={aiProvider} onChange={(event) => setAiProvider(event.target.value)}>
            <option value="mock">Mock</option>
            <option value="openai_compatible">OpenAI Compatible</option>
            <option value="gemini_compatible">Gemini Compatible（占位）</option>
          </select>
          <span style={{ opacity: 0.72 }} title={aiConfig ? `provider=${aiConfig.provider}` : undefined}>
            Key 状态：{apiKeyDirty ? "本次输入未保存" : aiStatus}
          </span>
        </div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>Base URL</div>
          <input className="input" style={{ flex: 1 }} value={aiBaseUrl} onChange={(event) => setAiBaseUrl(event.target.value)} placeholder="https://api.openai.com/v1" />
        </div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>Model</div>
          <input className="input" style={{ width: 260 }} value={aiModel} onChange={(event) => setAiModel(event.target.value)} placeholder="gpt-4.1-mini" />
        </div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>API Key</div>
          <div className="form-field" style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <input
              className="input"
              type={showApiKey ? "text" : "password"}
              style={{ flex: 1 }}
              value={apiKeyInput}
              onChange={(event) => {
                setApiKeyInput(event.target.value);
                setApiKeyDirty(true);
                setAiSaveMessage("");
              }}
              placeholder={aiConfig?.has_key ? "已保存；输入新 Key 可替换" : "未保存"}
              autoComplete="off"
            />
            <button className="btn" type="button" onClick={() => setShowApiKey((value) => !value)}>
              {showApiKey ? "隐藏" : "显示"}
            </button>
          </div>
        </div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button className="btn" onClick={saveAi}>保存 Key</button>
          <button className="btn" onClick={clearAiKey}>清除 Key</button>
          <button className="btn" onClick={testAi}>测试连接</button>
        </div>
        {(aiSaveMessage || aiTestStatus) && (
          <pre style={{ whiteSpace: "pre-wrap", background: "#111", border: "1px solid #444", padding: 8, color: aiTestStatus.startsWith("测试失败") ? "#ffb3b3" : "#cfcfcf", marginTop: 8 }}>
            {[aiSaveMessage, aiTestStatus].filter(Boolean).join("\n")}
          </pre>
        )}
        <div style={{ opacity: 0.72, fontSize: "0.9em", marginTop: 8 }}>
          API Key 不写入 localStorage、普通 config、prompt、plan 或日志；页面不会回显已保存的真实 Key。
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">后端</div>
        <PathRow label="工作目录" path={workspaceRoot || "..."} info={workspaceRoot ? ({ normalized: workspaceRoot, exists: true } as PathInfo) : null} />
        <PathRow label="litematica_core" path={coreInfo?.normalized || ""} info={coreInfo} />
        <PathRow label="native_viewer" path={viewerInfo?.normalized || ""} info={viewerInfo} />
        <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
          <button className="btn" onClick={checkBackend}>检查后端</button>
          <span style={{ color: backendOk === false ? "#ff8888" : "#aaa" }}>{statusText(backendOk)}</span>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">数据文件</div>
        <PathRow label="BlockState DB" path={BLOCKSTATE_DB} info={dbInfo} />
        <PathRow label="中文翻译 DB" path={BLOCKSTATE_ZH} info={zhInfo} />
        <PathRow label="block 图标库" path={BLOCK_ICON_DIR} info={iconDirInfo} />
        <div style={{ display: "flex", gap: 8, marginTop: 8, flexWrap: "wrap" }}>
          <button className="btn" onClick={checkDatabase}>检查 BlockState 数据库</button>
          <button className="btn" onClick={checkIcons}>检查图标库</button>
          <button className="btn" onClick={() => openWorkspacePath("data/minecraft_blockstates")}>打开数据目录</button>
          <span style={{ color: dbOk === false || iconOk === false ? "#ff8888" : "#aaa" }}>
            DB: {statusText(dbOk)} / 图标: {statusText(iconOk)}
          </span>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">渲染</div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>渲染页 displayMode</div>
          <div className="form-field" style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <select className="input" value={displayMode} onChange={(event) => handleDisplayMode(event.target.value)}>
              {DISPLAY_MODE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
            <button className="btn" onClick={() => handleDisplayMode("normal")}>重置为 normal</button>
          </div>
        </div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>预览图生成</div>
          <div className="form-field" style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <select className="input" value={previewMode} onChange={(event) => handlePreviewMode(event.target.value)}>
              {DISPLAY_MODE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
            <button className="btn" onClick={() => handlePreviewMode("normal")}>重置为 normal</button>
          </div>
        </div>
        <div style={{ opacity: 0.72, fontSize: "0.9em", marginTop: 8 }}>
          投影库“生成预览”使用“预览图生成”模式；它独立于渲染页 displayMode。
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">维护</div>
        <button className="btn" onClick={async () => setLog(await cleanupLocalTempFiles())}>清理本地临时文件</button>
      </div>

      {log && (
        <pre style={{ whiteSpace: "pre-wrap", background: "#111", border: "1px solid #444", padding: 8, color: "#ccc", maxHeight: 220, overflow: "auto" }}>{log}</pre>
      )}
    </div>
  );
}


