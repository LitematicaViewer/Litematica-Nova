import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { JSDOM } from 'jsdom';
import React from 'react';
import { createRoot } from 'react-dom/client';

// Real React effects and DOM image events, with no browser, native window or GPU.
const source = readFileSync(new URL('../ui/windows/main/pages/flake/LayerCanvas.tsx', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { reportDiagnostics: true, compilerOptions: {
  module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React, esModuleInterop: true,
} });
assert.deepEqual(compiled.diagnostics.filter(item => item.category === ts.DiagnosticCategory.Error), []);
const scene = (overrides = {}) => ({ sceneHandle: 'scene-1', generation: 1, sourceIdentity: 'source-1', resourceIdentity: 'pack-1', visualGeneration: 1,
  meta: { size_x: 100, size_y: 20, size_z: 100, min_x: 0, min_y: -10, min_z: 0 }, origin: null, ...overrides });
const diagnostics = { readyStates: 2, transparentStates: 1, missingStates: 0, failedStates: 0, retryableStates: 0, issues: [] };

async function fixture(run) {
  const dom = new JSDOM('<div id="root"></div>');
  const names = ['window', 'document', 'HTMLElement', 'Element', 'Node', 'Event', 'CustomEvent', 'MouseEvent', 'getComputedStyle', 'ResizeObserver', 'requestAnimationFrame', 'cancelAnimationFrame', 'IS_REACT_ACT_ENVIRONMENT'];
  const previous = Object.fromEntries(names.map(name => [name, globalThis[name]]));
  for (const name of ['window', 'document', 'HTMLElement', 'Element', 'Node', 'Event', 'CustomEvent', 'MouseEvent']) globalThis[name] = name === 'window' ? dom.window : dom.window[name];
  globalThis.getComputedStyle = dom.window.getComputedStyle;
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  let factor = 1, serial = 0, frameId = 0, invalidateFailure = null, visualGeneration = 1;
  const frames = new Map(), pixels = new Map(), requests = [], cancelled = [], invalidations = [], states = [], hits = [], hovers = [], changed = [];
  globalThis.requestAnimationFrame = callback => { const id = ++frameId; frames.set(id, callback); return id; };
  globalThis.cancelAnimationFrame = id => frames.delete(id);
  globalThis.ResizeObserver = class { observe() {} disconnect() {} };
  Object.defineProperties(dom.window.HTMLElement.prototype, {
    clientWidth: { configurable: true, get: () => 160 }, clientHeight: { configurable: true, get: () => 160 },
  });
  dom.window.HTMLElement.prototype.getBoundingClientRect = () => ({ left: 10, top: 20, width: 160 * factor, height: 160 * factor, right: 10 + 160 * factor, bottom: 20 + 160 * factor });
  dom.window.HTMLElement.prototype.setPointerCapture = () => {};
  Object.defineProperties(dom.window.HTMLImageElement.prototype, {
    complete: { configurable: true, get() { return pixels.get(this.src)?.complete ?? false; } },
    naturalWidth: { configurable: true, get() { return pixels.get(this.src)?.width ?? 0; } },
    naturalHeight: { configurable: true, get() { return pixels.get(this.src)?.height ?? 0; } },
  });
  const backend = {
    nextLayerRequestId: () => ++serial,
    requestLayerView: (scene, request) => new Promise((resolve, reject) => requests.push({ scene, request, resolve, reject })),
    cancelLayerView: async (scene, id) => { cancelled.push([scene.sceneHandle, id]); },
    invalidateLayerVisuals: async value => { invalidations.push(value); if (invalidateFailure) throw Error(invalidateFailure);
      return { sceneHandle: value.sceneHandle, generation: value.generation, resourceIdentity: value.resourceIdentity, visualGeneration: ++visualGeneration }; },
    hitTestLayer: async (scene, request, x, z) => { hits.push({ x, z, request }); return { sceneHandle: scene.sceneHandle, generation: scene.generation, requestId: request.requestId, hit: { x, z, y: request.y } }; },
  };
  const exports = {};
  vm.runInNewContext(compiled.outputText, { exports, require: name => {
    if (name === 'react') return React;
    if (name.endsWith('/layerView')) return backend;
    if (name.endsWith('/i18n') && name.includes('services')) return { t: (key, params) => `${key}${params ? ` ${JSON.stringify(params)}` : ''}` };
    if (name.endsWith('/i18n')) return { useI18n: () => ({ locale: 'en-US' }) };
    if (name.endsWith('/button')) return { Button: ({ children, size, ...props }) => React.createElement('button', props, children) };
    throw Error(`Unexpected dependency ${name}`);
  }, requestAnimationFrame: globalThis.requestAnimationFrame, cancelAnimationFrame: globalThis.cancelAnimationFrame,
  ResizeObserver: globalThis.ResizeObserver, window: dom.window, console });
  const ref = React.createRef();
  let props = { scene: scene(), layerY: 0, onionDepth: 0, showStateHints: true, interactionTool: 'debug-stick', editedBlockStates: {},
    initialView: { scale: 16, offset: { x: 0, y: 0 } }, originMarker: null, measurements: [],
    onViewState: (image, error, status) => states.push({ image, error, status }), onScaleChange() {},
    onResourceChanged: () => changed.push(true), onHoverBlock: block => hovers.push(block), onBlockClick() {}, onBlockRightClick() {},
    onMarkerContextMenu() {}, onOriginChange() {}, onAddMeasurement() {}, onDeleteMeasurementAt() {},
  };
  const root = createRoot(dom.window.document.getElementById('root'));
  const render = async (next = {}) => { props = { ...props, ...next }; await React.act(async () => { root.render(React.createElement(exports.LayerCanvas, { ...props, ref })); }); };
  const flush = async () => { await React.act(async () => { const pending = [...frames.values()]; frames.clear(); for (const callback of pending) callback(0); }); };
  const image = (entry, url, extra = {}) => ({ sceneHandle: entry.scene.sceneHandle, generation: entry.scene.generation, resourceIdentity: entry.scene.resourceIdentity,
    visualGeneration, requestId: entry.request.requestId, y: entry.request.y, worldY: entry.request.y - 10,
    viewRect: entry.request.viewRect, imageUrl: url, pixelWidth: 16, pixelHeight: 16, blockCount: 4, missingVisuals: 0, visualDiagnostics: diagnostics, ...extra });
  const resolve = async (entry, url, extra) => { await React.act(async () => { entry.resolve(image(entry, url, extra)); }); };
  const load = async (element, valid = true) => { await React.act(async () => {
    pixels.set(element.src, { complete: true, width: valid ? 16 : 0, height: valid ? 16 : 0 });
    element.dispatchEvent(new dom.window.Event(valid ? 'load' : 'error', { bubbles: false }));
  }); };
  const canvas = () => dom.window.document.querySelector('.flake-canvas');
  const pending = () => dom.window.document.querySelector('.flake-layer-image--pending');
  const visible = () => dom.window.document.querySelector('.flake-layer-image:not(.flake-layer-image--pending)');
  const clickRetry = async () => { await React.act(async () => { [...dom.window.document.querySelectorAll('button')].find(button => button.textContent === 'flake.retryVisuals').click(); }); };
  try {
    await render(); await flush();
    await run({ dom, ref, render, flush, image, resolve, load, requests, states, cancelled, invalidations, pixels, hits, hovers, changed,
      canvas, pending, visible, clickRetry, setFactor: value => { factor = value; }, failInvalidation: value => { invalidateFailure = value; } });
  } finally {
    await React.act(async () => root.unmount()); dom.window.close();
    for (const name of names) globalThis[name] = previous[name];
  }
}

test('Rust receipt is not ready until actual image decode; cached completion also commits', async () => fixture(async f => {
  await f.resolve(f.requests.at(-1), 'https://nova.test/first');
  assert.equal(f.states.at(-1).status, 'loading'); assert.ok(f.pending()); assert.equal(f.visible(), null);
  await f.load(f.pending()); assert.equal(f.states.at(-1).status, 'ready'); assert.equal(f.visible().src, 'https://nova.test/first');
  await f.render({ layerY: 1 }); await f.flush();
  f.pixels.set('https://nova.test/cached', { complete: true, width: 16, height: 16 });
  await f.resolve(f.requests.at(-1), 'https://nova.test/cached');
  assert.equal(f.states.at(-1).status, 'ready'); assert.equal(f.states.at(-1).image.y, 1);
}));

test('late layer/generation images never replace the newest displayed layer', async () => fixture(async f => {
  const old = f.requests.at(-1);
  await f.render({ layerY: 1 }); await f.flush(); const next = f.requests.at(-1);
  await f.resolve(old, 'https://nova.test/old'); assert.equal(f.pending(), null);
  await f.resolve(next, 'https://nova.test/next'); const staleElement = f.pending();
  await f.render({ scene: scene({ sceneHandle: 'scene-2', generation: 2, resourceIdentity: 'pack-2' }) }); await f.flush();
  await f.load(staleElement); assert.equal(f.visible(), null);
  await f.resolve(f.requests.at(-1), 'https://nova.test/current'); await f.load(f.pending());
  assert.equal(f.states.at(-1).image.sceneHandle, 'scene-2'); assert.equal(f.visible().src, 'https://nova.test/current');
  assert.ok(f.cancelled.some(([, id]) => id === old.request.requestId));
  assert.equal(f.ref.current.viewState().scale, 16, 'resource replacement preserves the UI view');
}));

test('decode and invalidation failures remain visible and recover on explicit retry', async () => fixture(async f => {
  await f.resolve(f.requests.at(-1), 'https://nova.test/broken'); await f.load(f.pending(), false);
  assert.equal(f.states.at(-1).status, 'failed');
  f.failInvalidation('temporary resource read failure');
  const previousRequests = f.requests.length;
  await f.clickRetry(); await f.flush();
  assert.match(f.states.at(-1).error, /temporary resource read failure/);
  assert.equal(f.requests.length, previousRequests, 'failed invalidation is not hidden by an automatic cached query');
  f.failInvalidation(null); await f.clickRetry(); await f.flush();
  await f.resolve(f.requests.at(-1), 'https://nova.test/recovered'); await f.load(f.pending());
  assert.equal(f.states.at(-1).status, 'ready'); assert.equal(f.invalidations.length, 2);
}));

test('current resource/session errors request a reopen; obsolete errors are ignored', async () => fixture(async f => {
  const obsolete = f.requests.at(-1);
  await f.render({ layerY: 2 }); await f.flush();
  await React.act(async () => obsolete.reject(Error('LayerResourceChanged: old')));
  assert.equal(f.changed.length, 0);
  await React.act(async () => f.requests.at(-1).reject(Error('LayerSceneExpired: restart')));
  assert.equal(f.changed.length, 1); assert.equal(f.states.at(-1).status, 'failed');
}));

test('retry invalidation rejects older visual generations even with a current request id', async () => fixture(async f => {
  await f.resolve(f.requests.at(-1), 'https://nova.test/partial-before', { missingVisuals: 1 }); await f.load(f.pending());
  await f.clickRetry(); await f.flush();
  await f.resolve(f.requests.at(-1), 'https://nova.test/obsolete-visual', { visualGeneration: 1 });
  assert.equal(f.states.at(-1).status, 'failed');
  assert.match(f.states.at(-1).error, /flake.imageIdentityFailed/);
  assert.equal(f.visible(), null);
}));

test('a queued hover for the old layer does not issue a hit on the new layer', async () => fixture(async f => {
  await f.resolve(f.requests.at(-1), 'https://nova.test/hover'); await f.load(f.pending());
  await React.act(async () => f.canvas().dispatchEvent(new f.dom.window.MouseEvent('pointermove', { bubbles: true, clientX: 50, clientY: 60 })));
  await f.render({ layerY: 3 }); await f.flush();
  assert.equal(f.hits.length, 0);
  assert.equal(f.requests.at(-1).request.y, 3);
}));

test('batched wheel input and mixed DPI use CSS coordinates exactly once', async () => fixture(async f => {
  for (const factor of [.5, 1, 1.5, 3, 5]) {
    f.setFactor(factor);
    await React.act(async () => { f.ref.current.zoomTo(8); }); await f.flush();
    if (!f.visible()) { await f.resolve(f.requests.at(-1), `https://nova.test/dpi-${factor}`); await f.load(f.pending()); }
    const view = f.ref.current.viewState();
    await React.act(async () => { f.canvas().dispatchEvent(new f.dom.window.MouseEvent('click', { bubbles: true,
      clientX: 10 + (view.offset.x + 2.25 * view.scale) * factor,
      clientY: 20 + (view.offset.y + 3.25 * view.scale) * factor })); });
    assert.equal(f.hits.at(-1).x, 2); assert.equal(f.hits.at(-1).z, 3);
  }
  await f.render({ interactionTool: 'hand' });
  const beforePan = f.ref.current.viewState();
  await React.act(async () => {
    f.canvas().dispatchEvent(new f.dom.window.MouseEvent('pointerdown', { bubbles: true, button: 0, clientX: 10, clientY: 20 }));
    f.canvas().dispatchEvent(new f.dom.window.MouseEvent('pointermove', { bubbles: true, clientX: 60, clientY: 70 }));
    f.canvas().dispatchEvent(new f.dom.window.MouseEvent('pointerup', { bubbles: true, clientX: 60, clientY: 70 }));
  });
  assert.equal(f.ref.current.viewState().offset.x, beforePan.offset.x + 10);
  assert.equal(f.ref.current.viewState().offset.y, beforePan.offset.y + 10);
  await React.act(async () => {
    for (let i = 0; i < 2; i++) f.canvas().dispatchEvent(new f.dom.window.WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -1, clientX: 10, clientY: 20 }));
  });
  assert.equal(f.ref.current.viewState().scale, 32, 'two input increments are not collapsed into one old-scale update');
}));

test('a malformed current receipt reports failure instead of loading forever', async () => fixture(async f => {
  await f.resolve(f.requests.at(-1), 'https://nova.test/wrong', { requestId: 9999 });
  assert.equal(f.states.at(-1).status, 'failed');
  assert.match(f.states.at(-1).error, /flake.imageIdentityFailed/);
  assert.equal(f.pending(), null);
}));

test('backend diagnostics expose real failure keys without treating transparency as failure', async () => fixture(async f => {
  await f.resolve(f.requests.at(-1), 'https://nova.test/partial', { missingVisuals: 1, visualDiagnostics: { ...diagnostics,
    missingStates: 1, issues: [{ state: 'mod:machine[facing=north]', kind: 'missing_source', resourceKey: 'mod:textures/machine.png', message: 'resource not found in selected pack', retryable: false, blockCount: 9 }] } });
  await f.load(f.pending());
  assert.equal(f.states.at(-1).status, 'partial');
  assert.match(f.dom.window.document.body.textContent, /mod:textures\/machine.png/);
  assert.match(f.dom.window.document.body.textContent, /resource not found in selected pack/);
  assert.match(f.dom.window.document.body.textContent, /"transparent":1/);
  assert.ok(f.dom.window.document.querySelector('details'));
}));
