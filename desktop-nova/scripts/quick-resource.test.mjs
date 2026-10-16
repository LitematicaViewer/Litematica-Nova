import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

const tick = () => new Promise(setImmediate);
const plain = (value) => JSON.parse(JSON.stringify(value));
const hidden = () => ({ mode: "hidden", visible: false, placement: null });
const report = (pressure = "warning", id = "quick-one") => ({
  schema: "nova.quick.resource.v1", requestId: id, sourceBytes: 840, preload: true,
  pressure, pressureBasisPoints: (pressure === "invalid" || pressure === "unmeasured") ? null : pressure === "hard" ? 9500 : 8000,
  budget: ["normal", "warning", "hard"].includes(pressure) ? { adapterName: "fixture", budgetBytes: 10_000, currentUsageBytes: 0, safeAvailableBytes: 10_000 } : null,
  estimate: ["normal", "warning", "hard"].includes(pressure) ? { gpuResidentUpperBytes: 8_000, gpuPeakUpperBytes: 8_000, uploadStagingUpperBytes: 0 } : null,
  invalidReason: pressure === "invalid" ? "预算未知" : null, decisionRequired: true,
});
const view = (overrides = {}) => ({ previewId: "quick-one", sourcePath: "C:/fixture.litematic",
  renderMode: "fast_experimental", state: "ready", preload: false,
  presentation: hidden(), presentationRevision: 0, visible: false, error: null, resource: null, ...overrides });
function deferred() { let resolve; const promise = new Promise((done) => { resolve = done; }); return { promise, resolve }; }
function load(path, require, globals = {}) {
  const result = ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React },
    reportDiagnostics: true,
  });
  assert.equal(result.diagnostics.length, 0);
  const context = { exports: {}, require, console, ...globals };
  vm.runInNewContext(result.outputText, context, { filename: path });
  return context.exports;
}
function platformFixture() {
  const calls = []; let current = null, sequence = 0, readGate, decisionGate, nextResource, actualSceneStarts = 0;
  const invoke = async (command, args) => {
    calls.push({ command, args: args && plain(args) });
    if (command === "application_quick_preview_start") {
      if (current && current.sourcePath === args.input.sourcePath
          && current.preload === (args.input.preload ?? false)
          && ["starting", "ready"].includes(current.state)) return current;
      current = view({ previewId: `quick-${++sequence}`, sourcePath: args.input.sourcePath, preload: args.input.preload ?? false,
        resource: nextResource ? { ...nextResource, requestId: `quick-${sequence}`, preload: args.input.preload ?? false } : null });
      if (current.preload && current.resource?.pressure === "unmeasured") current.state = "starting";
      else actualSceneStarts++;
      return current;
    }
    if (command === "application_quick_preview_snapshot") return readGate ? readGate.promise : current;
    if (command === "application_quick_preview_present") return current = { ...current, presentation: args.intent };
    if (command === "application_quick_preview_resource_decision") {
      if (decisionGate) return decisionGate.promise;
      if (current.state === "starting" && current.resource?.pressure === "unmeasured" && args.action === "realtime") {
        actualSceneStarts++;
        return current = { ...current, previewId: `quick-resolved-${sequence}`, state: "ready", preload: false, resource: null };
      }
      return current = { ...current, preload: args.action === "preload", resource: { ...current.resource, decisionRequired: false } };
    }
    if (command === "application_quick_preview_close") return;
    throw new Error(`Unexpected non-quick command: ${command}`);
  };
  const api = load("../src/platform/quickPreview.ts", (name) => {
    if (name === "./tauri") return { invoke };
    if (name === "../services/filePathIdentity") return load("../src/services/filePathIdentity.ts", (name) => { throw new Error(name); });
    throw new Error(name);
  }, { setTimeout, clearTimeout });
  return { api, calls, get launches() { return sequence; }, get actualSceneStarts() { return actualSceneStarts; }, setCurrent(value) { current = value; }, setReadGate(value) { readGate = value; },
    setDecisionGate(value) { decisionGate = value; }, setNextResource(value) { nextResource = value; } };
}

// Load the actual hook and Presentation viewport; only native geometry/React scheduling are fixtures.
async function hookFixture({ initial = null, preload = false, nextResource = null } = {}) {
  const platform = platformFixture();
  platform.setCurrent(initial); platform.setNextResource(nextResource);
  await platform.api.refreshQuickPreview();
  let props = { source: "C:/fixture.litematic", blocked: false, preload }, cursor = 0, effects = [], output;
  const slots = [];
  const hooks = {
    useRef(value) { const i = cursor++; return slots[i] ??= { current: value }; },
    useState(value) { const i = cursor++; slots[i] ??= { value }; return [slots[i].value, (next) => {
      slots[i].value = typeof next === "function" ? next(slots[i].value) : next;
    }]; },
    useSyncExternalStore(_subscribe, get) { cursor++; return get(); },
    useEffect(effect, dependencies) {
      const i = cursor++, previous = slots[i];
      if (previous && dependencies.every((item, j) => Object.is(item, previous.dependencies[j]))) return;
      slots[i] = { dependencies };
      effects.push(() => { previous?.cleanup?.(); slots[i].cleanup = effect(); });
    },
  };
  hooks.useLayoutEffect = hooks.useEffect;
  const globals = { document: { visibilityState: "visible", addEventListener() {}, removeEventListener() {} },
    window: { addEventListener() {}, removeEventListener() {}, setTimeout() { return 1; }, clearTimeout() {} },
    ResizeObserver: class { observe() {} disconnect() {} } };
  const geometry = { elementToNativeChildPlacement: async () => ({ x: 20, y: 30, width: 640, height: 480, dpiScale: 1.5, focused: true }),
    isUsableEmbeddedRect: (rect) => rect.width > 0 && rect.height > 0,
    subscribeNativeWindowGeometry: async () => () => {} };
  const viewport = load("../ui/windows/main/useEmbeddedViewer.ts", (name) => {
    if (name === "react") return hooks;
    if (name.endsWith("facade")) return platform.api;
    if (name.endsWith("embeddedViewerGeometry")) return geometry;
    throw new Error(name);
  }, globals);
  const hook = load("../ui/windows/main/useQuickPreview.ts", (name) => {
    if (name === "react") return hooks;
    if (name.endsWith("facade")) return platform.api;
    if (name.endsWith("useEmbeddedViewer")) return viewport;
    throw new Error(name);
  });
  function render() {
    cursor = 0; effects = [];
    output = hook.useQuickPreview(props.source, props.blocked, props.preload);
    output.hostRef.current = {};
    for (const effect of effects) effect();
  }
  render();
  return { ...platform, get launches() { return platform.launches; }, get actualSceneStarts() { return platform.actualSceneStarts; }, get result() { return output; },
    setProps(next) { props = { ...props, ...next }; render(); },
    async settle() { await tick(); render(); await tick(); render(); await tick(); },
    unmount() { for (const slot of slots) slot?.cleanup?.(); } };
}

test("快速预览按checkbox传真实preload参数，仍是独立Fast启动", async () => {
  for (const preload of [false, true]) {
    const h = await hookFixture({ preload });
    await h.result.start(); await h.settle();
    const start = h.calls.find((call) => call.command === "application_quick_preview_start");
    assert.equal(start.args.input.sourcePath, "C:/fixture.litematic");
    assert.equal(start.args.input.preload ?? false, preload);
    assert.equal(h.result.operation.renderMode, "fast_experimental");
    assert.equal(h.result.operation.preload, preload);
    assert(h.calls.every((call) => call.command.startsWith("application_quick_preview_")));
    h.unmount();
  }
});

test("Ready实际加载模式与checkbox不同必须重启Fast，不冒充预加载", async () => {
  for (const requested of [false, true]) {
    const h = await hookFixture({ initial: view({ preload: !requested }), preload: requested });
    await h.result.start(); await h.settle();
    assert.equal(h.calls.filter((call) => call.command === "application_quick_preview_start").length, 1);
    assert.equal(h.result.operation.preload, requested);
    h.unmount();
  }
});

test("相同Ready模式只改变Presentation，不重建Fast", async () => {
  const h = await hookFixture({ initial: view({ preload: true }), preload: true });
  await h.result.start(); await h.settle();
  assert.equal(h.calls.filter((call) => call.command === "application_quick_preview_start").length, 1);
  assert.equal(h.launches, 0);
  assert.equal(h.result.operation.previewId, "quick-one");
  assert(h.calls.some((call) => call.command === "application_quick_preview_present" && call.args.intent.visible));
  h.unmount();
});

test("Fast忽略历史预估报告，实时/预加载都不等待显存确认，也不发关闭阴影选择", async () => {
  for (const preload of [false, true]) for (const pressure of ["warning", "hard", "invalid", "unmeasured"]) {
    const h = await hookFixture({ initial: view({ preload, resource: report(pressure) }), preload });
    await h.result.start(); await h.settle();
    assert.equal(h.calls.filter(call => call.command === "application_quick_preview_resource_decision").length, 0);
    assert(h.calls.some(call => call.command === "application_quick_preview_present" && call.args.intent.visible));
    assert.equal(h.result.chooseResources, undefined);
    h.unmount();
  }
  for (const page of ["render/RenderPage", "properties/PropertiesPage"]) {
    const source = readFileSync(new URL(`../ui/windows/main/pages/${page}.tsx`, import.meta.url), "utf8");
    assert.doesNotMatch(source, /QuickResourceDialog|chooseResources|reduced_realtime|关闭阴影/);
  }
});

test("旧报告晚快照与旧决策回执均不能覆盖新Fast会话", async () => {
  const h = platformFixture();
  const old = await h.api.startQuickPreview("C:/old.litematic", true), read = deferred();
  h.setReadGate(read); const refreshing = h.api.refreshQuickPreview();
  const current = await h.api.startQuickPreview("C:/new.litematic", false);
  read.resolve({ ...old, resource: report("hard", old.previewId) }); await refreshing;
  assert.equal(h.api.getQuickPreviewState().previewId, current.previewId);
  assert.equal(h.api.getQuickPreviewState().resource, null);
  h.setReadGate(null); h.setCurrent({ ...current, resource: report("warning", current.previewId) });
  await h.api.refreshQuickPreview();
  const gate = deferred(); h.setDecisionGate(gate);
  const deciding = h.api.decideQuickPreviewResources(current.previewId, "realtime");
  const newest = await h.api.startQuickPreview("C:/newest.litematic", true);
  gate.resolve({ ...current, resource: { ...report("warning", current.previewId), decisionRequired: false } }); await deciding;
  assert.equal(h.api.getQuickPreviewState().previewId, newest.previewId);
  assert.equal(h.api.getQuickPreviewState().preload, true);
  await assert.rejects(h.api.decideQuickPreviewResources(current.previewId, "preload", true), /不属于当前快速预览/);
});
