import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { posix } from 'node:path';
import test from 'node:test';
import vm from 'node:vm';
import { JSDOM } from 'jsdom';

// Real React lifecycle in an in-memory DOM. No browser, HWND or GPU is started.
const dom = new JSDOM('<!doctype html><html><body></body></html>', { url: 'http://localhost' });
for (const name of ['window', 'document', 'HTMLElement', 'Element', 'Node', 'Event', 'CustomEvent', 'MutationObserver']) globalThis[name] = dom.window[name];
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const React = require('react');
const { act } = React;
const { createRoot } = require('react-dom/client');
const ts = require('typescript');
const base = new URL('../', import.meta.url);
const h = React.createElement;
const defer = () => { let resolve; const promise = new Promise(yes => { resolve = yes; }); return { promise, resolve }; };
const clone = value => value == null ? value : JSON.parse(JSON.stringify(value));
const hidden = () => ({ mode: 'hidden', visible: false, placement: null });
const sourceA = 'C:\\test\\A.litematic', sourceB = 'C:\\test\\B.litematic';
// Optional immutable baseline reads Git blobs in memory, without another checkout.
const sourceRef = process.env.NOVA_VISIBILITY_SOURCE_REF;

async function fixture(run) {
  let current = null, sequence = 0, originGate = null, snapshotGate = null, presentGate = null, startGate = null;
  let origin = { x: 100, y: 200 }, rect = { left: 20, top: 30, right: 420, bottom: 330, width: 400, height: 300 };
  let visible = true, minimized = false, docVisible = true;
  const calls = [], nativeListeners = new Set(), observers = new Set(), cache = new Map();
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1600 });
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: 900 });
  Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 1 });
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => docVisible ? 'visible' : 'hidden' });
  Object.defineProperty(document, 'hidden', { configurable: true, get: () => !docVisible });
  const originalBounds = HTMLElement.prototype.getBoundingClientRect;
  HTMLElement.prototype.getBoundingClientRect = function () { return this.dataset.preview ? rect : originalBounds.call(this); };
  globalThis.ResizeObserver = class {
    constructor(fn) { this.fn = fn; }
    observe() { observers.add(this.fn); }
    disconnect() { observers.delete(this.fn); }
  };
  const onNative = async fn => { nativeListeners.add(fn); return () => nativeListeners.delete(fn); };
  const nativeWindow = {
    innerPosition: async () => originGate ? originGate.promise : origin,
    isVisible: async () => visible,
    isMinimized: async () => minimized,
    onMoved: onNative, onResized: onNative, onScaleChanged: onNative, onFocusChanged: onNative,
    onCloseRequested: onNative, onDestroyed: onNative,
    listen: async (_event, fn) => onNative(fn),
  };
  const invoke = async (command, args) => {
    calls.push({ command, ...clone(args) });
    if (command === 'application_quick_preview_start') {
      if (startGate) return startGate.promise;
      current = { previewId: `quick-${++sequence}`, sourcePath: args.input.sourcePath, renderMode: 'fast_experimental',
        state: 'ready', presentation: hidden(), visible: false, presentationRevision: 0, error: null };
      return clone(current);
    }
    if (command === 'application_quick_preview_snapshot') return snapshotGate ? snapshotGate.promise : clone(current);
    if (command === 'application_quick_preview_present') {
      const held = args.intent.visible && presentGate;
      if (held) { presentGate = null; await held.promise; }
      if (current?.previewId === args.previewId && (args.revision == null || args.revision >= current.presentationRevision)) {
        current = { ...current, presentation: clone(args.intent), visible: args.intent.visible,
          presentationRevision: args.revision ?? current.presentationRevision };
      }
      return clone(current);
    }
    if (command === 'application_quick_preview_close') { current = { ...current, state: 'closed', visible: false, presentation: hidden() }; return; }
    throw Error(`Unexpected IPC: ${command}`);
  };
  function load(path) {
    if (cache.has(path)) return cache.get(path);
    const source = sourceRef
      ? execFileSync('git', ['show', `${sourceRef}:desktop-nova/${path}`], { cwd: base, encoding: 'utf8' })
      : readFileSync(new URL(path, base), 'utf8');
    const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.React, esModuleInterop: true } });
    const module = { exports: {} };
    vm.runInThisContext(`(function(require,module,exports){${compiled.outputText}\n})`, { filename: path })(name => {
      if (name.endsWith('/facade')) return facade;
      if (name === './tauri') return { invoke };
      if (name === '@tauri-apps/api/window') return { getCurrentWindow: () => nativeWindow };
      if (!name.startsWith('.')) return require(name);
      const relative = posix.join(posix.dirname(path), name);
      const resolved = [relative, `${relative}.ts`, `${relative}.tsx`].find(candidate => existsSync(new URL(candidate, base)));
      assert.ok(resolved, `Missing module ${name} from ${path}`);
      return load(resolved);
    }, module, module.exports);
    cache.set(path, module.exports);
    return module.exports;
  }
  const api = load('src/platform/quickPreview.ts');
  const facade = { ...api, ensureApplicationRenderSubscription: async () => {},
    restoreApplicationRenderPresentationForRoute: async () => false,
    suspendApplicationRenderPresentationForRoute: async () => false,
    updateApplicationRenderOverlayPlacement: async () => false,
    updateApplicationRenderPresentation: async () => { throw Error('Quick must not use Full presentation'); } };
  const { useQuickPreview } = load('ui/windows/main/useQuickPreview.ts');
  let control;
  function PreviewPage({ source, blocked }) {
    control = useQuickPreview(source, blocked);
    return h('div', { ref: control.hostRef, 'data-preview': 'host' });
  }
  const host = document.createElement('div'); document.body.append(host);
  const renderer = createRoot(host);
  const settle = async () => { await act(async () => { await new Promise(setImmediate); await new Promise(setImmediate); }); };
  const render = async (page = 'properties', source = sourceA, blocked = false) => {
    await act(async () => renderer.render(page === 'away' ? h('div', null, '材料') : h(PreviewPage, { key: page, source, blocked })));
    await settle();
  };
  const event = async name => {
    await act(async () => { (name === 'visibilitychange' ? document : window).dispatchEvent(new Event(name));
      await new Promise(resolve => setTimeout(resolve, 55)); });
    await settle();
  };
  const nativeEvent = async () => {
    await act(async () => { for (const fn of [...nativeListeners]) fn({ payload: {} });
      await new Promise(resolve => setTimeout(resolve, 55)); });
    await settle();
  };
  const getState = () => clone(current);
  const presentations = () => calls.filter(call => call.command === 'application_quick_preview_present');
  const assertNoDestruction = expectedStarts => {
    assert.equal(calls.filter(call => call.command === 'application_quick_preview_start').length, expectedStarts);
    assert.ok(calls.every(call => !/close|build|render_start/.test(call.command)), 'Only Presentation may change');
  };
  try {
    await run({ api, render, settle, event, nativeEvent, getState, calls, presentations, assertNoDestruction,
      control: () => control,
      setRect(value, dpr = 1) { rect = { ...value, right: value.left + value.width, bottom: value.top + value.height }; Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: dpr }); },
      setOrigin(value) { origin = value; }, setOriginGate(value) { originGate = value; },
      setPresentGate(value) { presentGate = value; }, setSnapshotGate(value) { snapshotGate = value; },
      setStartGate(value) { startGate = value; },
      setDocumentVisible(value) { docVisible = value; }, setMinimized(value) { minimized = value; }, setVisible(value) { visible = value; },
      setCurrent(value) { current = value; },
      async start() { await act(async () => control.start()); await settle(); },
      async hide() { await act(async () => control.hide()); await settle(); },
    });
  } finally {
    await act(async () => renderer.unmount()); await settle(); host.remove();
    HTMLElement.prototype.getBoundingClientRect = originalBounds;
    delete globalThis.ResizeObserver;
  }
}

test('React 切页卸载立即隐藏 Presentation，返回使用新容器与 DPI，不重建 Viewer', async () => fixture(async f => {
  await f.render(); await f.start(); assert.equal(f.getState().visible, true);
  const id = f.getState().previewId;
  await f.render('away'); assert.equal(f.getState().visible, false);
  f.setOrigin({ x: 500, y: 700 }); f.setRect({ left: 40, top: 60, width: 500, height: 350 }, 1.5);
  await f.render();
  assert.equal(f.getState().previewId, id); assert.equal(f.getState().visible, true);
  assert.deepEqual(f.getState().presentation.placement, { x: 560, y: 790, width: 750, height: 525, dpiScale: 1.5, focused: false });
  f.assertNoDestruction(1);
}));

test('投影 A 切换 B 只隐藏旧 A；返回 A 复用同一快速 Viewer', async () => fixture(async f => {
  await f.render(); await f.start(); const id = f.getState().previewId;
  await f.render('properties', sourceB); assert.equal(f.getState().visible, false);
  await f.render('properties', sourceA); assert.equal(f.getState().previewId, id); assert.equal(f.getState().visible, true);
  f.assertNoDestruction(1);
}));

test('显式隐藏不会被切页、尺寸变化、文档恢复重新显示', async () => fixture(async f => {
  await f.render(); await f.start(); await f.hide();
  await f.render('away'); await f.render();
  f.setRect({ left: 80, top: 90, width: 450, height: 320 }, 2); await f.nativeEvent();
  f.setDocumentVisible(false); await f.event('visibilitychange'); f.setDocumentVisible(true); await f.event('visibilitychange');
  assert.equal(f.getState().visible, false); f.assertNoDestruction(1);
}));

test('document hidden/pagehide 隐藏，pageshow 恢复最新位置；最小化与主窗口隐藏保持隐藏', async () => fixture(async f => {
  await f.render(); await f.start();
  f.setDocumentVisible(false); await f.event('visibilitychange'); assert.equal(f.getState().visible, false);
  f.setDocumentVisible(true); await f.event('visibilitychange'); assert.equal(f.getState().visible, true);
  await f.event('pagehide'); assert.equal(f.getState().visible, false);
  await f.event('pageshow'); assert.equal(f.getState().visible, true);
  f.setMinimized(true); await f.nativeEvent(); assert.equal(f.getState().visible, false);
  f.setOrigin({ x: 800, y: 900 }); f.setRect({ left: 10, top: 15, width: 300, height: 220 }, 2);
  f.setMinimized(false); await f.nativeEvent(); assert.equal(f.getState().visible, true);
  assert.equal(f.getState().presentation.placement.x, 820);
  f.setVisible(false); await f.nativeEvent(); assert.equal(f.getState().visible, false);
  f.setVisible(true); await f.nativeEvent(); assert.equal(f.getState().visible, true);
  f.assertNoDestruction(1);
}));

test('在途坐标结果在路由卸载后失效，不让已隐藏 HWND 再显示', async () => fixture(async f => {
  await f.render(); await f.start();
  const gate = defer(); f.setOriginGate(gate);
  await f.event('resize'); await f.render('away');
  assert.equal(f.getState().visible, false);
  const boundary = f.presentations().length;
  gate.resolve({ x: 900, y: 900 }); await f.settle();
  assert.equal(f.getState().visible, false);
  assert.equal(f.presentations().slice(boundary).filter(call => call.intent.visible).length, 0);
  f.assertNoDestruction(1);
}));

test('在途 placement/restore 不能阻塞离页 hide，也不能以旧 revision 重新显示', async () => fixture(async f => {
  await f.render(); await f.start();
  const gate = defer(); f.setPresentGate(gate);
  f.setRect({ left: 35, top: 45, width: 410, height: 310 });
  await f.event('resize'); await f.render('away');
  try {
    assert.equal(f.presentations().at(-1).intent.visible, false, 'Hide must reach Host before an old presentation resolves');
    assert.equal(f.getState().visible, false);
  } finally { gate.resolve(); await f.settle(); }
  assert.equal(f.getState().visible, false);
  const restoreGate = defer(); f.setPresentGate(restoreGate);
  await f.render(); await f.render('away'); restoreGate.resolve(); await f.settle();
  assert.equal(f.getState().visible, false);
  const requests = f.presentations();
  assert.ok(requests.every(call => Number.isSafeInteger(call.revision) && call.revision > 0));
  f.assertNoDestruction(1);
}));

test('迟到启动 Ready 与旧快照在离页之后不得自动呈现', async () => fixture(async f => {
  await f.render();
  const gate = defer(); f.setStartGate(gate);
  let starting; await act(async () => { starting = f.control().start(); });
  await f.render('away');
  const ready = { previewId: 'late-ready', sourcePath: sourceA, renderMode: 'fast_experimental', state: 'ready',
    presentation: hidden(), visible: false, presentationRevision: 0, error: null };
  f.setCurrent(ready); gate.resolve(clone(ready)); await act(async () => starting); await f.settle();
  assert.equal(f.getState().visible, false); assert.equal(f.presentations().filter(call => call.intent.visible).length, 0);
  f.setStartGate(null); await f.render(); await f.start();
  const old = f.getState(), snapshotGate = defer(); f.setSnapshotGate(snapshotGate);
  let reading; await act(async () => { reading = f.api.refreshQuickPreview(); });
  await f.render('away'); snapshotGate.resolve(old); await act(async () => reading); await f.settle();
  assert.equal(f.api.getQuickPreviewState().visible, false); assert.equal(f.getState().visible, false);
  f.assertNoDestruction(1);
}));

test('属性与渲染页快速交接只有最新 lease 呈现，新页位置覆盖旧页', async () => fixture(async f => {
  await f.render(); await f.start();
  const gate = defer(); f.setOriginGate(gate); await f.event('resize');
  await f.render('render'); await f.render('away');
  f.setOriginGate(null); f.setOrigin({ x: 300, y: 400 }); f.setRect({ left: 50, top: 20, width: 320, height: 200 }, 2);
  await f.render('properties'); gate.resolve({ x: 999, y: 999 }); await f.settle();
  assert.equal(f.getState().visible, true);
  assert.equal(f.getState().presentation.placement.x, 400); assert.equal(f.getState().presentation.placement.dpiScale, 2);
  f.assertNoDestruction(1);
}));

test('pagehide 出现在启动阶段时，迟到 Ready 维持隐藏；pageshow 才按新位置呈现', async () => fixture(async f => {
  await f.render(); const gate = defer(); f.setStartGate(gate);
  let starting; await act(async () => { starting = f.control().start(); });
  await f.event('pagehide');
  const ready = { previewId: 'starting-hidden', sourcePath: sourceA, renderMode: 'fast_experimental', state: 'ready',
    presentation: hidden(), visible: false, presentationRevision: 0, error: null };
  f.setCurrent(ready); gate.resolve(clone(ready)); await act(async () => starting); await f.settle();
  assert.equal(f.getState().visible, false);
  assert.equal(f.presentations().filter(call => call.intent.visible).length, 0);
  f.setOrigin({ x: 600, y: 700 }); f.setRect({ left: 25, top: 40, width: 350, height: 240 }, 2);
  await f.event('pageshow');
  assert.equal(f.getState().visible, true);
  assert.equal(f.getState().presentation.placement.x, 650); f.assertNoDestruction(1);
}));

test('starting 中父窗口最小化但 document visible，Ready 不显示；原生恢复使用新位置及 DPI', async () => fixture(async f => {
  await f.render(); const gate = defer(); f.setStartGate(gate);
  let starting; await act(async () => { starting = f.control().start(); });
  assert.equal(document.visibilityState, 'visible');
  f.setMinimized(true); await f.nativeEvent();
  const ready = { previewId: 'minimized-ready', sourcePath: sourceA, renderMode: 'fast_experimental', state: 'ready',
    presentation: hidden(), visible: false, presentationRevision: 0, error: null };
  f.setCurrent(ready); gate.resolve(clone(ready)); await act(async () => starting); await f.settle();
  assert.equal(f.getState().visible, false);
  assert.equal(f.presentations().filter(call => call.intent.visible).length, 0);
  f.setOrigin({ x: 400, y: 500 }); f.setRect({ left: 30, top: 45, width: 320, height: 240 }, 1.5);
  f.setMinimized(false); await f.nativeEvent();
  assert.equal(f.getState().visible, true);
  assert.deepEqual(f.getState().presentation.placement, { x: 445, y: 568, width: 480, height: 360, dpiScale: 1.5, focused: false });
  f.assertNoDestruction(1);
}));
