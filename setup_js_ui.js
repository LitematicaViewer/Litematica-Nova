const fs = require('fs');
const path = require('path');

function mkdir(dir) {
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
}

function write(file, content) {
  mkdir(path.dirname(file));
  fs.writeFileSync(file, content.trim() + '\n', 'utf8');
}

const root = 'desktop-nova';
mkdir(root);

write(`${root}/package.json`, `
{
  "name": "desktop-nova",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.0.0",
    "@tauri-apps/plugin-dialog": "^2.0.0",
    "@tauri-apps/plugin-shell": "^2.0.0",
    "lucide-react": "^0.300.0",
    "react": "^18.2.0",
    "react-dom": "^18.2.0",
    "react-router-dom": "^6.20.0",
    "clsx": "^2.1.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "@types/react": "^18.2.15",
    "@types/react-dom": "^18.2.7",
    "@vitejs/plugin-react": "^4.2.1",
    "typescript": "^5.2.2",
    "vite": "^5.0.0"
  }
}
`);

write(`${root}/vite.config.ts`, `
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_"],
});
`);

write(`${root}/tsconfig.json`, `
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true
  },
  "include": ["src"]
}
`);

write(`${root}/index.html`, `
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Litematica BA</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
`);

write(`${root}/src-tauri/Cargo.toml`, `
[package]
name = "desktop-nova"
version = "0.1.0"
description = "A Tauri App"
authors = ["you"]
edition = "2021"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-dialog = "2"
tauri-plugin-shell = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
base64 = "0.22"
`);

write(`${root}/src-tauri/build.rs`, `
fn main() { tauri_build::build() }
`);

write(`${root}/src-tauri/tauri.conf.json`, `
{
  "productName": "Litematica BA",
  "version": "0.1.0",
  "identifier": "com.litematicaba.dev",
  "build": {
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [{ "title": "Litematica BA", "width": 1024, "height": 768 }],
    "security": { "csp": null }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": []
  }
}
`);

write(`${root}/src-tauri/capabilities/default.json`, `
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["main"],
  "permissions": ["core:default", "dialog:default", "shell:default"]
}
`);

write(`${root}/src-tauri/src/main.rs`, `
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Command;
use std::env;
use std::path::PathBuf;
use base64::Engine;

fn get_root() -> PathBuf {
    let mut current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if current_dir.ends_with("src-tauri") { current_dir.pop(); current_dir.pop(); }
    else if current_dir.ends_with("desktop-nova") { current_dir.pop(); }
    current_dir
}

#[tauri::command]
async fn execute_backend(binary_name: String, args: Vec<String>) -> Result<String, String> {
    let current_dir = get_root();
    let exe_path = current_dir.join("bin").join("viewer-backend").join(&binary_name);
    
    let output = Command::new(exe_path).args(&args).current_dir(&current_dir).output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    
    if output.status.success() { Ok(stdout) } else { Err(format!("Exit code: {:?}\\nStdout: {}\\nStderr: {}", output.status.code(), stdout, stderr)) }
}

#[tauri::command]
fn start_native_viewer(file_path: String) -> Result<(), String> {
    let current_dir = get_root();
    let exe_path = current_dir.join("bin").join("viewer-backend").join("litematica_native_viewer.exe");
    Command::new(exe_path).arg(file_path).arg("--display-mode=full").arg("--basic-lighting").arg("--basic-shadows").current_dir(&current_dir).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn read_file_string(path: String) -> Result<String, String> {
    std::fs::read_to_string(get_root().join(path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn write_file_string(path: String, content: String) -> Result<(), String> {
    let full_path = get_root().join(path);
    if let Some(p) = full_path.parent() { std::fs::create_dir_all(p).map_err(|e| e.to_string())?; }
    std::fs::write(&full_path, content).map_err(|e| e.to_string())
}

#[tauri::command]
fn check_file_exists(path: String) -> bool {
    get_root().join(path).is_file()
}

#[tauri::command]
fn read_image_base64(path: String) -> Result<String, String> {
    if let Ok(bytes) = std::fs::read(get_root().join(path)) {
        Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)))
    } else {
        Err("Not found".into())
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![execute_backend, start_native_viewer, read_file_string, write_file_string, check_file_exists, read_image_base64])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
`);

// src files
write(`${root}/src/main.tsx`, `
import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import './styles/base.css';

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode><App /></React.StrictMode>
);
`);

write(`${root}/src/styles/base.css`, `
:root { --bg: #fff; --fg: #000; --border: #ccc; --primary: #0078d4; --primary-fg: #fff; --hover: #f0f0f0; }
body { margin: 0; font-family: sans-serif; background: var(--bg); color: var(--fg); }
* { box-sizing: border-box; }

.theme-metro10 {
  --bg: #faf9f8; --fg: #323130; --border: #e1dfdd; --primary: #0078d4; --hover: #f3f2f1; --surface: #ffffff;
}
.theme-minecraft {
  --bg: #c6c6c6; --fg: #1e1e1e; --border: #373737; --primary: #3c8527; --hover: #d0d0d0; --surface: #c6c6c6;
  font-family: monospace;
}
.theme-minecraft * {
  border-radius: 0 !important;
}

.app-container { display: flex; height: 100vh; overflow: hidden; background: var(--bg); color: var(--fg); }
.sidebar { width: 200px; border-right: 1px solid var(--border); background: var(--surface); display: flex; flex-direction: column; }
.sidebar-item { padding: 12px 16px; cursor: pointer; display: flex; align-items: center; gap: 8px; }
.sidebar-item:hover { background: var(--hover); }
.sidebar-item.active { background: var(--primary); color: var(--primary-fg); }
.main-content { flex: 1; display: flex; flex-direction: column; overflow: hidden; background: var(--bg); }
.topbar { height: 48px; border-bottom: 1px solid var(--border); display: flex; align-items: center; padding: 0 16px; background: var(--surface); font-weight: bold; }
.page-content { flex: 1; overflow: auto; padding: 16px; }

.btn { background: var(--surface); border: 1px solid var(--border); padding: 6px 12px; cursor: pointer; color: var(--fg); }
.btn:hover { background: var(--hover); }
.btn-primary { background: var(--primary); color: var(--primary-fg); border: none; }
.btn-primary:hover { opacity: 0.9; background: var(--primary); }
.btn:disabled { opacity: 0.5; cursor: not-allowed; }

.input { padding: 6px 8px; border: 1px solid var(--border); background: var(--surface); color: var(--fg); }

.card { background: var(--surface); border: 1px solid var(--border); padding: 16px; margin-bottom: 16px; }

.dropdown-container { position: relative; display: inline-block; }
.dropdown-menu { position: absolute; top: 100%; left: 0; background: var(--surface); border: 1px solid var(--border); z-index: 1000; max-height: 250px; overflow-y: auto; min-width: 100%; box-shadow: 0 4px 8px rgba(0,0,0,0.1); }
.dropdown-item { padding: 8px 12px; cursor: pointer; white-space: nowrap; }
.dropdown-item:hover { background: var(--hover); }

.dialog-overlay { position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 2000; }
.dialog-content { background: var(--surface); border: 1px solid var(--border); padding: 20px; min-width: 400px; max-width: 90%; max-height: 90%; overflow: auto; }

.rule-row { display: flex; gap: 16px; align-items: center; padding: 12px; border: 1px solid var(--border); margin-bottom: 12px; background: var(--surface); }
.rule-block { flex: 1; }
`);

write(`${root}/src/services/backend.ts`, `
import { invoke } from "@tauri-apps/api/core";

export async function readWorkspaceFile(path: string): Promise<string> {
  return await invoke("read_file_string", { path });
}
export async function writeWorkspaceFile(path: string, content: string): Promise<void> {
  return await invoke("write_file_string", { path, content });
}
export async function checkFileExists(path: string): Promise<boolean> {
  return await invoke("check_file_exists", { path });
}
export async function executeBackend(binaryName: string, args: string[]): Promise<string> {
  return await invoke("execute_backend", { binaryName, args });
}
export async function startNativeViewer(filePath: string): Promise<void> {
  return await invoke("start_native_viewer", { filePath });
}
`);

write(`${root}/src/services/blockstateDb.ts`, `
import { readWorkspaceFile } from "./backend";

export interface BlockStateDb { blocks: Record<string, { properties: Record<string, string[]> }>; }
export interface I18nDb { property_keys: Record<string, string>; property_values: Record<string, Record<string, string>>; }

let dbCache: BlockStateDb | null = null;
let i18nCache: I18nDb | null = null;

export async function loadDatabases() {
  if (!dbCache) {
    try { dbCache = JSON.parse(await readWorkspaceFile("data/minecraft_blockstates/26.1.json")); } catch (e) { console.warn(e); }
  }
  if (!i18nCache) {
    try { i18nCache = JSON.parse(await readWorkspaceFile("data/minecraft_blockstates/26.1.zh_cn.json")); } catch (e) { console.warn(e); }
  }
}
export function getBlockProperties(blockId: string): Record<string, string[]> {
  return dbCache?.blocks[blockId]?.properties || {};
}
export function translateKey(key: string): string {
  return i18nCache?.property_keys?.[key] || key;
}
export function translateValue(key: string, value: string): string {
  if (value === "$keep") return "淇濇寔鍘熺姸鎬?;
  return i18nCache?.property_values?.[key]?.[value] || value;
}
export function getAllBlocks() {
  return Object.keys(dbCache?.blocks || {});
}
`);

write(`${root}/src/components/BlockIcon.tsx`, `
import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

const iconCache = new Map<string, string | null>();

export function BlockIcon({ blockId }: { blockId: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    const fetchIcon = async () => {
      if (iconCache.has(blockId)) { setSrc(iconCache.get(blockId)!); return; }
      const displayId = blockId.replace("minecraft:", "");
      const candidates = [displayId, displayId.replace("_slab", ""), displayId.replace("potted_", "")];
      const paths = ["block", "item", "pack-in"];
      for (const p of paths) {
        for (const c of candidates) {
          try {
            const dataUrl = await invoke<string>("read_image_base64", { path: \`\${p}/\${c}.png\` });
            if (dataUrl) { iconCache.set(blockId, dataUrl); if(active) setSrc(dataUrl); return; }
          } catch (e) {}
        }
      }
      if (displayId.includes("potted") || displayId.includes("flower_pot")) {
        try {
          const dataUrl = await invoke<string>("read_image_base64", { path: \`block/flower_pot.png\` });
          iconCache.set(blockId, dataUrl); if(active) setSrc(dataUrl); return;
        } catch (e) {}
      }
      iconCache.set(blockId, null); if(active) setSrc(null);
    };
    fetchIcon();
    return () => { active = false; };
  }, [blockId]);
  
  if (src) return <img src={src} style={{ width: 32, height: 32, imageRendering: "pixelated" }} />;
  return <div style={{ width: 32, height: 32, border: "1px solid var(--border)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 10 }}>{blockId.replace("minecraft:", "").slice(0, 2).toUpperCase()}</div>;
}
`);

write(`${root}/src/components/Dropdown.tsx`, `
import React, { useState, useRef, useEffect } from "react";

export function Dropdown({ value, options, onChange, renderValue }: { value: string, options: {label: string, value: string}[], onChange: (v: string) => void, renderValue?: (v: string) => string }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const handleClick = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, []);
  
  const display = renderValue ? renderValue(value) : (options.find(o => o.value === value)?.label || value);
  return (
    <div className="dropdown-container" ref={ref}>
      <button type="button" className="btn" onClick={() => setOpen(!open)}>{display} 鈻?/button>
      {open && (
        <div className="dropdown-menu">
          {options.map(o => (
            <div key={o.value} className="dropdown-item" onClick={() => { onChange(o.value); setOpen(false); }}>
              {o.label}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
`);

write(`${root}/src/App.tsx`, `
import React, { useState, useEffect } from 'react';
import { loadDatabases } from './services/blockstateDb';
import { LibraryPage } from './routes/LibraryPage';
import { PropertiesPage } from './routes/PropertiesPage';
import { ReplacePage } from './routes/ReplacePage';
import { SettingsPage } from './routes/SettingsPage';
import { startNativeViewer } from './services/backend';

export function App() {
  const [currentFile, setCurrentFile] = useState<string>("");
  const [theme, setTheme] = useState(localStorage.getItem("theme") || "metro10");
  const [route, setRoute] = useState("library");

  useEffect(() => {
    localStorage.setItem("theme", theme);
    document.body.className = \`theme-\${theme}\`;
  }, [theme]);

  useEffect(() => { loadDatabases(); }, []);

  const pages: Record<string, { name: string, comp: React.FC<any> }> = {
    library: { name: "鎶曞奖搴?, comp: LibraryPage },
    properties: { name: "灞炴€?, comp: PropertiesPage },
    materials: { name: "鏉愭枡", comp: () => <div>鏉愭枡椤?(Placeholder)</div> },
    replace: { name: "鏂瑰潡鏇挎崲", comp: ReplacePage },
    settings: { name: "璁剧疆", comp: SettingsPage },
  };

  const Page = pages[route].comp;

  return (
    <div className="app-container">
      <div className="sidebar">
        <div style={{ padding: 16, fontWeight: 'bold', fontSize: 18, borderBottom: '1px solid var(--border)' }}>
          Litematica BA
        </div>
        {Object.entries(pages).map(([k, v]) => (
          <div key={k} className={\`sidebar-item \${route === k ? 'active' : ''}\`} onClick={() => setRoute(k)}>
            {v.name}
          </div>
        ))}
        <div style={{ flex: 1 }} />
        {currentFile && (
          <div style={{ padding: 16, borderTop: '1px solid var(--border)' }}>
            <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8, wordBreak: 'break-all' }}>{currentFile}</div>
            <button className="btn btn-primary" style={{ width: '100%' }} onClick={() => startNativeViewer(currentFile)}>
              鎵撳紑 3D 棰勮
            </button>
          </div>
        )}
      </div>
      <div className="main-content">
        <div className="topbar">
          {pages[route].name}
        </div>
        <div className="page-content">
          <Page currentFile={currentFile} setCurrentFile={setCurrentFile} theme={theme} setTheme={setTheme} />
        </div>
      </div>
    </div>
  );
}
`);

write(`${root}/src/routes/LibraryPage.tsx`, `
import React from 'react';
import { open } from "@tauri-apps/plugin-dialog";

export function LibraryPage({ currentFile, setCurrentFile }: any) {
  const handleSelect = async () => {
    const res = await open({ filters: [{ name: "Litematic", extensions: ["litematic"] }] });
    if (res && typeof res === "string") setCurrentFile(res);
  };
  return (
    <div className="card">
      <h2>鎶曞奖搴?/h2>
      <p>褰撳墠閫夋嫨: {currentFile || "鏈€夋嫨"}</p>
      <button className="btn btn-primary" onClick={handleSelect}>閫夋嫨 .litematic 鏂囦欢</button>
    </div>
  );
}
`);

write(`${root}/src/routes/PropertiesPage.tsx`, `
import React, { useState, useEffect } from 'react';
import { executeBackend } from '../services/backend';

export function PropertiesPage({ currentFile }: any) {
  const [data, setData] = useState<any>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!currentFile) return;
    executeBackend("litematica_core.exe", ["analyze", currentFile])
      .then(res => {
        try { setData(JSON.parse(res)); setError(""); }
        catch(e) { setError(res); }
      })
      .catch(e => setError(e));
  }, [currentFile]);

  if (!currentFile) return <div>璇峰厛鍦ㄦ姇褰卞簱閫夋嫨鏂囦欢銆?/div>;
  
  return (
    <div className="card">
      <h2>灞炴€т笌缁熻</h2>
      {error && <pre style={{ color: 'red' }}>{error}</pre>}
      {data && data.metadata && (
        <div>
          <p>鍚嶇О: {data.metadata.name}</p>
          <p>浣滆€? {data.metadata.author}</p>
          <p>鎻忚堪: {data.metadata.description}</p>
          <p>鎬绘柟鍧? {data.metadata.total_blocks}</p>
          <p>浣撶Н: {data.metadata.total_volume}</p>
          <p>鍖哄煙鏁? {data.metadata.region_count}</p>
        </div>
      )}
    </div>
  );
}
`);

write(`${root}/src/routes/SettingsPage.tsx`, `
import React from 'react';

export function SettingsPage({ theme, setTheme }: any) {
  return (
    <div className="card">
      <h2>璁剧疆</h2>
      <div>
        <label>涓婚: </label>
        <select className="input" value={theme} onChange={e => setTheme(e.target.value)}>
          <option value="metro10">Metro10</option>
          <option value="minecraft">Minecraft</option>
        </select>
      </div>
      <div style={{ marginTop: 20 }}>
        <h3>鍚庣鐘舵€?/h3>
        <p>璋冪敤璺緞: bin/viewer-backend/litematica_core.exe</p>
        <p>鏁版嵁搴撶増鏈? data/minecraft_blockstates/26.1.json</p>
      </div>
    </div>
  );
}
`);

write(`${root}/src/routes/ReplacePage.tsx`, `
import React, { useState } from 'react';
import { BlockIcon } from '../components/BlockIcon';
import { Dropdown } from '../components/Dropdown';
import { getBlockProperties, translateKey, translateValue, getAllBlocks } from '../services/blockstateDb';
import { executeBackend, writeWorkspaceFile, checkFileExists } from '../services/backend';
import { save } from "@tauri-apps/plugin-dialog";

function PropertySelector({ blockId, value, onChange, isTo }: any) {
  const props = getBlockProperties(blockId);
  const keys = Object.keys(props).sort();
  if (keys.length === 0) return <span style={{fontSize: 12, opacity: 0.5}}>鏃犲睘鎬?/span>;
  
  return (
    <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
      {keys.map(k => (
        <div key={k} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
          <span style={{ fontSize: 12 }}>{translateKey(k)}:</span>
          <Dropdown
            value={value[k] || (isTo ? "$keep" : "")}
            options={[
              ...(isTo ? [{label: "榛樿 (淇濇寔)", value: "$keep"}] : [{label: "鎵€鏈?, value: ""}]),
              ...props[k].map(v => ({ label: \`\${translateValue(k, v)} (\${v})\`, value: v }))
            ]}
            onChange={v => {
              const nv = { ...value };
              if (!v) delete nv[k]; else nv[k] = v;
              onChange(nv);
            }}
          />
        </div>
      ))}
    </div>
  );
}

function RuleRow({ rule, onChange, onRemove, onDuplicate }: any) {
  return (
    <div className="rule-row">
      <div className="rule-block">
        <h4>鏇挎崲鍓?(From)</h4>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 8 }}>
          <BlockIcon blockId={rule.match.name} />
          <input className="input" value={rule.match.name} onChange={e => onChange({...rule, match: {...rule.match, name: e.target.value}})} placeholder="minecraft:stone" />
        </div>
        <PropertySelector blockId={rule.match.name} value={rule.match.properties || {}} onChange={(p: any) => onChange({...rule, match: {...rule.match, properties: p}})} isTo={false} />
      </div>
      
      <div style={{ fontSize: 24, fontWeight: 'bold' }}>鈫?/div>
      
      <div className="rule-block">
        <h4>鏇挎崲鍚?(To)</h4>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 8 }}>
          <BlockIcon blockId={rule.replace.name} />
          <input className="input" value={rule.replace.name} onChange={e => onChange({...rule, replace: {...rule.replace, name: e.target.value}})} placeholder="minecraft:stone" />
        </div>
        <PropertySelector blockId={rule.replace.name} value={rule.replace.properties || {}} onChange={(p: any) => onChange({...rule, replace: {...rule.replace, properties: p}})} isTo={true} />
        <div style={{ marginTop: 8, display: 'flex', gap: 8, alignItems: 'center' }}>
          <span style={{ fontSize: 12 }}>妯″紡:</span>
          <Dropdown value={rule.property_mode} options={[{label: "淇濇寔鍚屽悕鐘舵€?(merge)", value: "merge"}, {label: "浣跨敤鐩爣榛樿 (drop)", value: "drop"}, {label: "浠呬娇鐢ㄦ墜鍔ㄦ寚瀹?(replace)", value: "replace"}]} onChange={v => onChange({...rule, property_mode: v})} />
        </div>
      </div>
      
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        <button className="btn" onClick={onDuplicate}>澶嶅埗</button>
        <button className="btn" onClick={onRemove}>鍒犻櫎</button>
      </div>
    </div>
  );
}

export function ReplacePage({ currentFile }: any) {
  const [rules, setRules] = useState<any[]>([]);
  const [log, setLog] = useState("");
  const [dryRunRes, setDryRunRes] = useState("");
  const [showDialog, setShowDialog] = useState(false);
  const [outPath, setOutPath] = useState("");

  if (!currentFile) return <div className="card">璇峰厛鍦ㄦ姇褰卞簱閫夋嫨鏂囦欢銆?/div>;

  const handleAdd = () => setRules([...rules, { match: { name: "minecraft:stone", properties: {} }, replace: { name: "minecraft:air", properties: {} }, property_mode: "merge" }]);

  const handleDryRun = async () => {
    if (rules.length === 0) return alert("璇锋坊鍔犺鍒?);
    const cleanRules = rules.map(r => {
      const matchProps = { ...r.match.properties };
      if (Object.keys(matchProps).length === 0) delete r.match.properties;
      return r;
    });
    
    await writeWorkspaceFile(".tmp/replace_rules_js.json", JSON.stringify({ rules: cleanRules }));
    setLog("鍒嗘瀽涓?..");
    try {
      const res = await executeBackend("litematica_core.exe", ["replace-blocks", "--input", currentFile, "--rules", ".tmp/replace_rules_js.json", "--dry-run"]);
      setDryRunRes(res);
      setOutPath(currentFile.replace(".litematic", ".replaced.litematic"));
      setShowDialog(true);
      setLog("Dry-run 瀹屾垚");
    } catch(e: any) {
      alert("鎵ц澶辫触: " + e);
      setLog(e);
    }
  };

  const handleApply = async () => {
    if (!outPath || outPath === currentFile) return alert("杈撳嚭璺緞涓嶅悎娉?);
    if (await checkFileExists(outPath)) return alert("杈撳嚭鏂囦欢宸插瓨鍦紝涓洪槻瑕嗙洊璇烽€夋嫨鍏朵粬璺緞锛?);
    setShowDialog(false);
    setLog("鏇挎崲涓?..");
    try {
      const res = await executeBackend("litematica_core.exe", ["replace-blocks", "--input", currentFile, "--output", outPath, "--rules", ".tmp/replace_rules_js.json"]);
      alert("鏇挎崲鎴愬姛锛乗\n" + outPath);
      setLog(res);
    } catch(e: any) {
      alert("鏇挎崲澶辫触: " + e);
      setLog(e);
    }
  };

  return (
    <div>
      <div className="card" style={{ display: 'flex', gap: 8 }}>
        <button className="btn" onClick={handleAdd}>娣诲姞瑙勫垯</button>
        <button className="btn" onClick={() => setRules([])}>娓呯┖</button>
        <div style={{ flex: 1 }} />
        <button className="btn btn-primary" onClick={handleDryRun}>鏇挎崲鏂瑰潡 (Dry-run)</button>
      </div>
      
      <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
        {rules.map((r, i) => (
          <RuleRow key={i} rule={r} onChange={(nr: any) => { const n = [...rules]; n[i] = nr; setRules(n); }} onRemove={() => setRules(rules.filter((_, idx) => idx !== i))} onDuplicate={() => setRules([...rules, JSON.parse(JSON.stringify(r))])} />
        ))}
      </div>
      
      <pre style={{ marginTop: 16, padding: 8, background: 'var(--surface)', border: '1px solid var(--border)', maxHeight: 100, overflow: 'auto' }}>{log}</pre>
      
      {showDialog && (
        <div className="dialog-overlay">
          <div className="dialog-content">
            <h3>纭鏇挎崲</h3>
            <pre style={{ maxHeight: 200, overflow: 'auto', background: 'var(--bg)', padding: 8 }}>{dryRunRes}</pre>
            <div style={{ marginTop: 16 }}>
              <label>杈撳嚭璺緞: </label>
              <div style={{ display: 'flex', gap: 8 }}>
                <input className="input" style={{ flex: 1 }} value={outPath} readOnly />
                <button className="btn" onClick={async () => {
                  const res = await save({ defaultPath: outPath, filters: [{ name: "Litematic", extensions: ["litematic"] }] });
                  if (res) setOutPath(res);
                }}>娴忚...</button>
              </div>
            </div>
            <div style={{ marginTop: 16, display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button className="btn" onClick={() => setShowDialog(false)}>鍙栨秷</button>
              <button className="btn btn-primary" onClick={handleApply}>纭鏇挎崲</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
`);
