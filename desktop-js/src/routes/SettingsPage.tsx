import React, { useEffect, useState } from 'react';
import {
  AiPublicConfig,
  aiClearKey,
  aiGetConfig,
  aiSaveConfig,
  aiTestConnection,
  cleanupJsTempFiles,
  executeBackend,
  getPathInfo,
  getWorkspaceRoot,
  openWorkspacePath,
  PathInfo,
  readWorkspaceFile,
} from '../services/backend';
import { DISPLAY_MODE_OPTIONS, loadDisplayMode, saveDisplayMode } from '../services/renderMode';

const CORE_EXE = 'bin/viewer-backend/litematica_core.exe';
const VIEWER_EXE = 'bin/viewer-backend/litematica_native_viewer.exe';
const BLOCKSTATE_DB = 'data/minecraft_blockstates/26.1.json';
const BLOCKSTATE_ZH = 'data/minecraft_blockstates/26.1.zh_cn.json';
const BLOCK_ICON_DIR = 'block';

function statusText(ok: boolean | null) {
  if (ok === null) return '未检查';
  return ok ? 'OK' : '失败';
}

function PathRow({ label, path, info }: { label: string; path: string; info?: PathInfo | null }) {
  return (
    <div className="form-row">
      <div className="form-label" style={{ width: 150 }}>{label}</div>
      <div className="form-field" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <input className="input" style={{ flex: 1 }} value={info?.normalized || path} readOnly />
        <span style={{ minWidth: 70, color: info ? (info.exists ? '#8fd18f' : '#ff8888') : '#aaa' }}>
          {info ? (info.exists ? '存在' : '缺失') : '未检查'}
        </span>
      </div>
    </div>
  );
}

export function SettingsPage({ theme, setTheme }: any) {
  const [workspaceRoot, setWorkspaceRoot] = useState('');
  const [displayMode, setDisplayMode] = useState(loadDisplayMode());
  const [coreInfo, setCoreInfo] = useState<PathInfo | null>(null);
  const [viewerInfo, setViewerInfo] = useState<PathInfo | null>(null);
  const [dbInfo, setDbInfo] = useState<PathInfo | null>(null);
  const [zhInfo, setZhInfo] = useState<PathInfo | null>(null);
  const [iconDirInfo, setIconDirInfo] = useState<PathInfo | null>(null);
  const [backendOk, setBackendOk] = useState<boolean | null>(null);
  const [dbOk, setDbOk] = useState<boolean | null>(null);
  const [iconOk, setIconOk] = useState<boolean | null>(null);
  const [log, setLog] = useState('');
  const [aiConfig, setAiConfig] = useState<AiPublicConfig | null>(null);
  const [aiProvider, setAiProvider] = useState('mock');
  const [aiBaseUrl, setAiBaseUrl] = useState('https://api.openai.com/v1');
  const [aiModel, setAiModel] = useState('gpt-4.1-mini');
  const [apiKeyInput, setApiKeyInput] = useState('');
  const [aiStatus, setAiStatus] = useState('未保存');
  const [apiKeyDirty, setApiKeyDirty] = useState(false);
  const [showApiKey, setShowApiKey] = useState(false);
  const [aiSaveMessage, setAiSaveMessage] = useState('');
  const [aiTestStatus, setAiTestStatus] = useState('');

  const refreshPathInfo = async () => {
    const [root, core, viewer, db, zh, iconDir] = await Promise.all([
      getWorkspaceRoot(),
      getPathInfo(CORE_EXE),
      getPathInfo(VIEWER_EXE),
      getPathInfo(BLOCKSTATE_DB),
      getPathInfo(BLOCKSTATE_ZH),
      getPathInfo(BLOCK_ICON_DIR),
    ]);
    setWorkspaceRoot(root);
    setCoreInfo(core);
    setViewerInfo(viewer);
    setDbInfo(db);
    setZhInfo(zh);
    setIconDirInfo(iconDir);
  };

  const refreshAiConfig = async () => {
    const config = await aiGetConfig();
    setAiConfig(config);
    setAiProvider(config.provider);
    setAiBaseUrl(config.base_url);
    setAiModel(config.model);
    setAiStatus(config.has_key ? '已保存' : config.key_status === 'cleared' ? '已清除' : '未保存');
    setApiKeyInput('');
    setApiKeyDirty(false);
  };

  useEffect(() => {
    refreshPathInfo().catch((err) => setLog(String(err)));
    refreshAiConfig().catch((err) => setLog(String(err)));
  }, []);

  const handleTheme = (value: string) => {
    setTheme(value === 'minecraft' ? 'minecraft' : 'metro10');
  };

  const handleDisplayMode = (value: string) => {
    setDisplayMode(saveDisplayMode(value));
  };

  const checkBackend = async () => {
    try {
      const [core, viewer] = await Promise.all([getPathInfo(CORE_EXE), getPathInfo(VIEWER_EXE)]);
      setCoreInfo(core);
      setViewerInfo(viewer);
      const coreHelp = await executeBackend('litematica_core.exe', ['--help']).catch((err) => String(err));
      const viewerHelp = await executeBackend('litematica_native_viewer.exe', ['--help']).catch((err) => String(err));
      const ok = core.exists && core.is_file && viewer.exists && viewer.is_file && coreHelp.length > 0 && viewerHelp.length > 0;
      setBackendOk(ok);
      setLog(`检查后端：${ok ? 'OK' : '失败'}\n\nlitematica_core:\n${coreHelp.slice(0, 1200)}\n\nnative_viewer:\n${viewerHelp.slice(0, 1200)}`);
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
        getPathInfo('block/dirt.png'),
        getPathInfo('block/grass_block.png'),
      ]);
      setIconDirInfo(iconDir);
      const ok = iconDir.exists && iconDir.is_dir && dirt.exists && grass.exists;
      setIconOk(ok);
      setLog(`检查图标库：${ok ? 'OK' : '失败'}\n目录：${iconDir.normalized}\ndirt.png=${dirt.exists}\ngrass_block.png=${grass.exists}`);
    } catch (err: any) {
      setIconOk(false);
      setLog(String(err));
    }
  };

  const cleanupTemp = async () => {
    const result = await cleanupJsTempFiles();
    setLog(result);
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
    if (typedKey) {
      setAiStatus('已保存');
      setAiSaveMessage('已保存 Key');
    } else if (saved.has_key) {
      setAiStatus('已保存');
      setAiSaveMessage('未修改已保存 Key');
    } else {
      setAiStatus('未保存');
      setAiSaveMessage('未保存 Key');
    }
    return saved;
  };

  const saveAi = async () => {
    const saved = await saveAiConfigOnly();
    setAiTestStatus('');
    setLog(`AI 配置已保存。provider=${saved.provider}, model=${saved.model}`);
  };

  const clearAiKey = async () => {
    const next = await aiClearKey();
    setApiKeyInput('');
    setApiKeyDirty(false);
    setAiConfig(next);
    setAiStatus('已清除');
    setAiSaveMessage('已清除 Key');
    setAiTestStatus('');
    setLog('AI API Key 已清除。');
  };

  const testAi = async () => {
    setAiTestStatus('测试中...');
    try {
      const saved = await saveAiConfigOnly();
      const result = await aiTestConnection();
      if (result.ok) {
        setAiTestStatus(saved.provider === 'mock' ? '测试成功：Mock 测试成功' : `测试成功：${result.message}`);
      } else {
        setAiTestStatus(`测试失败：${result.message}\nprovider=${saved.provider}\nbase_url=${saved.base_url}\nmodel=${saved.model}`);
      }
    } catch (err: any) {
      setAiTestStatus(`测试失败：${String(err)}\nprovider=${aiProvider}\nbase_url=${aiBaseUrl}\nmodel=${aiModel}`);
    }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12, height: '100%', overflow: 'auto' }}>
      <div style={{ fontSize: '1.3em', fontWeight: 'bold' }}>选项</div>

      <div className="group-box">
        <div className="group-box-title">外观</div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>主题</div>
          <div className="form-field">
            <select className="input" value={theme === 'minecraft' ? 'minecraft' : 'metro10'} onChange={(event) => handleTheme(event.target.value)}>
              <option value="metro10">Metro10</option>
              <option value="minecraft">Minecraft</option>
            </select>
          </div>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">AI 设置（实验/预留）</div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>Provider</div>
          <select className="input" value={aiProvider} onChange={(event) => setAiProvider(event.target.value)}>
            <option value="mock">Mock</option>
            <option value="openai_compatible">OpenAI Compatible</option>
            <option value="gemini_compatible">Gemini Compatible（占位）</option>
          </select>
          <span style={{ opacity: 0.72 }} title={aiConfig ? `provider=${aiConfig.provider}` : undefined}>
            Key 状态：{apiKeyDirty ? '本次输入未保存' : aiStatus}
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
          <div className="form-field" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <input
              className="input"
              type={showApiKey ? 'text' : 'password'}
              style={{ flex: 1 }}
              value={apiKeyInput}
              onChange={(event) => {
                setApiKeyInput(event.target.value);
                setApiKeyDirty(true);
                setAiSaveMessage('');
              }}
              placeholder={aiConfig?.has_key ? '已保存；输入新 Key 可覆盖' : '未保存'}
              autoComplete="off"
            />
            <button className="btn" type="button" onClick={() => setShowApiKey((value) => !value)} title={showApiKey ? '隐藏 Key' : '显示 Key'}>
              {showApiKey ? '隐藏' : '显示'}
            </button>
          </div>
        </div>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <button className="btn" onClick={saveAi}>保存 Key</button>
          <button className="btn" onClick={clearAiKey}>清除 Key</button>
          <button className="btn" onClick={testAi}>测试连接</button>
        </div>
        {(aiSaveMessage || aiTestStatus) && (
          <pre style={{ whiteSpace: 'pre-wrap', background: '#111', border: '1px solid #444', padding: 8, color: aiTestStatus.startsWith('测试失败') ? '#ffb3b3' : '#cfcfcf', marginTop: 8 }}>
            {[aiSaveMessage, aiTestStatus].filter(Boolean).join('\n')}
          </pre>
        )}
        <div style={{ opacity: 0.72, fontSize: '0.9em', marginTop: 8 }}>
          API Key 不写入 localStorage，不写入 prompt/plan/template。当前实现由 Tauri 后端保存到本地应用配置文件；页面不会回显已保存的真实 Key。
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">后端</div>
        <PathRow label="工作目录" path={workspaceRoot || '...'} info={workspaceRoot ? ({ normalized: workspaceRoot, exists: true } as PathInfo) : null} />
        <PathRow label="litematica_core" path={CORE_EXE} info={coreInfo} />
        <PathRow label="native_viewer" path={VIEWER_EXE} info={viewerInfo} />
        <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
          <button className="btn" onClick={checkBackend}>检查后端</button>
          <span style={{ color: backendOk === false ? '#ff8888' : '#aaa' }}>{statusText(backendOk)}</span>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">数据文件</div>
        <PathRow label="BlockState DB" path={BLOCKSTATE_DB} info={dbInfo} />
        <PathRow label="中文翻译 DB" path={BLOCKSTATE_ZH} info={zhInfo} />
        <PathRow label="block 图标库" path={BLOCK_ICON_DIR} info={iconDirInfo} />
        <div style={{ display: 'flex', gap: 8, marginTop: 8, flexWrap: 'wrap' }}>
          <button className="btn" onClick={checkDatabase}>检查 BlockState 数据库</button>
          <button className="btn" onClick={checkIcons}>检查图标库</button>
          <button className="btn" onClick={() => openWorkspacePath('data/minecraft_blockstates')}>打开数据目录</button>
          <span style={{ color: dbOk === false || iconOk === false ? '#ff8888' : '#aaa' }}>
            DB: {statusText(dbOk)} / 图标: {statusText(iconOk)}
          </span>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">渲染</div>
        <div className="form-row">
          <div className="form-label" style={{ width: 150 }}>displayMode 缓存</div>
          <div className="form-field" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <select className="input" value={displayMode} onChange={(event) => handleDisplayMode(event.target.value)}>
              {DISPLAY_MODE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
            <button className="btn" onClick={() => handleDisplayMode('normal')}>重置为 normal</button>
          </div>
        </div>
      </div>

      <div className="group-box">
        <div className="group-box-title">维护</div>
        <button className="btn" onClick={cleanupTemp}>清理 JS UI 临时文件</button>
      </div>

      {log && (
        <pre style={{ whiteSpace: 'pre-wrap', background: '#111', border: '1px solid #444', padding: 8, color: '#ccc', maxHeight: 220, overflow: 'auto' }}>{log}</pre>
      )}
    </div>
  );
}
