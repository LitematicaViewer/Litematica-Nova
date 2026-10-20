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
const hidden = () => ({ mode: "hidden", visible: false, placement: null });
const operation = (presentation = hidden(), state = "viewer_prepared", operationId = "one") => ({ operationId, presentation, state });
function fixture() {
  const commands = [];
  let origin = { x: 100, y: 200 };
  let originGate;
  let focused = true;
  let nativeVisible = true;
  let nativeListener;
  let nativeRender = () => {};
  const listeners = new Map();
  const timers = new Map();
  const observers = new Set();
  let timerId = 0;
  const host = { parentElement: null, getBoundingClientRect: () => ({ left: 20, top: 40, right: 420, bottom: 340, width: 400, height: 300 }) };
  const window = { devicePixelRatio: 1.5, innerWidth: 1280, innerHeight: 800, getComputedStyle: () => ({ overflowX: "visible", overflowY: "visible" }),
    setTimeout(fn) { timers.set(++timerId, fn); return timerId; },
    clearTimeout(id) { timers.delete(id); },
    addEventListener(name, fn) { listeners.set(name, fn); },
    removeEventListener(name, fn) { if (listeners.get(name) === fn) listeners.delete(name); },
  };
  const document = { visibilityState: "visible", hasFocus: () => focused,
    addEventListener: window.addEventListener, removeEventListener: window.removeEventListener };
  const platform = loadModule("../src/platform/applicationRender.ts", (id) => {
    if (id === "@tauri-apps/api/event") return { listen: async () => () => {} };
    if (id === "./tauri") return { invoke: async (command, payload) => { commands.push({ command, ...JSON.parse(JSON.stringify(payload)) }); } };
    throw Error(id);
  });
  const geometry = loadModule("../ui/windows/main/embeddedViewerGeometry.ts", () => ({
    getApplicationWindowOrigin: async () => originGate ? originGate.promise : origin,
    getApplicationWindowVisibility: async () => nativeVisible,
    subscribeApplicationWindowGeometry: async (fn) => { nativeListener = fn; return () => { if (nativeListener === fn) nativeListener = null; }; },
  }), { window, document });
  function mount(controller = null) {
    let cursor = 0;
    let effects = [];
    const hooks = [];
    const react = {
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
      if (id.endsWith("facade")) return { ...platform, ensureApplicationRenderSubscription: async () => {} };
      if (id.endsWith("embeddedViewerGeometry")) return geometry;
      throw Error(id);
    }, { window, document, ResizeObserver: class { constructor(fn) { this.fn = fn; observers.add(fn); } observe() {} disconnect() { observers.delete(this.fn); } } });
    return {
      render(op, blocked = false) { nativeRender = () => this.render(op, blocked); cursor = 0; const value = controller
        ? module.usePresentationViewport(op ? { operationId: op.operationId, prepared: platform.isApplicationViewerPrepared(op), presentation: op.presentation } : null, blocked, controller)
        : module.useEmbeddedViewer(op, blocked); value.hostRef.current = host; while (effects.length) effects.shift()(); return value; },
      unmount() { hooks.forEach((hook) => hook.cleanup?.()); },
    };
  }
  function start(op = operation()) {
    const h = mount();
    h.render(op).requestPresentationWhenReady(op.operationId);
    h.render(op);
    return h;
  }
  return { commands, platform, geometry, host, mount, start, window, document,
    setOrigin(value) { origin = value; }, setOriginGate(value) { originGate = value; },
    setFocused(value) { focused = value; }, setNativeVisible(value) { nativeVisible = value; },
    event(name) { listeners.get(name)?.(); }, nativeEvent() { nativeListener?.(); nativeRender(); },
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
