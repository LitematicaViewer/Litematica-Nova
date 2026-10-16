import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

const pathExports = {};
const pathCode = ts.transpileModule(fs.readFileSync(new URL("../src/services/filePathIdentity.ts", import.meta.url), "utf8"), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
vm.runInNewContext(pathCode, { exports: pathExports });

function deferred() { let resolve; const promise = new Promise((r) => resolve = r); return { promise, resolve }; }
function fixture(options = {}) {
  const calls = []; let current = null, readGate, presentGate, starts = 0;
  const exports = {};
  const invoke = async (command, args) => {
    calls.push({ command, args });
    if (command === "application_quick_preview_start") return current = {
      previewId: ++starts === 1 ? "quick-one" : `quick-one-scene-${starts}`, sourcePath: args.input.sourcePath, renderMode: "fast_experimental", state: options.startState ?? "ready",
      presentation: { mode: "hidden", visible: false, placement: null }, visible: false, error: null,
    };
    if (command === "application_quick_preview_snapshot") return readGate ? readGate.promise : current;
    if (command === "application_quick_preview_present") return presentGate ? presentGate.promise : current = { ...current, presentation: args.intent };
    if (command === "application_quick_preview_close") return;
    throw new Error(`非快速命令：${command}`);
  };
  const compiled = ts.transpileModule(fs.readFileSync(new URL("../src/platform/quickPreview.ts", import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
  vm.runInNewContext(compiled, { exports, require: (name) => { if (name.endsWith("filePathIdentity")) return pathExports; assert.equal(name, "./tauri"); return { invoke }; }, setTimeout: options.setTimeout ?? setTimeout, clearTimeout: options.clearTimeout ?? clearTimeout });
  return { api: exports, calls, setReadGate: (gate) => { readGate = gate; }, setPresentGate: (gate) => { presentGate = gate; }, setCurrent: (value) => { current = value; } };
}
const rect = { x: -10, y: 30, width: 500, height: 350, dpiScale: 1.5, focused: false };
const overlay = { mode: "embedded", visible: true, placement: rect };

test("正式渲染状态通知不等待被原生视口遮挡的 WebView 绘制帧，完成与取消及时通知", async () => {
  const api = {}, scheduledFrames = [];
  let receive, notices = 0;
  const compiled = ts.transpileModule(fs.readFileSync(new URL("../src/services/renderService.ts", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(compiled, { exports: api, queueMicrotask,
    window: { requestAnimationFrame: callback => scheduledFrames.push(callback) },
    require(name) {
      if (name === "./filePathIdentity") return pathExports;
      assert.equal(name, "../platform/applicationRender");
      return { subscribeApplicationRenderState: async listener => { receive = listener; return () => {}; },
        getApplicationRenderSnapshot: async () => ({ operations: [] }) };
    },
  });
  const unsubscribe = api.subscribeApplicationRender(() => { notices++; });
  await api.ensureApplicationRenderSubscription(); await Promise.resolve(); notices = 0;
  const publish = state => receive({ operations: [{ operationId: "giant-tree", state }] });
  publish("running"); publish("viewer_prepared");
  await Promise.resolve();
  assert.equal(notices, 1, "同一批状态合并，但通知不能依赖 rAF 才执行");
  assert.equal(api.getApplicationRenderState().operations[0].state, "viewer_prepared");
  publish("cancelled"); await Promise.resolve();
  assert.equal(notices, 2); assert.equal(api.getApplicationRenderState().operations[0].state, "cancelled");
  assert.equal(scheduledFrames.length, 0);
  unsubscribe(); publish("runtime_released"); await Promise.resolve(); assert.equal(notices, 2);
});

test("路径身份：Full 与快速共享盘符/UNC长路径规则，POSIX大小写不误合并", () => {
  const exports = {};
  const compiled = ts.transpileModule(fs.readFileSync(new URL("../src/services/renderService.ts", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(compiled, { exports, require(name) {
    if (name === "./filePathIdentity") return pathExports;
    assert.equal(name, "../platform/applicationRender"); return {};
  } });
  for (const [input, stored] of [
    ["C:/test/scene.litematic", String.raw`\\?\C:\TEST\scene.litematic`],
    [String.raw`\\server\share\scene.litematic`, String.raw`\\?\UNC\SERVER\share\scene.litematic`],
  ]) {
    const operation = { operationId: "full-real", source: { path: stored } };
    assert.equal(exports.findRenderOperationForSource(input, { operations: [operation] }), operation);
  }
  assert.equal(exports.findRenderOperationForSource("/Case/file.litematic", { operations: [{ source: { path: "/case/file.litematic" } }] }), null);
});

test("同路径正式operation按真实display/build模式隔离，长路径仍统一身份", () => {
  const exports = {};
  const compiled = ts.transpileModule(fs.readFileSync(new URL("../src/services/renderService.ts", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(compiled, { exports, require(name) { if (name === "./filePathIdentity") return pathExports; assert.equal(name, "../platform/applicationRender"); return {}; } });
  const path = "C:/test/scene.litematic";
  const operations = [
    { operationId: "standard-stream", source: { path: String.raw`\\?\C:\TEST\scene.litematic` }, displayMode: "standard", buildMode: "realtime" },
    { operationId: "full-stream", source: { path }, displayMode: "full", buildMode: "realtime" },
    { operationId: "full-preloaded", source: { path }, displayMode: "full", buildMode: "prebuilt" },
  ];
  for (const operation of operations) assert.equal(exports.findRenderOperationForSource(path, { operations }, { displayMode: operation.displayMode, buildMode: operation.buildMode }), operation);
  assert.equal(exports.findRenderOperationForSource(path, { operations }, { displayMode: "standard", buildMode: "prebuilt" }), null);
});

test("快速预览：独立启动只传路径和原点击计时，不进入 Full/Realtime；标准化同一 Windows 路径", async () => {
  const h = fixture();
  const source = "C:\\test\\sample.litematic";
  const view = await h.api.startQuickPreview(source, false, 1792000000123);
  assert.equal(view.renderMode, "fast_experimental");
  assert.deepEqual(h.calls.map((call) => call.command), ["application_quick_preview_start"]);
  assert.deepEqual(Object.keys(h.calls[0].args.input), ["sourcePath", "clickStartedAtEpochMs"]);
  assert.equal(h.calls[0].args.input.clickStartedAtEpochMs, 1792000000123);
  assert.equal(h.api.quickPreviewForSource(source, { ...view, sourcePath: "\\\\?\\C:\\test\\sample.litematic" })?.previewId, view.previewId);
  assert.equal(h.api.quickPreviewForSource("C:/test/SAMPLE.litematic")?.previewId, view.previewId);
  assert.equal(h.api.quickPreviewForSource("C:\\test\\other.litematic"), null);
  const unc = String.raw`\\server\share\投影.litematic`;
  assert.equal(h.api.quickPreviewForSource(unc, { ...view, sourcePath: String.raw`\\?\UNC\SERVER\share\投影.litematic` })?.previewId, view.previewId);
});

test("快速预览：切页暂停恢复，显式弹窗后迟到布局不得重新嵌入", async () => {
  const h = fixture(); const view = await h.api.startQuickPreview("sample.litematic");
  await h.api.presentQuickPreview(view.previewId, overlay);
  assert.equal(await h.api.suspendQuickPreview(view.previewId, overlay), true);
  assert.equal(h.api.getQuickPreviewState().presentation.visible, false);
  assert.equal(await h.api.restoreQuickPreview(view.previewId, rect), true);
  assert.equal(h.api.getQuickPreviewState().presentation.visible, true);
  await h.api.presentQuickPreview(view.previewId, { mode: "popup", visible: true, placement: null });
  const count = h.calls.length;
  assert.equal(await h.api.updateQuickPreviewPlacement(view.previewId, rect, overlay), false);
  assert.equal(await h.api.restoreQuickPreview(view.previewId, rect), false);
  assert.equal(h.calls.length, count);
  assert.ok(h.calls.every((call) => call.command.startsWith("application_quick_preview_")));
});

test("快速预览：启动前迟到快照不覆盖新会话，关闭不触碰 Full", async () => {
  const h = fixture(), gate = deferred(); h.setReadGate(gate);
  const read = h.api.refreshQuickPreview();
  await h.api.startQuickPreview("sample.litematic");
  gate.resolve(null); await read;
  assert.equal(h.api.getQuickPreviewState().previewId, "quick-one");
  await h.api.closeQuickPreview("quick-one");
  assert.equal(h.api.getQuickPreviewState().state, "closed");
  assert.equal(h.calls.at(-1).command, "application_quick_preview_close");
});

test("快速预览：同GPU会话更换场景后，旧呈现回执不能显示新投影", async () => {
  const h = fixture();
  const previous = await h.api.startQuickPreview("before.litematic");
  const gate = deferred(); h.setPresentGate(gate);
  const oldAttach = h.api.presentQuickPreview(previous.previewId, overlay);
  const current = await h.api.startQuickPreview("after.litematic");
  assert.notEqual(current.previewId, previous.previewId);
  gate.resolve({ ...previous, presentation: overlay, visible: true }); await oldAttach;
  assert.equal(h.api.getQuickPreviewState().sourcePath, "after.litematic");
  assert.equal(h.api.getQuickPreviewState().previewId, current.previewId);
  assert.equal(h.api.getQuickPreviewState().visible, false);
});

test("快速预览：实际启动立即替换空闲轮询，完成后恢复低频，不等待250/750ms", async () => {
  const active = new Map(), cancelled = []; let sequence = 0;
  const h = fixture({ startState: "starting", setTimeout(callback, delay) { const id = ++sequence; active.set(id, { callback, delay }); return id; },
    clearTimeout(id) { cancelled.push(id); active.delete(id); } });
  const unsubscribe = h.api.subscribeQuickPreview(() => {});
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal([...active.values()].at(-1).delay, 750);
  const started = await h.api.startQuickPreview("real.litematic");
  assert.ok(cancelled.length > 0);
  assert.equal([...active.values()].at(-1).delay, 8);
  h.setCurrent({ ...started, state: "ready" });
  const [id, timer] = [...active.entries()].at(-1); active.delete(id); timer.callback();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(h.api.getQuickPreviewState().state, "ready");
  assert.equal([...active.values()].at(-1).delay, 750);
  unsubscribe(); assert.equal(active.size, 0);
});

test("快速预览：原生 Esc/关窗 Hidden 快照阻止迟到布局自动显示", async () => {
  const h = fixture(); const view = await h.api.startQuickPreview("sample.litematic");
  await h.api.presentQuickPreview(view.previewId, overlay);
  h.setCurrent({ ...view, presentation: { mode: "hidden", visible: false, placement: null }, visible: false });
  await h.api.refreshQuickPreview(); const count = h.calls.length;
  assert.equal(await h.api.updateQuickPreviewPlacement(view.previewId, rect, overlay), false);
  assert.equal(h.calls.length, count);
  assert.equal(h.api.getQuickPreviewState().visible, false);
});
