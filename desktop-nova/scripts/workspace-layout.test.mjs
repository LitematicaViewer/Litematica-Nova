import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const read = path => fs.readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
function load(path, { locale = 'zh-CN', imports = {} } = {}) {
  const exports = {};
  const output = ts.transpileModule(read(path), { compilerOptions: {
    module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React,
  }}).outputText;
  vm.runInNewContext(output, { exports, require: name => {
    if (name in imports) return imports[name];
    if (name === 'react') return { ...React, default: React };
    if (name.includes('i18n')) return { useI18n: () => ({ locale }) };
    if (name.endsWith('/NavIcon')) return { NavIcon: ({ name }) => React.createElement('span', { 'data-icon': name }) };
    return { Button: ({ children, variant, ...props }) => React.createElement('button', props, children) };
  } });
  return exports;
}

test('界面倍率：完整区间、精确输入、0.05步进，非法值不静默截断', () => {
  const api = load('ui/windows/main/pages/settings/uiScaleInput.ts');
  for (const value of [.5, .75, 1, 1.25, 2, 3, 4, 5, 1.23]) assert.equal(api.parseUiScaleDraft(value.toFixed(2)), value);
  for (const value of ['', 'NaN', 'Infinity', '.49', '0.49', '5.01', '1.234', 'abc']) assert.equal(api.parseUiScaleDraft(value), null);
  assert.equal(api.stepUiScale(1.23, 1), 1.28);
  assert.equal(api.stepUiScale(.5, -1), null);
  assert.equal(api.stepUiScale(5, 1), null);
  assert.equal(api.UI_SCALE_PRESETS.at(-1), 5);
});

test('一级导航：十个入口直接可达，保持旧route ID，不再显示工作台二级标签', () => {
  const expected = ['home', 'library', 'properties', 'render', 'flake', 'statistics', 'replace', 'composition', 'generate', 'settings'];
  for (const locale of ['zh-CN', 'en-US']) {
    const navigation = load('ui/shell/PrimaryNavigation.tsx', { locale });
    assert.deepEqual([...navigation.PRIMARY_ROUTES], expected);
    const visited = [];
    const element = navigation.PrimaryNavigation({ route: 'render', onNavigate: route => visited.push(route), theme: 'shadcn' });
    for (const button of React.Children.toArray(element.props.children)) button.props.onClick();
    assert.deepEqual(visited, expected);
    const html = renderToStaticMarkup(element);
    assert.equal((html.match(/<button/g) ?? []).length, 10);
    assert.equal((html.match(/aria-current="page"/g) ?? []).length, 1);
    for (const label of locale === 'zh-CN' ? ['首页', '投影库', '概览', '三维', '分层', '备货', '替换', '组合/拆分', '生成', '设置']
      : ['Home', 'Library', 'Overview', '3D', 'Layers', 'Stockpile', 'Replace', 'Compose / Split', 'Generate', 'Settings']) assert.ok(html.includes(label));
    assert.equal(navigation.requiresProjection('composition'), false);
    for (const route of ['properties', 'render', 'flake', 'statistics', 'replace']) assert.equal(navigation.requiresProjection(route), true);
  }
  const shell = read('ui/shell/AppShell.tsx');
  assert.ok(shell.includes('<PrimaryNavigation route={route} onNavigate={setRoute} theme={theme} />'));
  assert.ok(shell.includes('requiresProjection(route) && !currentFile'));
  assert.ok(shell.includes('currentFile={currentFile}'));
  assert.ok(!shell.includes('WorkbenchNavigation'));
  assert.ok(!shell.includes('lastWorkbenchRoute'));
  const css = read('ui/shell/workspace.css');
  assert.ok(css.includes('width: 188px; min-width: 188px; flex-basis: 188px'));
  assert.ok(css.includes('.sidebar > .nav-scroll-area { flex: 1 1 0; min-height: 38px; }'));
  assert.ok(!css.includes('.workbench-tabs'));
});

test('设置目录：七类设置均真实保留', () => {
  const settings = load('ui/windows/main/pages/settings/SettingsCategories.tsx');
  const categories = renderToStaticMarkup(React.createElement(settings.SettingsCategories, { category: 'appearance', onSelect() {} }));
  assert.equal((categories.match(/<button/g) ?? []).length, 7);
  for (const label of ['外观', '数据与目录', '渲染', '光影与资源包', 'AI', '高级与诊断', '维护']) assert.ok(categories.includes(label));
});

test('倍率控件：慢回执期间不丢连按步进，精确值不被滑块量化，仍以Host回执为准', async () => {
  const { JSDOM } = await import('jsdom');
  const dom = new JSDOM('<div id="root"></div>');
  const names = ['window', 'document', 'HTMLElement', 'Element', 'Node', 'Event', 'CustomEvent', 'getComputedStyle', 'ResizeObserver', 'IS_REACT_ACT_ENVIRONMENT'];
  const previous = Object.fromEntries(names.map(name => [name, globalThis[name]]));
  for (const name of ['window', 'document', 'HTMLElement', 'Element', 'Node', 'Event', 'CustomEvent']) globalThis[name] = name === 'window' ? dom.window : dom.window[name];
  globalThis.getComputedStyle = dom.window.getComputedStyle;
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} };
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { createRoot } = await import('react-dom/client');
  const radix = await import('radix-ui');
  const realSlider = load('ui/components/ui/slider.tsx', { imports: { 'radix-ui': radix, '../../lib/utils': { cn: (...values) => values.filter(Boolean).join(' ') } } }).Slider;
  let sliderProps;
  const requests = [], values = [];
  let broadcast;
  const api = load('ui/windows/main/pages/settings/UiScaleControl.tsx', { imports: {
    'lucide-react': { Minus: () => null, Plus: () => null },
    '../../../../components/ui/slider': { Slider: props => { sliderProps = props; return React.createElement(realSlider, props); } },
    '../../../../../src/platform/appData': { getUserConfig: async () => ({ config: { ui_scale: 1 } }), saveUserConfig: input => new Promise(resolve => {
      values.push(input.ui_scale);
      requests.push(() => { broadcast({ payload: input.ui_scale }); resolve({ config: { ui_scale: input.ui_scale } }); });
    }) },
    '../../../../../src/platform/events': { listenEvent: async (_name, handler) => { broadcast = handler; return () => {}; } },
    './uiScaleInput': load('ui/windows/main/pages/settings/uiScaleInput.ts'),
  } });
  const root = createRoot(dom.window.document.getElementById('root'));
  try {
    await React.act(async () => { root.render(React.createElement(api.UiScaleControl)); });
    const precise = () => dom.window.document.querySelector('input[aria-label="精确倍率"]');
    await React.act(async () => {
      const increase = dom.window.document.querySelector('button[aria-label="放大界面"]');
      for (let i = 0; i < 3; i++) increase.click();
    });
    assert.equal(precise().value, '1.15');
    for (let i = 0; i < 3; i++) {
      assert.equal(requests.length, 1);
      await React.act(async () => { requests.shift()(); });
      assert.equal(precise().value, '1.15');
    }
    assert.deepEqual(values, [1.05, 1.1, 1.15]);
    await React.act(async () => { broadcast({ payload: 1.23 }); });
    assert.equal(precise().value, '1.23');
    const slider = dom.window.document.querySelector('[role="slider"]');
    assert.equal(slider.getAttribute('aria-valuenow'), '1.23'); assert.equal(sliderProps.step, 0.05);
    assert.equal(slider.getAttribute('aria-valuemin'), '0.5'); assert.equal(slider.getAttribute('aria-valuemax'), '5');
    await React.act(async () => { [...dom.window.document.querySelectorAll('button')].find(button => button.textContent === '恢复默认').click(); });
    await React.act(async () => { requests.shift()(); });
    assert.equal(precise().value, '1.00');
  } finally {
    await React.act(async () => { root.unmount(); });
    for (const name of names) globalThis[name] = previous[name]; dom.window.close();
  }
});

test('Host全局缩放：新开窗口、已开窗口、保存和原生恢复共享同一配置', () => {
  const host = read('src-tauri/src/main.rs');
  const zoom = read('src-tauri/src/application/ui_scale.rs');
  assert.ok(host.includes('webview.set_zoom(read_user_config_data(&dir).ui_scale)'));
  assert.ok(host.includes('application::ui_scale::apply(&app, info.config.ui_scale)?'));
  assert.ok(zoom.includes('for window in app.webview_windows().values()'));
  assert.ok(zoom.includes('CmdOrCtrl+Alt+0'));
  assert.ok(zoom.includes('crate::save_user_config_inner(input)'));
  assert.ok(!zoom.includes('build_scene'));
  assert.ok(read('src/platform/nativeWindowGeometry.ts').includes('listenEvent<number>("nova-ui-scale-changed", notify)'));
  assert.ok(!read('ui/shell/workspace.css').includes('transform: scale'));
});

test('原生视口：各UI倍率×系统DPI只换算一次，不改变几何身份', async () => {
  for (const uiScale of [.5, .75, 1, 1.25, 2, 3, 4, 5]) {
    for (const osDpi of [1, 1.25, 1.5, 2]) {
      const exports = {};
      const code = ts.transpileModule(read('ui/windows/main/embeddedViewerGeometry.ts'), { compilerOptions: { module: ts.ModuleKind.CommonJS }}).outputText;
      const dpr = uiScale * osDpi;
      vm.runInNewContext(code, { exports, require: () => ({
        getApplicationWindowOrigin: async () => ({ x: 0, y: 0 }), getApplicationWindowVisibility: async () => true,
      }), document: { hasFocus: () => true }, window: { devicePixelRatio: dpr, innerWidth: 1000, innerHeight: 800 } });
      const result = await exports.elementToNativeChildPlacement({ parentElement: null,
        getBoundingClientRect: () => ({ left: 10, top: 20, right: 110, bottom: 70 }) });
      assert.equal(result.width, Math.round(100 * dpr));
      assert.equal(result.height, Math.round(50 * dpr));
    }
  }
});

test('导航只有Radix滚动层，独立热区10px、视觉4/6px，不改变页面滚动条', () => {
  const controls = read('ui/styles/controls.css');
  const workspace = read('ui/shell/workspace.css');
  const widgets = read('ui/styles/widgets.css');
  assert.match(workspace, /\.app-shell \.sidebar \{[^}]*overflow: visible/);
  assert.doesNotMatch(workspace, /\.app-shell \.sidebar \{[^}]*overflow-y: auto/);
  assert.match(controls, /--nova-nav-scroll-hit: 10px; --nova-nav-scroll-visual: 4px/);
  assert.match(controls, /width: calc\(100% - var\(--nova-nav-scroll-hit\)\)/);
  assert.match(controls, /\[data-slot="scroll-area-scrollbar"\]:is\(:hover, :active\) \{ --nova-nav-scroll-visual: 6px/);
  assert.match(controls, /--radix-scroll-area-thumb-width: var\(--nova-nav-scroll-visual\)/);
  assert.match(controls, /width: var\(--nova-nav-scroll-hit\);\s+align-items: flex-start/);
  assert.match(controls, /border-image: none/);
  assert.match(controls, /scroll-area-viewport"\] \{ scrollbar-width: none/);
  assert.match(controls, /scroll-area-viewport"\]::-webkit-scrollbar \{ display: none/);
  assert.match(controls, /scroll-area-viewport"\]:focus-visible \{ outline: 2px/);
  assert.match(widgets, /--ln-scroll-size: 12px/);
  assert.match(widgets, /--ln-scroll-size: 14px/);
  assert.ok(read('ui/components/ui/scroll-area.tsx').includes('type="auto"'));
  for (const scale of [.5, 1, 1.5, 3, 5]) {
    // 56px collapsed rail: 10px reserved, 12px button margins, 12px padding;
    // the 16px icon fits without entering the drag target at every WebView zoom.
    assert.ok((56 - 10 - 12 - 12) * scale >= 16 * scale);
    assert.equal(6 * scale / (56 * scale), 6 / 56);
    assert.ok((40 - 10 - 4 - 4) * scale >= 16 * scale);
  }
});

function motionWindow({ initial = 'system', reduced = false, readConfig } = {}) {
  const exports = {}, handlers = new Map(), media = new EventTarget();
  media.matches = reduced;
  const document = { documentElement: { dataset: {} } };
  const consoleMessages = [];
  const code = ts.transpileModule(read('ui/shell/motionRuntime.ts'), { compilerOptions: {
    module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022,
  } }).outputText;
  vm.runInNewContext(code, {
    exports, document, window: { matchMedia: () => media },
    console: { warn: value => consoleMessages.push(value) },
    getComputedStyle: () => ({ getPropertyValue: key => ({ '--nova-motion-page-duration': '180ms', '--nova-motion-page-distance': '8px' }[key] || '') }),
    require: () => ({ listenEvent: async (name, callback) => { handlers.set(name, callback); return () => handlers.delete(name); } }),
  });
  return { api: exports, document, media, handlers, consoleMessages,
    init: () => exports.initializeWindowMotion(readConfig ?? (async () => ({ config: { motion_preference: initial } }))),
    broadcast: value => handlers.get('user-config-changed')({ payload: { config: { motion_preference: value } } }),
    system: value => { media.matches = value; media.dispatchEvent(new Event('change')); },
  };
}

test('动效默认跟随实时系统，五级手动覆盖，主子窗口只接受Rust配置回执', async () => {
  const main = motionWindow({ reduced: true });
  const child = motionWindow({ reduced: true });
  await Promise.all([main.init(), child.init()]);
  for (const win of [main, child]) {
    assert.equal(win.document.documentElement.dataset.motionPreference, 'system');
    assert.equal(win.document.documentElement.dataset.motion, 'reduced');
    win.system(false); assert.equal(win.api.currentMotionState().effective, 'standard');
    win.system(true); assert.equal(win.api.currentMotionState().effective, 'reduced');
    for (const preference of ['off', 'reduced', 'standard', 'rich']) {
      win.broadcast(preference);
      assert.equal(win.api.currentMotionState().preference, preference);
      assert.equal(win.api.currentMotionState().effective, preference);
      win.system(false); win.system(true);
      assert.equal(win.api.currentMotionState().effective, preference);
    }
    win.broadcast('invalid'); assert.equal(win.api.currentMotionState().preference, 'system');
    assert.ok(!read('ui/shell/motionRuntime.ts').includes('localStorage'));
  }
  // A subsequently opened child gets the same saved manual preference at bootstrap.
  const reopened = motionWindow({ initial: 'rich', reduced: true });
  await reopened.init(); assert.equal(reopened.api.currentMotionState().effective, 'rich');
  assert.match(read('ui/shell/themeRuntime.ts'), /initializeWindowMotion\(\(\) => savedConfig\)/);
  assert.match(read('src-tauri/src/main.rs'), /motion_preference/);
});

test('晚到初始配置不覆盖运行中Rust广播，页面动画只改变呈现且即时停止', async () => {
  let resolve;
  const win = motionWindow({ readConfig: () => new Promise(done => { resolve = done; }) });
  const init = win.init();
  await new Promise(setImmediate);
  win.broadcast('rich'); resolve({ config: { motion_preference: 'off' } });
  await init;
  assert.equal(win.api.currentMotionState().effective, 'rich');
  let animations = 0, cancellations = 0, frames;
  const element = { animate: (keyframes, options) => {
    animations++; frames = keyframes;
    assert.equal(options.duration, 180);
    return { cancel: () => { cancellations++; } };
  } };
  const dispose = win.api.animatePageChange(element);
  assert.equal(animations, 1); assert.equal(frames[0].transform, 'translateY(8px)');
  let nativeFrames;
  const disposeNative = win.api.animatePageChange({
    querySelectorAll: () => [{ closest: () => null }],
    animate: keyframes => { nativeFrames = keyframes; return { cancel() {} }; },
  });
  assert.equal('transform' in nativeFrames[0], false, '原生子HWND锚点不跟随WebView位移');
  disposeNative();
  win.broadcast('off'); assert.equal(cancellations, 1);
  win.api.animatePageChange(element); assert.equal(animations, 1);
  dispose(); assert.equal(cancellations, 2);
  const runtime = read('ui/shell/motionRuntime.ts');
  assert.doesNotMatch(runtime, /setTimeout|setInterval|invoke\(|startViewTransition|cloneNode/);
  const shell = read('ui/shell/AppShell.tsx');
  assert.match(shell, /useLayoutEffect\(\(\) => animatePageChange\(pageRef.current\), \[route\]\)/);
  assert.match(shell, /hidden=\{route !== "composition"\}/);
  assert.doesNotMatch(shell, /key=\{route\}/);
});

test('统一动效覆盖按钮/标签/卡片/弹层，关闭/精简/像素语言保持正确', () => {
  const css = read('ui/styles/motion.css');
  for (const entry of ['[data-slot="button"]', '[role="tab"]', '.library-tile', '[data-slot="dialog-content"]', '[data-slot="select-content"]', '.nav-item']) assert.ok(css.includes(entry));
  for (const mode of ['off', 'reduced', 'rich']) assert.ok(css.includes(`html[data-motion="${mode}"]`));
  assert.ok(css.includes('--nova-motion-overlay-scale: 1'));
  assert.ok(css.includes('[data-theme="minecraft"]'));
  assert.ok(css.includes('html:not([data-motion])'));
  assert.ok(css.includes('nth-child(-n+8)'));
  assert.doesNotMatch(css, /infinite|blur\(|spring|elastic/);
  assert.match(read('ui/themes/modern.css'), /html:not\(\[data-motion\]\) \*/);
  for (const key of ['motion', 'motion.system', 'motion.off', 'motion.reduced', 'motion.standard', 'motion.rich', 'motionHelp', 'motionSystemReduced', 'motionFailed']) {
    assert.equal(read('src/services/locales/settings.ts').split(`"settings.${key}":`).length - 1, 2);
  }
});

test('动效控件等待Rust回执后生效，保存失败保留旧有效选项并显示原错误', async () => {
  const slots = [], effects = [], subscribers = new Set();
  let cursor = 0, state = { preference: 'system', effective: 'standard', systemReduced: false }, settle;
  const hooks = { ...React,
    useState(initial) {
      const index = cursor++;
      if (!(index in slots)) slots[index] = typeof initial === 'function' ? initial() : initial;
      return [slots[index], next => { slots[index] = typeof next === 'function' ? next(slots[index]) : next; }];
    },
    useEffect(effect) { if (!effects.length) effects.push(effect()); },
  };
  hooks.default = hooks;
  const api = load('ui/windows/main/pages/settings/MotionControl.tsx', { imports: {
    react: hooks,
    '../../../../../src/services/i18n': { t: (key, params) => params?.error ? `${key}: ${params.error}` : key },
    '../../../../../src/platform/appData': { saveUserConfig: input => new Promise((resolve, reject) => {
      assert.deepEqual(Object.keys(input), ['motion_preference']); settle = { resolve, reject };
    }) },
    '../../../../shell/motionRuntime': {
      currentMotionState: () => state,
      subscribeToMotionChanges: callback => { subscribers.add(callback); return () => subscribers.delete(callback); },
      acceptMotionConfig: info => { state = { ...state, preference: info.config.motion_preference, effective: info.config.motion_preference }; for (const callback of subscribers) callback(); },
    },
    '../../../../components/Dropdown': { Dropdown: 'motion-dropdown' },
  } });
  const render = () => { cursor = 0; return api.MotionControl(); };
  const all = tree => !React.isValidElement(tree) ? [] : [tree, ...React.Children.toArray(tree.props.children).flatMap(all)];
  let tree = render();
  all(tree).find(el => el.type === 'motion-dropdown').props.onChange('rich');
  tree = render();
  assert.equal(all(tree).find(el => el.type === 'motion-dropdown').props.value, 'system');
  assert.equal(all(tree).find(el => el.type === 'motion-dropdown').props.disabled, true);
  settle.reject(Error('disk write failed'));
  await new Promise(setImmediate);
  tree = render();
  assert.equal(all(tree).find(el => el.type === 'motion-dropdown').props.value, 'system');
  assert.match(all(tree).find(el => el.props.role === 'alert').props.children, /disk write failed/);
  all(tree).find(el => el.type === 'motion-dropdown').props.onChange('reduced');
  settle.resolve({ config: { motion_preference: 'reduced' } });
  await new Promise(setImmediate);
  tree = render();
  assert.equal(all(tree).find(el => el.type === 'motion-dropdown').props.value, 'reduced');
  assert.equal(all(tree).find(el => el.type === 'motion-dropdown').props.disabled, false);
  for (const dispose of effects) dispose();
});
