import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { posix } from 'node:path';
import vm from 'node:vm';
import test from 'node:test';

const require = createRequire(import.meta.url);
const ts = require('typescript');
const React = require('react');
const root = new URL('../', import.meta.url);
const source = (path) => readFileSync(new URL(path, root), 'utf8');
const tick = () => new Promise((resolve) => setImmediate(resolve));

// `import.meta.glob` is a build-time feature; the stub derives its map from the
// manifest so a new theme does not also need editing here.
const themeGlob = () => Object.fromEntries(
  manifest.THEMES.map((theme) => [`../themes/${theme.directory}/theme.css`, `/${theme.directory}.css`]),
);

// Local modules every UI module may import but that this suite does not need to
// exercise: a stub keeps the import graph resolvable without pulling the Tauri seam.
const builtinDependencies = {
  './motionRuntime': { initializeWindowMotion: async () => {}, animatePageChange: () => () => {} },
  '../../src/platform/nativeWindowGeometry': {
    synchronizeApplicationWindowBackground: async () => {},
    getApplicationWindowOrigin: async () => ({ x: 0, y: 0 }),
    getApplicationWindowVisibility: async () => true,
    subscribeApplicationWindowGeometry: async () => () => {},
    closeApplicationWindow: async () => {},
    getApplicationWindowLabel: () => 'main',
    subscribeApplicationWindowCloseRequested: async () => () => {},
  },
  '../../src/platform/appData': { getApplicationVersion: async () => '0.0.0' },
  '../../src/platform/tauri': { invoke: async () => undefined, isTauri: () => false },
  '../../src/services/i18n': { t: (key) => key, getUiLocale: () => 'zh-CN', localizeMessage: (value) => typeof value === 'string' ? value : value?.key ?? '', message: (key, params) => ({ key, params }) },
  '../i18n': { useI18n: () => {} },
};

function load(path, dependencies = {}, globals = {}) {
  const module = { exports: {} };
  const compiled = ts.transpileModule(source(path).replaceAll('import.meta', '__import_meta'), {
    fileName: path,
    reportDiagnostics: true,
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React, esModuleInterop: true },
  });
  assert.deepEqual(compiled.diagnostics?.filter((entry) => entry.category === ts.DiagnosticCategory.Error), []);
  const context = vm.createContext({ module, exports: module.exports,
    // The background-sync frame is a native concern; the DOM-less environment here
    // has no canvas, so the queued callback is intentionally dropped.
    requestAnimationFrame: () => 0,
    cancelAnimationFrame: () => {},
    ...globals,
    require: (name) => name in dependencies ? dependencies[name] : name in builtinDependencies ? builtinDependencies[name] : require(name),
    __import_meta: { glob: themeGlob },
  });
  vm.runInContext(compiled.outputText, context, { filename: path });
  return module.exports;
}

function environment(initialStorage = {}, { native = true, storage = true } = {}) {
  const storageValues = new Map(Object.entries(initialStorage));
  const windows = [];
  const nativeHandlers = new Set();
  const writes = [];
  const broadcasts = [];
  function createWindow() {
    const target = new EventTarget();
    const classes = new Set(['unrelated-shell-class']);
    const rootClasses = new Set();
    const mediaQuery = new EventTarget();
    mediaQuery.matches = false;
    target.matchMedia = () => mediaQuery;
    const elements = new Map();
    const document = {
      documentElement: { dataset: {}, classList: { toggle(name, enabled) { if (enabled) rootClasses.add(name); else rootClasses.delete(name); } } },
      body: { classList: { [Symbol.iterator]: () => classes[Symbol.iterator](), add: (name) => classes.add(name), remove: (name) => classes.delete(name) } },
      createElement: () => ({}),
      getElementById: (id) => elements.get(id),
      head: { appendChild(link) { elements.set(link.id, link); queueMicrotask(() => link.onload()); } },
    };
    target.localStorage = {
      getItem(key) { if (!storage) throw Error('storage unavailable'); return storageValues.get(key) ?? null; },
      setItem(key, value) {
        if (!storage) throw Error('storage unavailable');
        const oldValue = storageValues.get(key);
        storageValues.set(key, value);
        writes.push([key, value]);
        if (oldValue !== value) for (const window of windows) {
          if (window !== target) queueMicrotask(() => {
            const event = new Event('storage');
            Object.assign(event, { key, newValue: value });
            window.dispatchEvent(event);
          });
        }
      },
    };
    windows.push(target);
    const runtime = load('ui/shell/themeRuntime.ts', {
      '../../src/platform/events': {
        emitEvent: async (name, payload) => {
          broadcasts.push([name, payload]);
          if (!native) throw Error('native channel unavailable');
          for (const [eventName, handler] of nativeHandlers) if (eventName === name) handler({ payload });
        },
        listenEvent: async (name, callback) => {
          if (!native) throw Error('native channel unavailable');
          const subscription = [name, callback];
          nativeHandlers.add(subscription);
          return () => nativeHandlers.delete(subscription);
        },
      },
      '../../src/services/themeManifest': manifest,
    }, { window: target, document, CustomEvent });
    return { runtime, document, classes, elements, target, rootClasses,
      setSystemDark(dark) { mediaQuery.matches = dark; mediaQuery.dispatchEvent(new Event('change')); },
    };
  }
  return { createWindow, storageValues, writes, broadcasts, nativeHandlers };
}

const manifest = load('src/services/themeManifest.ts');

const configNormalizer = load('src/services/userConfig.ts', {
  './backend': {}, './renderMode': { normalizeDisplayMode: () => 'full' }, './themeManifest': manifest,
}).normalizeTheme;

test('UI 与配置服务统一主题 ID，已退役主题和未知值迁移到 shadcn', () => {
  const { runtime } = environment().createWindow();
  const samples = ['', undefined, null, 'WebDefault', 'Bootstrap5', 'Metro10', 'QTDefault', 'unknown', ' SHADCN ', ' 8BITCN ', ' minecraft ', ' metro10 '];
  for (const value of samples) assert.equal(runtime.normalizeThemeId(value), configNormalizer(value));
  assert.equal(runtime.normalizeThemeId('Bootstrap5'), 'shadcn');
  assert.equal(runtime.normalizeThemeId('minecraft'), 'Minecraft');
  assert.equal(runtime.normalizeThemeId('Metro10'), 'Metro10');
  assert.equal(runtime.normalizeThemeId(' metro10 '), 'Metro10');
  assert.equal(runtime.themeClassName('8bitcn'), 'theme-8bitcn');
  assert.equal(runtime.themeClassName('Metro10'), 'theme-metro10');
});

test('首次挂载读取已保存主题，各主题样式预载且仅当前激活，保留非主题 body class', async () => {
  const { runtime, document, classes, elements } = environment().createWindow();
  await runtime.initializeWindowTheme(async () => ({ config: { theme: 'Minecraft' } }), true);
  assert.equal(runtime.currentThemeId(), 'Minecraft');
  assert.equal(document.documentElement.dataset.theme, 'minecraft');
  assert.deepEqual([...classes], ['unrelated-shell-class', 'theme-minecraft']);
  assert.equal(elements.size, manifest.THEMES.length);
  assert.equal(elements.get('litematicanova-theme-css-minecraft').media, 'all');
  assert.equal(elements.get('litematicanova-theme-css-shadcn').media, 'not all');
  runtime.announceThemeChange('8bitcn');
  assert.equal(elements.size, manifest.THEMES.length, 'switching must reuse preloaded stylesheets');
  assert.equal(elements.get('litematicanova-theme-css-minecraft').media, 'not all');
  assert.equal(document.documentElement.dataset.theme, '8bitcn');
});

test('主窗与子窗双向同步；native/storage 回声去重且无循环广播写入', async () => {
  const env = environment({ 'litematicanova-theme-id': 'shadcn' });
  const main = env.createWindow();
  const child = env.createWindow();
  const mainChanges = [], childChanges = [];
  const offMain = main.runtime.subscribeToThemeChanges((id) => mainChanges.push(id));
  const offChild = child.runtime.subscribeToThemeChanges((id) => childChanges.push(id));
  main.runtime.announceThemeChange('8bitcn');
  await tick();
  child.runtime.announceThemeChange('Minecraft');
  await tick();
  assert.deepEqual(mainChanges, ['8bitcn', 'Minecraft']);
  assert.deepEqual(childChanges, ['8bitcn', 'Minecraft']);
  assert.equal(env.broadcasts.length, 2);
  assert.equal(env.writes.length, 2);
  assert.equal(main.document.documentElement.dataset.theme, 'minecraft');
  assert.equal(child.document.documentElement.dataset.theme, 'minecraft');
  offMain(); offChild();
  await tick();
  assert.equal(env.nativeHandlers.size, 0);
});

test('storage 或原生事件任一通道不可用时仍同步主题', async () => {
  for (const flags of [{ native: false }, { storage: false }]) {
    const env = environment({}, flags);
    const main = env.createWindow();
    const child = env.createWindow();
    const changes = [];
    const off = child.runtime.subscribeToThemeChanges((id) => changes.push(id));
    main.runtime.announceThemeChange('Minecraft');
    await tick();
    assert.deepEqual(changes, ['Minecraft']);
    assert.equal(child.runtime.currentThemeId(), 'Minecraft');
    off();
  }
});

test('新开子窗复用最新缓存，不读可能尚未完成持久化的旧配置', async () => {
  const env = environment();
  env.createWindow().runtime.announceThemeChange('8bitcn');
  const child = env.createWindow();
  let reads = 0;
  await child.runtime.initializeWindowTheme(async () => { reads++; return { config: { theme: 'Minecraft' } }; });
  assert.equal(reads, 0);
  assert.equal(child.runtime.currentThemeId(), '8bitcn');
  assert.equal(child.document.documentElement.dataset.theme, '8bitcn');
});

test('启动时慢配置读取不得覆盖随后收到的新主题', async () => {
  const env = environment();
  const main = env.createWindow();
  let finish;
  const loading = main.runtime.initializeWindowTheme(() => new Promise((resolve) => { finish = resolve; }), true);
  main.runtime.announceThemeChange('8bitcn');
  finish({ config: { theme: 'Minecraft' } });
  await loading;
  assert.equal(main.runtime.currentThemeId(), '8bitcn');
});

test('主题选项由清单生成；Settings 不双写或从普通配置响应重置主题；所有窗口预初始化', () => {
  const settings = source('ui/windows/main/pages/settings/SettingsPage.tsx');
  assert.match(settings, /const NOVA_THEME_OPTIONS = THEMES\.map\(/);
  assert.doesNotMatch(settings, /const NOVA_THEME_OPTIONS = \[\s*\{\s*value:/, 'theme ids must not be hardcoded in the picker');
  assert.doesNotMatch(settings, /saveThemeConfig/);
  assert.match(settings, /if \(changedConfigDirectory\) setTheme/);
  for (const file of ['app/main.tsx', 'windows/material_list/main.tsx', 'windows/local_library_folders/main.tsx', 'windows/reden_library/main.tsx', 'windows/demo_window/main.tsx']) {
    assert.match(source(`ui/${file}`), /initializeWindowTheme\(/);
  }
  assert.doesNotMatch(source('ui/shell/AppShell.tsx'), /key=\{theme\}|key=\{themeId\}|\.metro10/);
});

test('主题清单自洽：id/key 唯一、样式表存在、与 Rust 规范化一致', () => {
  const { THEMES, DEFAULT_THEME_ID, normalizeThemeId, themeKeyOf } = manifest;
  assert.ok(THEMES.length > 0);
  assert.equal(new Set(THEMES.map((theme) => theme.id)).size, THEMES.length, 'theme ids must be unique');
  assert.equal(new Set(THEMES.map((theme) => theme.key)).size, THEMES.length, 'theme keys must be unique');
  assert.ok(THEMES.some((theme) => theme.id === DEFAULT_THEME_ID), 'default theme must exist in the manifest');

  for (const theme of THEMES) {
    assert.match(theme.key, /^[a-z0-9-]+$/, `theme key must be lowercase: ${theme.key}`);
    assert.ok(existsSync(new URL(`ui/themes/${theme.directory}/theme.css`, root)),
      `missing stylesheet for theme ${theme.id}: ui/themes/${theme.directory}/theme.css`);
    assert.equal(normalizeThemeId(theme.id), theme.id);
    assert.equal(normalizeThemeId(theme.key.toUpperCase()), theme.id, 'lookup is case-insensitive');
    assert.equal(themeKeyOf(theme.id), theme.key);
  }

  // The backend rewrites stored config on read, so an id it does not know would be
  // silently replaced by the default. The canonical ids are the contract between
  // the manifest and `normalize_theme` in src-tauri/src/main.rs.
  const rust = source('src-tauri/src/main.rs');
  const normalizeTheme = rust.match(/fn normalize_theme\(value: &str\) -> String \{([\s\S]*?)\n\}/)?.[1];
  assert.ok(normalizeTheme, 'normalize_theme not found in src-tauri/src/main.rs');
  const canonical = [...normalizeTheme.matchAll(/=>\s*"([^"]+)"\.to_string\(\)/g)].map((match) => match[1]);
  const manifestIds = Array.from(THEMES, (theme) => theme.id); // host-realm array: deepEqual compares prototypes
  assert.deepEqual([...canonical].sort(), manifestIds.sort(),
    'src-tauri normalize_theme must accept every manifest id and nothing else');
});

test('快速切换时晚到的 storage 事件不回滚最新主题', async () => {
  const env = environment({ 'litematicanova-theme-id': 'shadcn' });
  const main = env.createWindow();
  const child = env.createWindow();
  const offMain = main.runtime.subscribeToThemeChanges(() => {});
  const offChild = child.runtime.subscribeToThemeChanges(() => {});
  main.runtime.announceThemeChange('8bitcn');
  main.runtime.announceThemeChange('Minecraft');
  main.runtime.announceThemeChange('shadcn');
  await tick();
  assert.equal(main.runtime.currentThemeId(), 'shadcn');
  assert.equal(child.runtime.currentThemeId(), 'shadcn');
  assert.equal(env.broadcasts.length, 3);
  assert.equal(env.writes.length, 3);
  offMain(); offChild();
});

function appHarness(runtime, facade) {
  const slots = [], effects = [];
  let cursor = 0;
  const hooks = {
    ...React,
    useState(initial) {
      const index = cursor++;
      if (!(index in slots)) slots[index] = typeof initial === 'function' ? initial() : initial;
      return [slots[index], (value) => { slots[index] = typeof value === 'function' ? value(slots[index]) : value; }];
    },
    useRef(initial) {
      const index = cursor++;
      return slots[index] ??= { current: initial };
    },
    useEffect(effect, dependencies) {
      const index = cursor++;
      if (!slots[index] || dependencies.some((value, offset) => value !== slots[index][offset])) {
        slots[index] = dependencies;
        effects.push(effect);
      }
    },
  };
  hooks.useLayoutEffect = hooks.useEffect;
  const pageStub = (props) => React.createElement('div', props);
  const dependencies = {
    react: hooks,
    '../../src/platform/events': { listenEvent: async () => () => {} },
    '../../src/business/facade': {
      normalizeDebugMode: () => false, displayPathText: (value) => value, loadStructuralBlocks: async () => [],
      createRuntimeProjectionCollection: () => ({}), saveColorModeConfig: async () => {}, ...facade,
    },
    './themeRuntime': runtime,
    '../components/ui/button': { Button: (props) => React.createElement('button', props) },
    '../components/ui/scroll-area': { ScrollArea: (props) => React.createElement('div', props) },
    '../windows/libraryEvents': { projectionLibraryImportedEvent: 'library-import' },
    // D-phase additions to AppShell's import surface; stubbed so the shell renders
    // without loading every page bundle through the vm harness.
    '../../src/business/projectionLibrary': {
      addOrUpdateRecord: async (state) => state,
      loadLibrary: async () => ({ entries: [], replicas: [], folders: [] }),
    },
    './NavIcon': { NavIcon: () => null },
    '../windows/main/UpdateDialog': { UpdateDialog: () => null },
    '../windows/main/FormatImportDialog': { FormatImportDialog: () => null },
    '../windows/main/pages/composition/CompositionPage': { CompositionPage: () => null },
    './PrimaryNavigation': { PrimaryNavigation: () => null, primaryNavigationItems: () => [], requiresProjection: () => false },
    '../windows/main/updateRecovery': {
      readUpdateDraft: () => null, takeUpdateDraft: () => null, useUpdateDraft: () => null,
      useApplicationCloseRecovery: () => ({ error: "", retry: async () => {} }),
    },
    '../windows/enumerator': { EnumeratorDialog: () => null, openEnumeratorWithWindowBehavior: async () => {} },
    '../styles/enumerator.css': {},
    '../windows/main/pages/home/HomePage': { HomePage: pageStub },
    '../windows/main/pages/library/LibraryPage': { LibraryPage: pageStub },
    '../windows/main/pages/properties/PropertiesPage': { PropertiesPage: pageStub },
    '../windows/main/pages/statistics/StatisticsPage': { StatisticsPage: pageStub },
    '../windows/main/pages/flake/FlakePage': { FlakePage: pageStub },
    '../windows/main/pages/replace/ReplacePage': { ReplacePage: pageStub },
    '../windows/main/pages/generate/GeneratePage': { GeneratePage: pageStub },
    '../windows/main/pages/render/RenderPage': { RenderPage: pageStub },
    '../windows/main/pages/settings/SettingsPage': { SettingsPage: pageStub },
    '../windows/main/pages/ui-test/UiTestPage': { UiTestPage: pageStub },
  };
  for (const [folder, name] of [['home', 'Home'], ['library', 'Library'], ['properties', 'Properties'], ['statistics', 'Statistics'], ['flake', 'Flake'], ['replace', 'Replace'], ['generate', 'Generate'], ['render', 'Render'], ['settings', 'Settings'], ['ui-test', 'UiTest']]) {
    dependencies[`../windows/main/pages/${folder}/${name}Page`] = { [`${name}Page`]: () => null };
  }
  const App = load('ui/shell/AppShell.tsx', dependencies).App;
  return () => {
    cursor = 0;
    const tree = App();
    while (effects.length) effects.shift()();
    return tree;
  };
}

function elements(tree) {
  if (Array.isArray(tree)) return tree.flatMap(elements);
  if (!React.isValidElement(tree)) return [];
  return [tree, ...elements(tree.props.children)];
}

test('AppShell 单独持久化并串行保存，主题切换保留页面组件身份和路由', async () => {
  const { runtime } = environment({ 'litematicanova-theme-id': 'shadcn' }).createWindow();
  const saved = [];
  let releaseFirst;
  const render = appHarness(runtime, {
    loadDatabases() {}, initI18n() {},
    loadUserConfigMigratingLocalStorage: async () => ({ config: { theme: 'WebDefault', show_ui_test_page: true } }),
    normalizeShowUiTestPage: (value) => value !== false,
    saveThemeConfig: (theme) => {
      saved.push(theme);
      return theme === '8bitcn' ? new Promise((resolve) => { releaseFirst = resolve; }) : Promise.resolve();
    },
  });
  const initial = render();
  const pageBefore = elements(initial).find((item) => item.props.activeRoute === 'home');
  pageBefore.props.setTheme('8bitcn');
  pageBefore.props.setTheme('Minecraft');
  await tick();
  assert.deepEqual(saved, ['8bitcn']);
  const duringSave = render();
  const pageAfter = elements(duringSave).find((item) => item.props.activeRoute === 'home');
  assert.equal(pageAfter.type, pageBefore.type);
  assert.equal(pageAfter.key, pageBefore.key);
  assert.equal(pageAfter.props.theme, 'Minecraft');
  releaseFirst();
  await tick();
  assert.deepEqual(saved, ['8bitcn', 'Minecraft']);
  runtime.announceThemeChange('Minecraft');
  await tick();
  assert.deepEqual(saved, ['8bitcn', 'Minecraft'], 'native/storage echoes and redundant updates must not save again');
});


test('每个主题均解析浅色/深色/跟随系统，系统切换只改变实际配色不写回用户偏好', async () => {
  for (const theme of manifest.THEMES.map((entry) => entry.id)) {
    const env = environment();
    const window = env.createWindow();
    await window.runtime.initializeWindowTheme(async () => ({ config: { theme, color_mode: 'system' } }), true);
    assert.equal(window.document.documentElement.dataset.colorScheme, 'light');
    const writesBefore = env.writes.length;
    window.setSystemDark(true);
    assert.equal(window.document.documentElement.dataset.colorScheme, 'dark');
    assert.equal(window.runtime.currentColorMode(), 'system');
    assert.equal(window.rootClasses.has('dark'), true);
    assert.equal(env.writes.length, writesBefore);
    window.runtime.announceColorModeChange('light');
    window.setSystemDark(false); window.setSystemDark(true);
    assert.equal(window.document.documentElement.dataset.colorScheme, 'light');
    window.runtime.announceColorModeChange('dark');
    window.setSystemDark(false);
    assert.equal(window.document.documentElement.dataset.colorScheme, 'dark');
    assert.equal(window.runtime.currentThemeId(), theme);
    assert.equal([...window.elements.values()].filter((link) => link.media === 'all').length, 1);
  }
});

test('明暗模式在主窗/子窗同步并供新窗口复用；断开订阅后停止接收', async () => {
  const env = environment();
  const main = env.createWindow(), child = env.createWindow();
  const changes = [];
  const offMain = main.runtime.subscribeToThemeChanges(() => {}, (mode) => changes.push(mode));
  const offChild = child.runtime.subscribeToThemeChanges(() => {});
  main.runtime.announceThemeChange('8bitcn');
  main.runtime.announceColorModeChange('dark');
  main.runtime.announceColorModeChange('light');
  child.runtime.announceColorModeChange('system');
  await tick();
  assert.deepEqual(changes, ['dark', 'light', 'system']);
  assert.equal(main.runtime.currentColorMode(), 'system');
  assert.equal(child.runtime.currentColorMode(), 'system');
  assert.equal(main.runtime.currentThemeId(), '8bitcn', 'appearance events must never normalize the theme');
  const fresh = env.createWindow();
  fresh.setSystemDark(true);
  await fresh.runtime.initializeWindowTheme(async () => { throw Error('new child should use shared cache'); });
  assert.equal(fresh.document.documentElement.dataset.colorScheme, 'dark');
  assert.equal(fresh.document.documentElement.dataset.theme, '8bitcn');
  offMain(); offChild(); await tick();
  assert.equal(env.nativeHandlers.size, 0);
});

test('明暗模式 native/storage 单通道及慢启动配置均不会丢失最新用户选择', async () => {
  for (const flags of [{ native: false }, { storage: false }]) {
    const env = environment({}, flags);
    const main = env.createWindow(), child = env.createWindow();
    const off = child.runtime.subscribeToThemeChanges(() => {});
    main.runtime.announceColorModeChange('dark');
    await tick();
    assert.equal(child.runtime.currentColorMode(), 'dark');
    off();
  }
  const window = environment().createWindow();
  let finish;
  const loading = window.runtime.initializeWindowTheme(() => new Promise((resolve) => { finish = resolve; }), true);
  window.runtime.announceColorModeChange('dark');
  finish({ config: { theme: 'Minecraft', color_mode: 'light' } });
  await loading;
  assert.equal(window.runtime.currentColorMode(), 'dark');
  assert.equal(window.runtime.currentThemeId(), 'Minecraft');
});

test('配置仅保存用户明暗偏好，旧配置默认跟随系统；设置页改用共享实际控件', async () => {
  const saves = [];
  const config = load('src/services/userConfig.ts', {
    './backend': { saveUserConfig: async (patch) => { saves.push(patch); return patch; } },
    './renderMode': { normalizeDisplayMode: () => 'full' },
    './themeManifest': manifest,
  });
  const window = environment().createWindow();
  for (const mode of [undefined, '', 'unknown', 'light', 'dark', 'system']) {
    assert.equal(config.normalizeColorMode(mode), window.runtime.normalizeColorMode(mode));
  }
  await config.saveColorModeConfig('dark');
  await config.saveColorModeConfig('invalid');
  assert.equal(JSON.stringify(saves), JSON.stringify([{ color_mode: 'dark' }, { color_mode: 'system' }]));
  const settings = source('ui/windows/main/pages/settings/SettingsPage.tsx');
  assert.doesNotMatch(settings.replace(/<select\b[^>]*id="nova-ui-language"[\s\S]*?<\/select>/g, ''), /<(?:button|input|select|textarea)\b/);
  for (const component of ['Button', 'Input', 'Dropdown', 'Checkbox', 'Textarea', 'ScrollArea']) assert.match(settings, new RegExp(String.raw`<${component}\b`));
});

test('主题与明暗偏好共用串行写入队列，系统外观变化不产生保存', async () => {
  const window = environment({ 'litematicanova-theme-id': 'shadcn' }).createWindow();
  const saved = [];
  let releaseFirst;
  const render = appHarness(window.runtime, {
    loadDatabases() {}, initI18n() {},
    loadUserConfigMigratingLocalStorage: async () => ({ config: { theme: 'shadcn', color_mode: 'system', show_ui_test_page: true } }),
    normalizeShowUiTestPage: (value) => value !== false,
    saveThemeConfig: (theme) => {
      saved.push('theme:' + theme);
      return new Promise((resolve) => { releaseFirst = resolve; });
    },
    saveColorModeConfig: async (mode) => { saved.push('mode:' + mode); },
  });
  const page = elements(render()).find((item) => item.props.activeRoute === 'home');
  page.props.setTheme('8bitcn');
  page.props.setColorMode('dark');
  page.props.setColorMode('system');
  await tick();
  assert.deepEqual(saved, ['theme:8bitcn']);
  releaseFirst(); await tick();
  assert.deepEqual(saved, ['theme:8bitcn', 'mode:dark', 'mode:system']);
  window.setSystemDark(true); await tick();
  assert.deepEqual(saved, ['theme:8bitcn', 'mode:dark', 'mode:system']);
});
