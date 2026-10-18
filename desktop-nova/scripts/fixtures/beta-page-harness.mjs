import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import ts from "typescript";

// Exercises the page handlers and state with a deferred facade. Native IO and
// rendered layout are covered separately by the application/real-file smoke.
const require = createRequire(import.meta.url);
const React = require("react");
const pageRoot = new URL("../../ui/windows/main/pages/", import.meta.url);
function pureService(file) {
  const exports = {};
  const source = readFileSync(new URL(`../../src/services/${file}.ts`, import.meta.url), "utf8");
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS, esModuleInterop: true } }).outputText;
  vm.runInNewContext(compiled, { exports, require(id) {
    if (id.endsWith("minecraft-versions.json")) return JSON.parse(readFileSync(new URL("../../../data/minecraft-versions.json", import.meta.url), "utf8"));
    if (id === "./filePathIdentity") return pureService("filePathIdentity");
    if (id === "./themeManifest") return pureService("themeManifest");
    if (id === "./i18n") return {translateBuildingType: id => id, resolveBlockDisplayName:(id,fallback)=>fallback||id, subscribeI18nChanges:()=>()=>{}};
    if (id === "./backend" || id === "../platform/dialogs" || id === "../platform/applicationOperations") return {};
    if (id === "./renderMode") return {normalizeDisplayMode:value=>value};
    throw new Error("Unexpected pure-service dependency: " + id);
  } });
  return exports;
}
const nextTurn = () => new Promise((resolve) => setImmediate(resolve));
const deferred = () => {
  let resolve; let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
function harness(page, name, facade = {}) {
  let cursor = 0;
  const hooks = [];
  const effects = [];
  const mockReact = {
    ...React,
    useState(initial) {
      const index = cursor++;
      if (!(index in hooks)) hooks[index] = typeof initial === "function" ? initial() : initial;
      return [hooks[index], (value) => { hooks[index] = typeof value === "function" ? value(hooks[index]) : value; }];
    },
    useRef(initial) { const index = cursor++; return hooks[index] ??= { current: initial }; },
    useMemo(callback, deps) {
      const index=cursor++;
      if (!hooks[index] || !deps || deps.some((value,i)=>value!==hooks[index].deps[i])) hooks[index]={deps,value:callback()};
      return hooks[index].value;
    },
    useSyncExternalStore(subscribe, snapshot) { const index=cursor++; if(!hooks[index] || hooks[index].subscribe!==subscribe) {hooks[index]?.cleanup?.();hooks[index]={subscribe,cleanup:subscribe(()=>{})};} return snapshot(); },
    useEffect(callback, deps) {
      const index = cursor++;
      if (!hooks[index] || deps.some((dep, i) => dep !== hooks[index].deps[i])) {
        hooks[index]?.cleanup?.();
        hooks[index] = { deps };
        effects.push(() => { hooks[index].cleanup = callback(); });
      }
    },
  };
  function BlockSearchSelect() { return null; }
  function BlockPropertySelector() { return null; }
  function Dropdown() { return null; }
  function BlockIcon() { return null; }
  function Dialog({ children, footer }) { return React.createElement("div", null, children, footer); }
  const source = readFileSync(new URL(page + ".tsx", pageRoot), "utf8");
  const compiled = ts.transpileModule(source, { fileName: page + ".tsx", reportDiagnostics: true, compilerOptions: {
    jsx: ts.JsxEmit.React, target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS, esModuleInterop: true,
  } });
  assert.deepEqual(compiled.diagnostics, []);
  const viewport = { hostRef: { current: null }, error: "", showOverlay: async () => {}, setPresentation: async () => {}, requestPresentationWhenReady() {}, ...facade._viewport };
  let quickHook;
  let fullHook;
  let recoveryHook;
  const sharedUiModules = new Map();
  const recoveryStorage = new Map();
  const resolvedFacade = { ...pureService("filePathIdentity"), ...pureService("statsService"), ...pureService("userConfig"),
    getI18nRevision:()=>0,subscribeI18nChanges:()=>()=>{},
    readProjectionPreviewImage: async () => null, windowsPathKey: (path) => path, isApplicationViewerPrepared: () => false, isApplicationPresentationVisible: () => false,
    getApplicationRenderState: () => ({ operations: [] }), subscribeApplicationRender: () => () => {},
    findRenderOperationForSource: () => null, isApplicationRenderPending: () => false,
    ensureApplicationRenderSubscription: async () => {},
    updateApplicationRenderPresentation: async () => {},
    getQuickPreviewState: () => null, subscribeQuickPreview: () => () => {},
    readProjectionPreviewImage: async () => null, loadStructuralBlocks: async () => [],
    loadUserConfigMigratingLocalStorage: async () => ({ config: { material_list_window_behavior:"main_window_overlay" } }),
    normalizeMaterialListWindowBehavior: value => value === "independent_window" ? value : "main_window_overlay",
    quickPreviewForSource: (path, value) => value?.sourcePath === path ? value : null, ...facade };
  const context = { exports: {}, React:mockReact,console, performance, URLSearchParams, setTimeout, clearTimeout, window: { confirm: () => true,...facade._window }, require(id) {
    if (id === "react") return mockReact;
    if (id.endsWith("platform/nativeWindowGeometry")) return { getApplicationWindowLabel: () => "main" };
    if (id.endsWith("business/resourcePreflightUnits")) return pureService("../business/resourcePreflightUnits");
    if (id.endsWith(".css")) return {};
    if (id.endsWith("facade")) return resolvedFacade;
    if (id.endsWith("business/replace")) return { isSeparatorItem:item => item._kind === "separator", listReplacePresets:async()=>[], ...resolvedFacade };
    if (id.endsWith("replaceSessionStore")) return pureService("replaceSessionStore");
    if (id.endsWith("/statistics/StatisticsPage") || id.endsWith("/VirtualSpacerCell") || id.endsWith("/useVirtualWindow")) {
      const relative = id.endsWith("/statistics/StatisticsPage") ? "windows/main/pages/statistics/StatisticsPage.tsx"
        : `components/${id.endsWith("/VirtualSpacerCell") ? "VirtualSpacerCell.tsx" : "useVirtualWindow.ts"}`;
      if (!sharedUiModules.has(relative)) {
        const sharedSource=readFileSync(new URL(`../../ui/${relative}`,import.meta.url),"utf8");
        const code=ts.transpileModule(sharedSource,{fileName:relative,compilerOptions:{jsx:ts.JsxEmit.React,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS,esModuleInterop:true}}).outputText;
        const moduleContext={...context,exports:{}};vm.runInNewContext(code,moduleContext,{filename:relative});sharedUiModules.set(relative,moduleContext.exports);
      }
      return sharedUiModules.get(relative);
    }
    if (id.endsWith("/enumerator")) return {};
    if (id.endsWith("business/projectionLibrary")) return { ...pureService("filePathIdentity"), ...pureService("minecraftVersions"), ...resolvedFacade };
    if (id.endsWith("platform/events")) return { emitEvent: facade._emitEvent || (async () => {}),listenEvent:facade._listenEvent|| (async()=>()=>{}) };
    if(id.endsWith("themeRuntime")) return {applyThemeStylesheet() {},currentThemeId:()=>"shadcn",subscribeToThemeChanges:()=>()=>{},themeClassName:()=>"",...facade._theme};
    if (id.endsWith("platform/applicationOperations")) return {
      confirmApplicationClose: async () => {}, getApplicationCloseRequest: async () => null,
      requestApplicationClose: async () => {},
    };
    if (id.endsWith("libraryEvents")) return { projectionLibraryStateChangedEvent: "projection-library-state-changed" };
    if (id.endsWith("updateRecovery")) {
      if (!recoveryHook) {
        const source = readFileSync(new URL("../../ui/windows/main/updateRecovery.ts", import.meta.url), "utf8");
        const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS } }).outputText;
        const recoveryContext = { ...context, exports: {}, localStorage: {
          getItem: (key) => recoveryStorage.get(key) || null,
          setItem: (key, value) => recoveryStorage.set(key, value),
        }};
        vm.runInNewContext(code, recoveryContext);
        recoveryHook = recoveryContext.exports;
      }
      return recoveryHook;
    }
    if (id.endsWith("useEmbeddedViewer")) return { useEmbeddedViewer: () => viewport, usePresentationViewport: () => viewport };
    if (id.endsWith("useFullPreview")) {
      if (!fullHook) {
        const source = readFileSync(new URL("../../ui/windows/main/useFullPreview.ts", import.meta.url), "utf8");
        const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS } }).outputText;
        const hookContext = { ...context, exports: {} };
        vm.runInNewContext(code, hookContext); fullHook = hookContext.exports;
      }
      return fullHook;
    }
    if (id.endsWith("useQuickPreview")) {
      if (!quickHook) {
        const hookSource = readFileSync(new URL("../../ui/windows/main/useQuickPreview.ts", import.meta.url), "utf8");
        const hookCode = ts.transpileModule(hookSource, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS } }).outputText;
        const hookContext = { ...context, exports: {} };
        vm.runInNewContext(hookCode, hookContext);
        quickHook = hookContext.exports;
      }
      return quickHook;
    }
    if (id === "lucide-react") return new Proxy({}, { get: (_target, name) => () => React.createElement("svg", { "data-icon": name }) });
    const controls = { button: { Button: "button" }, input: { Input: "input" }, textarea: { Textarea: "textarea" },
      "scroll-area": { ScrollArea: "section", ScrollBar: "div" },
      checkbox: { Checkbox: "checkbox-control" }, slider: { Slider: "slider-control" },
      table: { Table: "table", TableHeader: "thead", TableBody: "tbody", TableFooter: "tfoot", TableRow: "tr", TableHead: "th", TableCell: "td", TableCaption: "caption" },
    };
    const ui = id.match(/\/components\/ui\/([^/]+)$/)?.[1];
    if (ui && controls[ui]) return controls[ui];
    if (id.endsWith("BlockSearchSelect")) return { BlockSearchSelect, BlockPropertySelector };
    if (id.endsWith("Dropdown")) return { Dropdown };
    if (id.endsWith("BlockIcon")) return { BlockIcon };
    if (id.endsWith("VersionConversionDialog")) return { VersionConversionDialog: () => null };
    if (id.endsWith("QuickResourceDialog")) return { QuickResourceDialog: () => null };
    if (id.endsWith("Dialog")) return { Dialog };
    throw new Error("Unexpected dependency: " + id);
  } };
  mockReact.useLayoutEffect = mockReact.useEffect;
  vm.runInNewContext(compiled.outputText, context, { filename: page + ".tsx" });
  let tree;
  return {
    render(props) { cursor = 0; tree = context.exports[name](props); while (effects.length) effects.shift()(); return tree; },
    nodes() {
      const result = [];
      function visit(node) {
        if (!node || typeof node !== "object") return;
        if (Array.isArray(node)) { node.forEach(visit); return; }
        result.push(node);
        if (typeof node.type === "function") {
          if (["BlockSearchSelect", "BlockPropertySelector", "Dropdown"].includes(node.type.name)) return;
          visit(node.type(node.props));
        } else visit(node.props?.children);
      }
      visit(tree); return result;
    },
    button(label) { return this.nodes().find((node) => node.type === "button" && text(node.props.children) === label); },
    component(name) { return this.nodes().find((node) => node.type?.name === name || node.type?.type?.name === name); },
  };
}
function text(value) {
  if (value == null || typeof value === "boolean") return "";
  if (Array.isArray(value)) return value.map(text).join("");
  if (typeof value === "object") return text(value.props?.children);
  return String(value);
}

export { harness, text, nextTurn, deferred };
