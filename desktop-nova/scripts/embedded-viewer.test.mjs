import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import test from "node:test";
import ts from "typescript";

const settle = async () => { await new Promise(setImmediate); await new Promise(setImmediate); };
const defer = () => { let resolve; const promise = new Promise((yes) => { resolve = yes; }); return { promise, resolve }; };
function loadModule(path, require, globals = {}) {
  const result = ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS }, reportDiagnostics: true,
  });
  assert.equal(result.diagnostics.length, 0);
  const context = { exports: {}, require, console, ...globals };
  vm.runInNewContext(result.outputText, context, { filename: path });
  return context.exports;
}

test("shadcn HUD cuts only borders, numbers and item images; slot input keeps the empty centre", () => {
  const { sceneOverlayGeometry } = loadModule("../ui/windows/main/embeddedViewerGeometry.ts", () => ({}), {
    getComputedStyle: element => ({ borderTopWidth: element.border }),
  });
  const element = (x, y, width, height, children = [], border = "1px") => ({
    getBoundingClientRect: () => ({ left: x, top: y, right: x + width, bottom: y + height }),
    querySelectorAll: () => children, border,
  });
  const slots = Array.from({length: 9}, (_, i) => {
    const x = i * 34;
    return element(x, 40, 32, 32, [element(x + 3, 43, 6, 10), ...(i === 2 ? [element(x + 9, 49, 14, 14)] : [])], i === 0 ? "2px" : "1px");
  });
  const geometry = (surfaces, outlined, x = 100) => sceneOverlayGeometry(surfaces, slots, outlined,
    { x, y: 200, width: 608, height: 200 }, { x: 100, y: 200 }, 2);
  const hit = (regions, x, y) => regions.some(r => r.x <= x && r.y <= y && x < r.x + r.width && y < r.y + r.height);
  const { uiRegions, hotbarRegions } = geometry([], true);
  assert.equal(uiRegions.length, 46);
  assert.equal(hotbarRegions.length, 9);
  assert.equal(hit(hotbarRegions, 32, 112), true);
  assert.equal(hit(uiRegions, 32, 112), false);
  assert.equal(hit(uiRegions, 2, 112), true, "selected two-pixel stroke");
  assert.equal(hit(uiRegions, 70, 112), false, "idle one-pixel stroke");
  assert.equal(hit(uiRegions, 8, 90), true, "number");
  assert.equal(hit(uiRegions, 168, 112), true, "item icon");
  assert.equal(hit(hotbarRegions, 66, 112), false, "gap remains native scene");
  assert.equal(geometry([], false).uiRegions.length, 9, "solid themes expose nine surfaces, not the dock");
  assert.equal(geometry([element(0, 0, 304, 100)], true).uiRegions.length, 1, "a dialog subsumes the ink below it");
  const clipped = geometry([], true, 170);
  assert.equal(clipped.hotbarRegions.length, 9);
  assert.equal(clipped.hotbarRegions[0].width, 0, "offscreen slots keep their original index");
  assert.equal(geometry(Array.from({length: 30}, (_, i) => element(i * 4, 5, 2, 2)), true).uiRegions.length <= 64, true);
});
const hidden = () => ({ mode: "hidden", visible: false, placement: null });
const operation = (presentation = hidden(), state = "viewer_prepared", operationId = "one") => ({
  operationId, source: { path: `${operationId}.litematic` }, displayMode: "full", presentation, state,
});
function fixture() {
  const commands = [];
  const sceneControls = [];
  let origin = { x: 100, y: 200 };
  let originGate;
  let focused = true;
  let nativeVisible = true;
  const nativeListeners = new Set();
  let nativeRender = () => {};
  const listeners = new Map();
  const timers = new Map();
  const observers = new Set();
  let timerId = 0;
  const host = { parentElement: null, querySelectorAll: () => [], getBoundingClientRect: () => ({ left: 20, top: 40, right: 420, bottom: 340, width: 400, height: 300 }) };
  const window = { devicePixelRatio: 1.5, innerWidth: 1280, innerHeight: 800, getComputedStyle: element => ({ overflowX: "visible", overflowY: "visible", backgroundColor: "rgba(0, 0, 0, 0)", opacity:"1", ...element.style }),
    setTimeout(fn) { timers.set(++timerId, fn); return timerId; },
    clearTimeout(id) { timers.delete(id); },
    addEventListener(name, fn) { if (!listeners.has(name)) listeners.set(name, new Set()); listeners.get(name).add(fn); },
    removeEventListener(name, fn) { listeners.get(name)?.delete(fn); },
  };
  const document = { visibilityState: "visible", hasFocus: () => focused, documentElement: {dataset: {theme: "shadcn"}}, querySelectorAll: () => [],
    addEventListener: window.addEventListener, removeEventListener: window.removeEventListener };
  const platform = loadModule("../src/platform/applicationRender.ts", (id) => {
    if (id === "@tauri-apps/api/event") return { listen: async () => () => {} };
    if (id === "./tauri") return { invoke: async (command, payload) => { commands.push({ command, ...JSON.parse(JSON.stringify(payload)) }); } };
    throw Error(id);
  });
  const nativeGeometry = {
    getApplicationWindowOrigin: async () => originGate ? originGate.promise : origin,
    getApplicationWindowVisibility: async () => nativeVisible,
    subscribeApplicationWindowGeometry: async (fn) => { nativeListeners.add(fn); return () => nativeListeners.delete(fn); },
  };
  const geometry = loadModule("../ui/windows/main/embeddedViewerGeometry.ts", () => nativeGeometry, { window, document, getComputedStyle: window.getComputedStyle });
  function mount(controller = null) {
    let cursor = 0;
    let effects = [];
    const hooks = [];
    const react = {
      useCallback(callback, deps) { const i = cursor++; if (!hooks[i] || deps.some((v,j) => v !== hooks[i].deps[j])) hooks[i] = {deps, value: callback}; return hooks[i].value; },
      useRef(value) { const i = cursor++; return hooks[i] ??= { current: value }; },
      useState(initial) { const i = cursor++; hooks[i] ??= { value: initial }; return [hooks[i].value, (value) => { hooks[i].value = typeof value === "function" ? value(hooks[i].value) : value; }]; },
      useEffect(callback, deps) {
        const i = cursor++;
        if (!hooks[i] || deps.some((value, j) => value !== hooks[i].deps[j])) {
          const previous = hooks[i];
          hooks[i] = { deps };
          effects.push(() => { previous?.cleanup?.(); hooks[i].cleanup = callback(); });
        }
      },
    };
    react.useLayoutEffect = react.useEffect;
    const module = loadModule("../ui/windows/main/useEmbeddedViewer.ts", (id) => {
      if (id === "react") return react;
      if (id === "../../i18n") return { useI18n: () => ({locale: "zh-CN"}) };
      if (id.endsWith("/services/i18n")) return { message: (key, params) => ({key, params}), localizeMessage: m => typeof m === "string" ? m : m.key };
      if (id.endsWith("/nativeWindowGeometry")) return nativeGeometry;
      if (id.endsWith("/sceneView")) return { updateSceneView: async ({controls}) => { sceneControls.push(controls); return {}; } };
      if (id.endsWith("facade")) return { ...platform, ensureApplicationRenderSubscription: async () => {} };
      if (id.endsWith("embeddedViewerGeometry")) return geometry;
      throw Error(id);
    }, { window, document, getComputedStyle: window.getComputedStyle, requestAnimationFrame: window.setTimeout, cancelAnimationFrame: window.clearTimeout,
      queueMicrotask,
      MutationObserver: class { observe() {} disconnect() {} },
      ResizeObserver: class { constructor(fn) { this.fn = fn; observers.add(fn); } observe() {} disconnect() { observers.delete(this.fn); } } });
    return {
      render(op, blocked = false) { nativeRender = () => this.render(op, blocked); cursor = 0; const value = controller
        ? module.usePresentationViewport(op ? { operationId: op.operationId, prepared: platform.isApplicationViewerPrepared(op), presentation: op.presentation } : null, blocked, controller)
        : module.useEmbeddedViewer(op, blocked); value.mountHost(host); while (effects.length) effects.shift()(); return value; },
      unmount() { hooks.forEach((hook) => hook.cleanup?.()); },
    };
  }
  function start(op = operation()) {
    const h = mount();
    h.render(op).requestPresentationWhenReady(op.operationId);
    h.render(op);
    return h;
  }
  return { commands, sceneControls, platform, geometry, host, mount, start, window, document,
    setOrigin(value) { origin = value; }, setOriginGate(value) { originGate = value; },
    setFocused(value) { focused = value; }, setNativeVisible(value) { nativeVisible = value; },
    event(name) { listeners.get(name)?.forEach(fn => fn()); }, nativeEvent(deferRender = false) { [...nativeListeners].forEach(fn => fn()); if (!deferRender) nativeRender(); },
    resize() { observers.forEach((fn) => fn()); },
    flush() { const pending = [...timers.values()]; timers.clear(); pending.forEach((fn) => fn()); },
  };
}

test("Application 在准备期排队最新原生位置，Ready 不再往返前端重复发 Show", async () => {
  const f = fixture(); const h = f.mount();
  h.render(operation(hidden(), "viewer_preparing")).requestPresentationWhenReady("one");
  h.render(operation(hidden(), "viewer_preparing")); await settle();
  assert.equal(f.commands.length, 1);
  assert.equal(f.commands[0].command, "application_render_update_presentation");
  assert.equal(f.commands[0].intent.mode, "embedded");
  assert.deepEqual(f.commands[0].intent.placement, { x: 130, y: 260, width: 600, height: 450, dpiScale: 1.5, focused: false });
  h.render(operation(f.commands[0].intent, "realtime_ready")); await settle();
  assert.equal(f.commands.length, 1, "Service already owns desired visibility before GPU Ready");
});

test("切页只隐藏并在新 host 恢复同一 operation，不释放 runtime", async () => {
  const f = fixture(); const h = f.start(); await settle();
  h.unmount(); await settle(); assert.equal(f.commands.at(-1).intent.visible, false);
  f.setOrigin({ x: 300, y: 400 });
  const second = f.mount(); second.render(operation({ ...f.commands.at(-1).intent })); await settle();
  assert.equal(f.commands.at(-1).intent.visible, true); assert.equal(f.commands.at(-1).intent.placement.x, 330);
  assert.ok(f.commands.every((entry) => entry.command === "application_render_update_presentation" && entry.operationId === "one"));
});

test("瞄准方块的 HUD 和提示只更新裁剪；真正打开菜单才释放一次原生输入", async () => {
  const f = fixture(), h = f.mount();
  let left = 140, popup = false;
  const hud = { contains: () => false, dataset:{}, hidden:false, closest:() => hud, getAnimations:() => [],
    getBoundingClientRect:() => ({left, right:left+80, top:60, bottom:90, width:80, height:30}) };
  const menu = {...hud, closest:() => null};
  f.host.querySelectorAll = selector => selector === "[data-scene-ui-region]" ? [hud] : [];
  f.document.querySelectorAll = selector => selector === "[data-native-ui-surface]" ? [popup ? menu : hud] : [];
  const refresh = async () => { f.resize(); f.flush(); await settle(); f.flush(); await settle(); };
  h.render(operation({mode:"embedded",visible:true,placement:{x:130,y:260,width:600,height:450,dpiScale:1.5,focused:false}}));
  await refresh();
  ++left; await refresh();
  assert.ok(f.sceneControls.length >= 2);
  assert.ok(f.sceneControls.every(control => !control.releaseCameraInput && !control.uiInputBlocked));
  popup = true; await refresh(); ++left; await refresh();
  assert.equal(f.sceneControls.filter(control => control.releaseCameraInput).length, 1);
  popup = false; ++left; await refresh();
  assert.equal(f.sceneControls.filter(control => control.releaseCameraInput).length, 1);
  h.unmount();
});

test("对话框隐藏页内原生窗口，关闭后恢复；Popup 不受影响", async () => {
  const f = fixture(); const h = f.start(); await settle();
  const visible = operation(f.commands.at(-1).intent);
  h.render(visible, true); await settle(); assert.equal(f.commands.at(-1).intent.visible, false);
  h.render(operation(f.commands.at(-1).intent), false); await settle(); assert.equal(f.commands.at(-1).intent.visible, true);
  await f.platform.updateApplicationRenderPresentation("one", { mode: "popup", visible: true, placement: null });
  const count = f.commands.length; h.render(operation(f.commands.at(-1).intent), true); await settle(); h.unmount(); await settle();
  assert.equal(f.commands.length, count);
});

test("初始Hidden后实际嵌入，切换来源或加载模式立即隐藏旧lease而不释放场景", async () => {
  for (const next of [null, operation(hidden(), "viewer_preparing", "two")]) {
    const f = fixture(); const h = f.start(operation(hidden(), "viewer_preparing")); await settle();
    const embedded = f.commands.at(-1).intent;
    h.render(operation(embedded, "realtime_ready")); await settle();
    assert.equal(f.commands.at(-1).intent.visible, true);
    const before = f.commands.length;
    h.render(next); await settle();
    const delta = f.commands.slice(before);
    assert.equal(delta.length, 1);
    assert.equal(delta[0].operationId, "one");
    assert.equal(delta[0].intent.visible, false);
    assert.equal(f.commands.filter(command => command.command !== "application_render_update_presentation").length, 0);
  }
});

test("原生位置、DPI 和焦点更新复用 placement；失焦不是隐藏", async () => {
  const f = fixture(); const h = f.start(); await settle();
  h.render(operation(f.commands.at(-1).intent)); await settle();
  f.window.devicePixelRatio = 2; f.setOrigin({ x: 500, y: 700 }); f.nativeEvent(); f.flush(); await settle();
  assert.deepEqual(f.commands.at(-1).intent.placement, { x: 540, y: 780, width: 800, height: 600, dpiScale: 2, focused: false });
  f.setFocused(false); f.event("blur"); f.flush(); await settle();
  assert.equal(f.commands.at(-1).intent.visible, true); assert.equal(f.commands.at(-1).intent.placement.focused, false);
});

test("迟到的位置结果不覆盖新的 Popup 或显式 Hide", async () => {
  const f = fixture(); const h = f.start(); await settle();
  h.render(operation(f.commands.at(-1).intent)); await settle();
  const gate = defer(); f.setOriginGate(gate); f.resize(); f.flush();
  await f.platform.updateApplicationRenderPresentation("one", { mode: "popup", visible: true, placement: null });
  gate.resolve({ x: 1000, y: 1000 }); await settle();
  assert.equal(f.commands.at(-1).intent.mode, "popup");
  const hiddenOverlay = { mode: "embedded", visible: false, placement: null };
  await f.platform.updateApplicationRenderPresentation("one", hiddenOverlay);
  h.render(operation(hiddenOverlay)); await settle(); const count = f.commands.length;
  f.resize(); f.flush(); await settle(); assert.equal(f.commands.length, count);
});

test("卸载期间的迟到 show 不重新显示原生窗口", async () => {
  const f = fixture(); const h = f.start(); await settle();
  const value = h.render(operation(f.commands.at(-1).intent)); await settle();
  const gate = defer(); f.setOriginGate(gate); const show = value.showOverlay(); h.unmount();
  gate.resolve({ x: 0, y: 0 }); await show; await settle();
  assert.equal(f.commands.at(-1).intent.visible, false);
});

test("短窗口没有可用 host 时先隐藏，恢复尺寸后显示且不重建", async () => {
  const f = fixture(); const h = f.start(); await settle();
  h.render(operation(f.commands.at(-1).intent)); await settle();
  f.host.getBoundingClientRect = () => ({ left: 20, top: 40, right: 420, bottom: 40, width: 400, height: 0 });
  f.resize(); f.flush(); await settle(); assert.equal(f.commands.at(-1).intent.visible, false);
  h.render(operation(f.commands.at(-1).intent)); await settle();
  f.host.getBoundingClientRect = () => ({ left: 20, top: 40, right: 420, bottom: 290, width: 400, height: 250 });
  f.resize(); f.flush(); await settle(); assert.equal(f.commands.at(-1).intent.visible, true);
  assert.equal(f.commands.at(-1).intent.placement.height, 375);
});

test("首次自动显示的迟到位置读取不覆盖用户显式选择的 Popup", async () => {
  const f = fixture(); const gate = defer(); f.setOriginGate(gate); const h = f.start();
  const value = h.render(operation());
  await value.setPresentation({ mode: "popup", visible: true, placement: null });
  gate.resolve({ x: 0, y: 0 }); await settle();
  assert.equal(f.commands.length, 1); assert.equal(f.commands[0].intent.mode, "popup");
});


test("挂载已有 hidden 或 Popup 不隐式显示，只有明确页内操作才恢复 hidden", async () => {
  const f = fixture(); const h = f.mount();
  const value = h.render(operation()); await settle(); assert.equal(f.commands.length, 0);
  await value.showOverlay(); await settle(); assert.equal(f.commands.length, 1); assert.equal(f.commands[0].intent.mode, "embedded");
  await value.setPresentation(hidden()); h.render(operation(hidden())); await settle(); h.unmount(); await settle();
  const second = f.mount(); const count = f.commands.length;
  second.render(operation(hidden())); await settle(); assert.equal(f.commands.length, count);
  second.render(operation({ mode: "popup", visible: true, placement: null }, "viewer_prepared", "popup")); await settle();
  assert.equal(f.commands.length, count);
});


test("未选择 deferred 的快速控制器仍等待原 Ready 契约", async () => {
  const f = fixture();
  const controller = { inlineMode: "embedded", ensureSubscription: async () => {},
    restore: f.platform.restoreApplicationRenderPresentationForRoute,
    suspend: f.platform.suspendApplicationRenderPresentationForRoute,
    updatePlacement: f.platform.updateApplicationRenderOverlayPlacement,
    present: f.platform.updateApplicationRenderPresentation };
  const h = f.mount(controller);
  h.render(operation(hidden(), "viewer_preparing")).requestPresentationWhenReady("one");
  h.render(operation(hidden(), "viewer_preparing")); await settle();
  assert.equal(f.commands.length, 0);
  h.render(operation(hidden(), "realtime_ready")); await settle();
  assert.equal(f.commands.length, 1);
  assert.equal(f.commands[0].intent.visible, true);
});

test("Preparing 期间移动容器和 DPI 更新已排队 intent，不等 Ready", async () => {
  const f = fixture(); const h = f.start(operation(hidden(), "viewer_preparing")); await settle();
  h.render(operation(hidden(), "viewer_preparing")); await settle();
  f.window.devicePixelRatio = 2; f.setOrigin({ x: 500, y: 700 });
  f.resize(); f.flush(); await settle();
  assert.deepEqual(f.commands.at(-1).intent.placement, { x: 540, y: 780, width: 800, height: 600, dpiScale: 2, focused: false });
  const count = f.commands.length;
  h.render(operation(f.commands.at(-1).intent, "realtime_ready")); await settle();
  assert.equal(f.commands.length, count);
  assert.ok(f.commands.every(c => c.command === "application_render_update_presentation"));
});

test("Preparing 期间弹窗覆盖 deferred Show，关闭才恢复最新位置", async () => {
  const f = fixture(); const h = f.start(operation(hidden(), "viewer_preparing")); await settle();
  h.render(operation(hidden(), "viewer_preparing"), true); await settle();
  assert.equal(f.commands.at(-1).intent.visible, false);
  const count = f.commands.length;
  h.render(operation(f.commands.at(-1).intent, "realtime_ready"), true); await settle();
  assert.equal(f.commands.length, count);
  f.setOrigin({ x: 300, y: 400 });
  h.render(operation(f.commands.at(-1).intent, "realtime_ready"), false); await settle();
  assert.equal(f.commands.at(-1).intent.visible, true);
  assert.equal(f.commands.at(-1).intent.placement.x, 330);
});

test("Preparing 切页立即 Hide，Ready 与迟到 rect 不复活旧页", async () => {
  const f = fixture(); const h = f.start(operation(hidden(), "viewer_preparing")); await settle();
  h.render(operation(hidden(), "viewer_preparing")); await settle();
  const gate = defer(); f.setOriginGate(gate); f.resize(); f.flush();
  h.unmount(); await settle();
  assert.equal(f.commands.at(-1).intent.visible, false);
  const count = f.commands.length;
  gate.resolve({ x: 1000, y: 2000 }); await settle();
  assert.equal(f.commands.length, count);
  const second = f.mount(); f.setOriginGate(null); f.setOrigin({ x: 300, y: 400 });
  second.render(operation(f.commands.at(-1).intent, "viewer_preparing")); await settle();
  assert.equal(f.commands.at(-1).intent.visible, true);
  assert.equal(f.commands.at(-1).intent.placement.x, 330);
});

test("Preparing pagehide 和最小化撤销 deferred，恢复前的 Ready 不能 Show", async () => {
  for (const reason of ["pagehide", "minimize"]) {
    const f = fixture(); const h = f.start(operation(hidden(), "viewer_preparing")); await settle();
    h.render(operation(hidden(), "viewer_preparing")); await settle();
    if (reason === "pagehide") { f.document.visibilityState = "hidden"; f.event("pagehide"); }
    else { f.setNativeVisible(false); f.nativeEvent(); f.flush(); }
    await settle();
    assert.equal(f.commands.at(-1).intent.visible, false, reason);
    const count = f.commands.length;
    h.render(operation(f.commands.at(-1).intent, "realtime_ready")); await settle();
    assert.equal(f.commands.length, count, reason);
    f.document.visibilityState = "visible"; f.setNativeVisible(true); f.setOrigin({ x: 700, y: 800 });
    if (reason === "pagehide") { f.event("pageshow"); h.render(operation(f.commands.at(-1).intent, "realtime_ready")); }
    else f.nativeEvent();
    f.flush(); await settle();
    assert.equal(f.commands.at(-1).intent.visible, true, reason);
    assert.equal(f.commands.at(-1).intent.placement.x, 730, reason);
  }
});

test("Preparing 首次 rect 迟到且已开弹窗时不能排队 Show", async () => {
  const f = fixture(); const gate = defer(); f.setOriginGate(gate);
  const h = f.start(operation(hidden(), "viewer_preparing"));
  h.render(operation(hidden(), "viewer_preparing"), true);
  gate.resolve({ x: 100, y: 200 }); await settle();
  assert.equal(f.commands.length, 0);
  f.setOriginGate(null);
  h.render(operation(hidden(), "viewer_preparing"), false); await settle();
  assert.equal(f.commands.length, 1);
  assert.equal(f.commands[0].intent.visible, true);
});

test("Full 预加载和实时就绪后连续切页、最小化恢复、最大化及缩放只更新同一 Presentation", async () => {
  for (const state of ["viewer_prepared", "realtime_ready"]) {
    const f = fixture();
    const id = `full-${state}`;
    const view = () => operation(f.commands.at(-1)?.intent ?? hidden(), state, id);
    const first = f.start(operation(hidden(), state, id)); await settle();
    first.render(view()); await settle();
    first.unmount(); await settle();
    assert.equal(f.commands.at(-1).intent.visible, false, `${state}: route leave`);

    f.setOrigin({ x: 50, y: 60 }); f.window.devicePixelRatio = 2;
    const second = f.mount(); second.render(view()); await settle();
    assert.equal(f.commands.at(-1).intent.visible, true, `${state}: route restore`);
    assert.deepEqual(f.commands.at(-1).intent.placement,
      { x: 90, y: 140, width: 800, height: 600, dpiScale: 2, focused: false });
    second.render(view()); await settle();

    f.setNativeVisible(false); f.nativeEvent(); f.flush(); await settle();
    assert.equal(f.commands.at(-1).intent.visible, false, `${state}: minimized`);
    second.render(view()); await settle();
    f.setOrigin({ x: 300, y: 400 }); f.setNativeVisible(true);
    f.nativeEvent(); f.flush(); await settle();
    assert.equal(f.commands.at(-1).intent.visible, true, `${state}: restored`);
    assert.deepEqual(f.commands.at(-1).intent.placement,
      { x: 340, y: 480, width: 800, height: 600, dpiScale: 2, focused: false });
    second.render(view()); await settle();

    f.setOrigin({ x: 0, y: 0 });
    f.host.getBoundingClientRect = () => ({ left: 20, top: 40, right: 1020, bottom: 640, width: 1000, height: 600 });
    f.nativeEvent(); f.resize(); f.flush(); await settle();
    assert.deepEqual(f.commands.at(-1).intent.placement,
      { x: 40, y: 80, width: 2000, height: 1200, dpiScale: 2, focused: false });
    second.render(view()); await settle();

    f.window.devicePixelRatio = 1.5; f.setOrigin({ x: 200, y: 300 });
    f.host.getBoundingClientRect = () => ({ left: 20, top: 40, right: 620, bottom: 240, width: 600, height: 200 });
    f.nativeEvent(); f.resize(); f.flush(); await settle();
    assert.equal(f.commands.at(-1).intent.visible, true, `${state}: resized`);
    assert.deepEqual(f.commands.at(-1).intent.placement,
      { x: 230, y: 360, width: 900, height: 300, dpiScale: 1.5, focused: false });
    second.unmount(); await settle();
    assert.equal(f.commands.at(-1).intent.visible, false, `${state}: final leave`);
    assert.ok(f.commands.every(entry => entry.command === "application_render_update_presentation" && entry.operationId === id),
      "no new build, release, cancel or task authorization commands are allowed during presentation transitions");
  }
});

test("原生最小化或最大化通知在 React 提交前立即废弃旧几何读取", async () => {
  const f = fixture(); const h = f.start(); await settle();
  const view = () => operation(f.commands.at(-1).intent);
  h.render(view()); await settle();

  const minimizeGate = defer(); f.setOriginGate(minimizeGate);
  f.resize(); f.flush(); await settle();
  const beforeMinimize = f.commands.length;
  f.setNativeVisible(false); f.nativeEvent(true);
  minimizeGate.resolve({ x: 900, y: 1000 }); await settle();
  assert.equal(f.commands.length, beforeMinimize, "迟到的可见 rect 不能在原生最小化后发布");
  f.setOriginGate(null); h.render(view()); await settle();
  assert.equal(f.commands.at(-1).intent.visible, false);

  f.setNativeVisible(true); f.setOrigin({ x: 300, y: 400 });
  f.nativeEvent(); f.flush(); await settle();
  assert.equal(f.commands.at(-1).intent.visible, true);
  h.render(view()); await settle();

  const maximizeGate = defer(); f.setOriginGate(maximizeGate);
  f.resize(); f.flush(); await settle();
  const beforeMaximize = f.commands.length;
  f.window.devicePixelRatio = 2;
  f.host.getBoundingClientRect = () => ({ left: 20, top: 40, right: 1020, bottom: 640, width: 1000, height: 600 });
  f.nativeEvent(true);
  maximizeGate.resolve({ x: 700, y: 800 }); await settle();
  assert.equal(f.commands.length, beforeMaximize, "迟到的窗口化 rect 不能覆盖最大化状态");
  f.setOriginGate(null); f.setOrigin({ x: 0, y: 0 }); h.render(view()); await settle();
  assert.deepEqual(f.commands.at(-1).intent.placement,
    { x: 40, y: 80, width: 2000, height: 1200, dpiScale: 2, focused: false });
  assert.ok(f.commands.every(entry => entry.command === "application_render_update_presentation" && entry.operationId === "one"));
});


test("native dialog backdrops compose each live owner without cutting out the entire viewport", () => {
  const { nativeBackdropOpacity } = loadModule("../ui/windows/main/embeddedViewerGeometry.ts", () => ({}), {
    getComputedStyle: element => element,
  });
  const outer = { backgroundColor: "rgba(0, 0, 0, 0.42)", opacity: "1" };
  const inner = { backgroundColor: "rgba(0, 0, 0, 0.42)", opacity: "0.5" };
  assert.equal(nativeBackdropOpacity([]), 0);
  assert.ok(Math.abs(nativeBackdropOpacity([outer]) - .42) < 1e-6);
  assert.ok(Math.abs(nativeBackdropOpacity([outer, inner]) - (1-.58*.79)) < 1e-6);
  assert.ok(Math.abs(nativeBackdropOpacity([outer]) - .42) < 1e-6, "inner close retains outer");
  assert.equal(nativeBackdropOpacity([{backgroundColor:"rgba(0, 0, 0, 0)",opacity:"1"}]),0);
});


test("native modal retains the same presentation and nested owner after inner close", async () => {
  const f = fixture(), h = f.mount();
  const panel = {contains: () => false, dataset:{},hidden:false,closest:()=>null,getAnimations:()=>[],
    getBoundingClientRect:()=>({left:100,top:100,right:300,bottom:250,width:200,height:150})};
  const blocker = {...panel,style:{backgroundColor:"rgba(0, 0, 0, 0.42)",opacity:"1"}};
  let count=0, systemOpen=false;
  f.host.querySelectorAll = selector => selector === "[data-scene-system-modal]" && systemOpen ? [{}] : [];
  f.document.querySelectorAll = selector => selector === "[data-native-ui-surface]" ? [panel]
    : selector === "[data-native-ui-blocking]" ? Array.from({length:count},()=>blocker) : [];
  const view=operation({mode:"embedded",visible:true,placement:{x:130,y:260,width:600,height:450,dpiScale:1.5,focused:false}});
  h.render(view);
  const refresh=async()=>{f.resize();f.flush();await settle();f.flush();await settle();};
  await refresh();
  assert.equal(f.sceneControls.at(-1).uiInputBlocked,false,"floating panel alone is not modal");
  count=2;await refresh();
  assert.equal(f.sceneControls.at(-1).releaseCameraInput,true,"entering modal releases input even if a floating panel was already open");
  assert.ok(f.sceneControls.at(-1).uiRegions.length>0);
  assert.ok(Math.abs(f.sceneControls.at(-1).uiBackdropOpacity-(1-.58**2))<1e-6);
  assert.equal(f.sceneControls.at(-1).uiInputBlocked,true);
  count=1;await refresh();
  assert.ok(Math.abs(f.sceneControls.at(-1).uiBackdropOpacity-.42)<1e-6);
  assert.notEqual(f.sceneControls.at(-1).uiInputBlocked,false);
  systemOpen=true;count=0;await refresh();
  assert.equal(f.sceneControls.at(-1).uiBackdropOpacity,0);
  assert.notEqual(f.sceneControls.at(-1).uiInputBlocked,false,"native file dialog still owns input");
  systemOpen=false;await refresh();
  assert.equal(f.sceneControls.at(-1).uiInputBlocked,false);
  assert.ok(f.commands.every(c=>c.command==="application_render_update_presentation"&&c.intent.visible),"modal never hides/releases/rebuilds Viewer");
  h.unmount();await settle();
});
