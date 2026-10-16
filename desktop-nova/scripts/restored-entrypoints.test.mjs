import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { test } from "node:test";
import vm from "node:vm";
import ts from "typescript";

// Handler/state tests, not a browser or visual acceptance. Window behavior is
// evaluated by the restored original helper, rather than an always-overlay stub.
const require = createRequire(import.meta.url);
const React = require("react");
const tick = () => new Promise(resolve => setImmediate(resolve));
function harness(file, exportName, options = {}) {
  let cursor = 0, tree;
  const slots = [], effects = [], cache = new Map();
  const calls = { window: [], stats: [], structural: [], quick: [], full: [], starts: [], cancels: [] };
  const config = { config: { material_list_window_behavior: options.behavior || "main_window_overlay", theme: "shadcn", color_mode: "light", preview_mode: "full", show_ui_test_page: false, local_library_tail_path_count: 3 } };
  const hooks = {
    ...React,
    useState(initial) { const index = cursor++; slots[index] ??= { value: typeof initial === "function" ? initial() : initial }; return [slots[index].value, next => { slots[index].value = typeof next === "function" ? next(slots[index].value) : next; }]; },
    useRef(initial) { const index = cursor++; slots[index] ??= { value: { current: initial } }; return slots[index].value; },
    useMemo(fn) { cursor++; return fn(); }, useCallback(fn) { cursor++; return fn; },
    useSyncExternalStore(_subscribe, snapshot) { cursor++; return snapshot(); },
    useEffect(fn, deps = []) { const index = cursor++; const previous = slots[index]; if (!previous || deps.some((value, i) => value !== previous.deps[i])) { previous?.cleanup?.(); slots[index] = { deps }; effects.push(() => { slots[index].cleanup = fn(); }); } },
  };
  hooks.useLayoutEffect = hooks.useEffect;
  const facade = {
    loadUserConfigMigratingLocalStorage: async () => config, normalizeMaterialListWindowBehavior: value => value === "main_window_overlay" ? value : "independent_window",
    openAssetManagerWindow: async () => { calls.window.push(["assets"]); }, openMaterialListWindow: async (...args) => { calls.window.push(["materials", ...args]); }, openEnumeratorWindow: async (...args) => { calls.window.push(["enumerator", ...args]); },
    loadStructureStats: async source => { calls.stats.push(source); return { marker: source, materials: [{ id: "minecraft:stone", totalCount: 2 }] }; },
    loadStructuralBlocks: async source => { calls.structural.push(source); return [{ id: "minecraft:stone", count: 2 }]; },
    createRuntimeProjectionCollection: (source, blocks) => ({ id: source, symbol: "L", values: blocks.map(block => block.id) }),
    normalizeShowUiTestPage: value => value !== false, normalizeTheme: value => value || "shadcn", normalizeColorMode: value => value || "light", normalizeDisplayMode: value => value || "full", normalizeLocalLibraryTailPathCount: value => value || 3,
    normalizeGraphicsBackend: value => value === "dx12" || value === "vulkan" ? value : "auto",
    getNovaRuntimePathInfo: async () => ({ workspaceRoot: "C:/fixture", dbInfo: null, zhInfo: null, iconDirInfo: null }),
    aiGetConfig: async () => ({ provider: "mock", base_url: "", model: "", has_key: false }),
    initI18n: async () => {}, saveThemeConfig: async () => config, saveColorModeConfig: async () => config,
    getApplicationRenderState: () => ({ operations: options.fullOperation ? [options.fullOperation] : [] }), subscribeApplicationRender: () => () => {},
    findRenderOperationForSource: () => options.fullOperation || null, windowsPathKey: value => value,
    isApplicationViewerPrepared: () => false, isApplicationPresentationVisible: () => false, isApplicationRenderPending: value => !!value,
    ensureApplicationRenderSubscription: async () => {}, updateApplicationRenderPresentation: async () => {}, discardApplicationRenderPreflight: async () => {},
    cancelApplicationRender: async () => { calls.cancels.push("full"); },
    ...options.facade,
  };
  function AssetManagerDialog() { return null; }
  function EnumeratorDialog() { return null; }
  function MaterialsDialog() { return null; }
  const preview = { operation: null, pending: false, error: "", hostRef: { current: null }, start: async () => { calls.starts.push("fast"); }, close: async () => { calls.cancels.push("fast"); }, hide: async () => {} };
  const full = { operation: options.fullOperation || null, pending: false, loading: !!options.fullOperation, ready: false, error: "", hostRef: { current: null }, start: async () => { calls.starts.push("full"); }, close: async () => { calls.cancels.push("full"); }, hide: async () => {} };
  const ui = { Button: "button", Input: "input", Checkbox: "checkbox-control", RadioGroup: "radio-group", RadioGroupItem: "radio-item", Textarea: "textarea", ScrollArea: "scroll-area", Table: "table", TableHeader: "thead", TableBody: "tbody", TableRow: "tr", TableCell: "td", TableHead: "th" };
  function load(relative) {
    if (cache.has(relative)) return cache.get(relative);
    const exports = {}; cache.set(relative, exports);
    const code = ts.transpileModule(readFileSync(new URL("../" + relative, import.meta.url), "utf8"), { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.React, esModuleInterop: true } }).outputText;
    vm.runInNewContext(code, { exports, console, setTimeout, clearTimeout, window: { confirm: () => true }, require(id) {
      if (id === "react") return hooks;
      if (id.endsWith(".css")) return {};
      if (id.endsWith("facade") || id.endsWith("business/projectionLibrary") || id.endsWith("enumeratorService")) return facade;
      if (id.endsWith("asset_manager")) return { ...load("ui/windows/asset_manager/AssetManagerWindow.tsx"), AssetManagerDialog };
      if (id.endsWith("enumerator")) return { ...load("ui/windows/enumerator/EnumeratorWindow.tsx"), EnumeratorDialog };
      if (id.endsWith("statistics/StatisticsPage")) return { ...load("ui/windows/main/pages/statistics/StatisticsPage.tsx"), MaterialsDialog };
      if (id.endsWith("useQuickPreview")) return { useQuickPreview(...args) { calls.quick.push(args); return preview; } };
      if (id.endsWith("useFullPreview")) return { useFullPreview(...args) { calls.full.push(args); return options.realFull ? load("ui/windows/main/useFullPreview.ts").useFullPreview(...args) : full; } };
      if (id.endsWith("useEmbeddedViewer")) return { useEmbeddedViewer: () => ({ hostRef: { current: null }, error: "", requestPresentationWhenReady() {}, setPresentation: async () => {}, showOverlay: async () => {} }) };
      if (id.endsWith("themeRuntime")) return { currentThemeId: () => "shadcn", currentColorMode: () => "light", themeClassName: value => value, applyThemeStylesheet() {}, subscribeToThemeChanges: () => () => {}, announceThemeChange() {}, announceColorModeChange() {} };
      if (id.endsWith("updateRecovery")) return { readUpdateDraft: () => ({ currentFile: "C:/fixture/source.litematic", route: "render" }), takeUpdateDraft() {}, useUpdateDraft() {}, useApplicationCloseRecovery: () => ({ error: "" }) };
      if (id.endsWith("platform/events")) return { emitEvent: async () => {}, listenEvent: async () => () => {} };
      if (id.endsWith("platform/tauri")) return { invoke: async () => [] };
      if (id.endsWith("libraryEvents")) return {};
      if (id === "@tauri-apps/api/window") return { getCurrentWindow: () => ({ close: async () => {} }) };
      if (id === "lucide-react") return new Proxy({}, { get: () => "svg" });
      if (id.includes("components/ui/")) return ui;
      const component = id.split("/").pop();
      return { [component]: Object.defineProperty(() => null, "name", { value: component }), useVirtualWindow: () => ({ visibleItems: [] }) };
    } });
    return exports;
  }
  const module = load(file);
  function nodes() { const values = []; const visit = value => { if (!value || typeof value !== "object") return; if (Array.isArray(value)) return value.forEach(visit); values.push(value); visit(value.props?.children); }; visit(tree); return values; }
  const text = value => value == null ? "" : Array.isArray(value) ? value.map(text).join("") : typeof value === "object" ? text(value.props?.children) : String(value);
  return {
    calls, render(props = {}) { cursor = 0; tree = module[exportName](props); while (effects.length) effects.shift()(); return tree; }, nodes,
    button(label) { return nodes().find(node => node.type === "button" && text(node.props.children) === label); },
    component(name) { return nodes().find(node => node.type?.name === name); },
    text() { return text(tree); },
  };
}

test("设置入口遵循独立窗口/遮罩偏好，关闭仅结束资源弹窗", async () => {
  for (const behavior of ["main_window_overlay", "independent_window"]) {
    const h = harness("ui/windows/main/pages/settings/SettingsPage.tsx", "SettingsPage", { behavior });
    const props = { theme: "Minecraft", setTheme() {}, setColorMode() {}, setShowUiTestPage() {} };
    h.render(props); await tick(); h.render(props);
    await h.button("游戏资源管理").props.onClick(); await tick(); h.render(props);
    if (behavior === "independent_window") { assert.deepEqual(h.calls.window, [["assets"]]); assert.equal(h.component("AssetManagerDialog"), undefined); }
    else { const dialog = h.component("AssetManagerDialog"); assert.equal(dialog.props.theme, "Minecraft"); dialog.props.onClose(); h.render(props); assert.equal(h.component("AssetManagerDialog"), undefined); }
  }
});

test("图形API读取持久配置，异步保存受控状态；失败恢复且不触发渲染重建", async () => {
  const props = { theme: "shadcn", setTheme() {}, colorMode: "light", setColorMode() {} };
  let stored = { graphics_backend: "vulkan" }, finish;
  const saved = [];
  const facade = {
    loadUserConfigMigratingLocalStorage: async () => ({ config: stored }),
    saveGraphicsBackendConfig: value => { saved.push(value); return new Promise(resolve => { finish = () => { stored = { ...stored, graphics_backend: value }; resolve({ config: stored }); }; }); },
  };
  const h = harness("ui/windows/main/pages/settings/SettingsPage.tsx", "SettingsPage", { facade });
  const selection = () => h.nodes().find(node => node.type?.name === "Dropdown" && node.props.ariaLabel === "图形 API");
  h.render(props); assert.equal(selection().props.disabled, true);
  await tick(); h.render(props);
  assert.equal(selection().props.value, "vulkan");
  assert.deepEqual(Array.from(selection().props.options, option => option.value), ["auto", "dx12", "vulkan"]);
  const lightRoute = () => h.nodes().find(node => node.type?.name === "Dropdown" && node.props.ariaLabel === "渲染后端");
  lightRoute().props.onChange("iris-gl46"); h.render(props);
  const saving = selection().props.onChange("dx12"); h.render(props);
  assert.equal(selection().props.disabled, true);
  assert.equal(selection().props.value, "dx12");
  finish(); await saving; h.render(props);
  assert.equal(selection().props.disabled, false);
  assert.equal(lightRoute().props.value, "iris-gl46", "saving graphics API must not discard an unsaved shader configuration draft");
  assert.deepEqual(saved, ["dx12"]);
  assert.equal(h.calls.starts.length, 0); assert.equal(h.calls.cancels.length, 0);
  assert.match(h.text(), /标准和完整模式当前使用 Vulkan/);
  assert.match(h.text(), /不会替换成快速模式/);
  const restarted = harness("ui/windows/main/pages/settings/SettingsPage.tsx", "SettingsPage", { facade: {
    ...facade, saveGraphicsBackendConfig: async () => { throw new Error("write failed"); },
  } });
  restarted.render(props); await tick(); restarted.render(props);
  const restoredSelection = () => restarted.nodes().find(node => node.type?.name === "Dropdown" && node.props.ariaLabel === "图形 API");
  assert.equal(restoredSelection().props.value, "dx12");
  await restoredSelection().props.onChange("vulkan"); restarted.render(props);
  assert.equal(restoredSelection().props.value, "dx12");
  assert.match(restarted.text(), /图形 API 保存失败：Error: write failed/);
  assert.equal(restarted.calls.starts.length, 0);
});

test("渲染材料浏览只在遮罩需要时读取数据，关闭恢复Preview阻挡状态且保留三模式", async () => {
  const props = { currentFile: "C:/fixture/source.litematic" };
  const independent = harness("ui/windows/main/pages/render/RenderPage.tsx", "RenderPage", { behavior: "independent_window" });
  independent.render(props); independent.render(props); independent.button("材料列表").props.onClick(); await tick(); independent.render(props);
  assert.deepEqual(independent.calls.window, [["materials", props.currentFile, "details"]]); assert.deepEqual(independent.calls.stats, []);
  const h = harness("ui/windows/main/pages/render/RenderPage.tsx", "RenderPage"); h.render(props); h.render(props);
  h.button("材料列表").props.onClick(); await tick(); h.render(props);
  const dialog = h.component("MaterialsDialog"); assert.equal(dialog.props.data.marker, props.currentFile); assert.equal(h.calls.quick.at(-1)[1], true);
  dialog.props.onClose(); h.render(props); assert.equal(h.calls.quick.at(-1)[1], false);
  for (const name of ["标准模式", "完整模式", "快速模式"]) { h.button(name).props.onClick(); h.render(props); }
  assert.equal(h.calls.starts.length, 0); assert.equal(h.calls.cancels.length, 0);
});

test("真实进度DTO驱动阶段、百分比与完成量，取消仍调用原Full动作", async () => {
  const h = harness("ui/windows/main/pages/render/RenderPage.tsx", "RenderPage", { realFull: true, fullOperation: { operationId: "full-op", state: "running", progress: { phase: "mesh_compile", completed: 5, total: 10 } } });
  h.render({ currentFile: "C:/fixture/source.litematic" }); h.render({ currentFile: "C:/fixture/source.litematic" }); h.button("完整模式").props.onClick(); h.render({ currentFile: "C:/fixture/source.litematic" });
  assert.ok(h.text().includes("正在构建 50%（5 / 10）")); const progress = h.nodes().find(node => node.type === "progress"); assert.equal(progress.props.value, 5); assert.equal(progress.props.max, 10);
  await h.button("关闭 / 取消").props.onClick(); assert.deepEqual(h.calls.cancels, ["full"]);
});

test("Shell枚举器非新路由，遮罩加载原始结构集合并向活动页面传Presentation阻挡", async () => {
  const h = harness("ui/shell/AppShell.tsx", "App"); h.render(); await tick(); h.render();
  const entry = h.nodes().find(node => node.type === "button" && node.props["aria-label"] === "枚举器"); await entry.props.onClick(); h.render(); await tick(); h.render();
  const dialog = h.component("EnumeratorDialog"); assert.equal(dialog.props.runtimeCollection.symbol, "L"); assert.deepEqual(dialog.props.runtimeCollection.values, ["minecraft:stone"]);
  const page = h.component("RenderPage"); assert.equal(page.props.presentationBlocked, true); dialog.props.onClose(); h.render(); assert.equal(h.component("RenderPage").props.presentationBlocked, false);
  assert.deepEqual(h.calls.structural, ["C:/fixture/source.litematic"]);
});
