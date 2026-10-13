import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import vm from 'node:vm';
import test from 'node:test';

const require = createRequire(import.meta.url);
const ts = require('typescript');
const React = require('react');
const root = new URL('../', import.meta.url);
const source = (path) => readFileSync(new URL(path, root), 'utf8');
const tick = () => new Promise((resolve) => setImmediate(resolve));

function load(path, dependencies = {}, globals = {}) {
  const module = { exports: {} };
  const compiled = ts.transpileModule(source(path).replaceAll('import.meta', '__import_meta'), {
    fileName: path,
    reportDiagnostics: true,
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React, esModuleInterop: true },
  });
  assert.deepEqual(compiled.diagnostics?.filter((entry) => entry.category === ts.DiagnosticCategory.Error), []);
  const context = vm.createContext({ module, exports: module.exports, ...globals,
    require: (name) => name in dependencies ? dependencies[name] : require(name),
    __import_meta: { glob: () => ({
      '../themes/shadcn/theme.css': '/shadcn.css',
      '../themes/8bitcn/theme.css': '/8bitcn.css',
      '../themes/minecraft/theme.css': '/minecraft.css',
    }) },
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
    }, { window: target, document, CustomEvent });
    return { runtime, document, classes, elements, target, rootClasses,
      setSystemDark(dark) { mediaQuery.matches = dark; mediaQuery.dispatchEvent(new Event('change')); },
    };
  }
  return { createWindow, storageValues, writes, broadcasts, nativeHandlers };
}

const configNormalizer = load('src/services/userConfig.ts', {
  './backend': {}, './renderMode': { normalizeDisplayMode: () => 'full' },
}).normalizeTheme;

test('UI 与配置服务统一三主题 ID，旧主题和未知值迁移到 shadcn', () => {
  const { runtime } = environment().createWindow();
  const samples = ['', undefined, null, 'WebDefault', 'Bootstrap5', 'Metro10', 'QTDefault', 'unknown', ' SHADCN ', ' 8BITCN ', ' minecraft '];
  for (const value of samples) assert.equal(runtime.normalizeThemeId(value), configNormalizer(value));
  assert.equal(runtime.normalizeThemeId('Bootstrap5'), 'shadcn');
  assert.equal(runtime.normalizeThemeId('minecraft'), 'Minecraft');
  assert.equal(runtime.themeClassName('8bitcn'), 'theme-8bitcn');
});

test('首次挂载读取已保存主题，三样式预载且仅当前激活，保留非主题 body class', async () => {
  const { runtime, document, classes, elements } = environment().createWindow();
  await runtime.initializeWindowTheme(async () => ({ config: { theme: 'Minecraft' } }), true);
  assert.equal(runtime.currentThemeId(), 'Minecraft');
  assert.equal(document.documentElement.dataset.theme, 'minecraft');
  assert.deepEqual([...classes], ['unrelated-shell-class', 'theme-minecraft']);
  assert.equal(elements.size, 3);
  assert.equal(elements.get('litematicanova-theme-css-minecraft').media, 'all');
  assert.equal(elements.get('litematicanova-theme-css-shadcn').media, 'not all');
  runtime.announceThemeChange('8bitcn');
  assert.equal(elements.size, 3, 'switching must reuse preloaded stylesheets');
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

test('三主题选项唯一；Settings 不双写或从普通配置响应重置主题；所有窗口预初始化', () => {
  const settings = source('ui/windows/main/pages/settings/SettingsPage.tsx');
  const optionBlock = settings.match(/const NOVA_THEME_OPTIONS = \[([\s\S]+?)\];/)[1];
  assert.deepEqual([...optionBlock.matchAll(/value: "([^"]+)"/g)].map((match) => match[1]), ['shadcn', '8bitcn', 'Minecraft']);
  assert.doesNotMatch(settings, /saveThemeConfig/);
  assert.match(settings, /if \(changedConfigDirectory\) setTheme/);
  for (const file of ['app/main.tsx', 'windows/material_list/main.tsx', 'windows/local_library_folders/main.tsx', 'windows/reden_library/main.tsx', 'windows/demo_window/main.tsx']) {
    assert.match(source(`ui/${file}`), /initializeWindowTheme\(/);
  }
  assert.doesNotMatch(source('ui/shell/AppShell.tsx'), /key=\{theme\}|key=\{themeId\}|\.metro10/);
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
  const dependencies = {
    react: hooks,
    '../../src/platform/events': { listenEvent: async () => () => {} },
    '../../src/business/facade': facade,
    './themeRuntime': runtime,
    '../components/ui/button': { Button: (props) => React.createElement('button', props) },
    '../components/ui/scroll-area': { ScrollArea: (props) => React.createElement('div', props) },
    '../windows/libraryEvents': { projectionLibraryImportedEvent: 'library-import' },
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


test('三主题均解析浅色/深色/跟随系统，系统切换只改变实际配色不写回用户偏好', async () => {
  for (const theme of ['shadcn', '8bitcn', 'Minecraft']) {
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
  });
  const window = environment().createWindow();
  for (const mode of [undefined, '', 'unknown', 'light', 'dark', 'system']) {
    assert.equal(config.normalizeColorMode(mode), window.runtime.normalizeColorMode(mode));
  }
  await config.saveColorModeConfig('dark');
  await config.saveColorModeConfig('invalid');
  assert.equal(JSON.stringify(saves), JSON.stringify([{ color_mode: 'dark' }, { color_mode: 'system' }]));
  const settings = source('ui/windows/main/pages/settings/SettingsPage.tsx');
  assert.doesNotMatch(settings, /<(?:button|input|select|textarea)\b/);
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
