import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

function deferred() { let resolve; const promise = new Promise((r) => resolve = r); return { promise, resolve }; }
function fixture() {
  const calls = []; let current = null, readGate;
  const exports = {};
  const invoke = async (command, args) => {
    calls.push({ command, args });
    if (command === "application_quick_preview_start") return current = {
      previewId: "quick-one", sourcePath: args.input.sourcePath, renderMode: "fast_experimental", state: "ready",
      presentation: { mode: "hidden", visible: false, placement: null }, visible: false, error: null,
    };
    if (command === "application_quick_preview_snapshot") return readGate ? readGate.promise : current;
    if (command === "application_quick_preview_present") return current = { ...current, presentation: args.intent };
    if (command === "application_quick_preview_close") return;
    throw new Error(`非快速命令：${command}`);
  };
  const compiled = ts.transpileModule(fs.readFileSync(new URL("../src/platform/quickPreview.ts", import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
  vm.runInNewContext(compiled, { exports, require: (name) => { assert.equal(name, "./tauri"); return { invoke }; }, setTimeout, clearTimeout });
  return { api: exports, calls, setReadGate: (gate) => { readGate = gate; }, setCurrent: (value) => { current = value; } };
}
const rect = { x: -10, y: 30, width: 500, height: 350, dpiScale: 1.5, focused: false };
const overlay = { mode: "embedded", visible: true, placement: rect };

test("快速预览：独立启动只传文件路径，不进入 Full/Realtime；标准化同一 Windows 路径", async () => {
  const h = fixture();
  const source = "C:\\test\\sample.litematic";
  const view = await h.api.startQuickPreview(source);
  assert.equal(view.renderMode, "fast_experimental");
  assert.deepEqual(h.calls.map((call) => call.command), ["application_quick_preview_start"]);
  assert.deepEqual(Object.keys(h.calls[0].args.input), ["sourcePath"]);
  assert.equal(h.api.quickPreviewForSource(source, { ...view, sourcePath: "\\\\?\\C:\\test\\sample.litematic" })?.previewId, view.previewId);
  assert.equal(h.api.quickPreviewForSource("C:/test/SAMPLE.litematic")?.previewId, view.previewId);
  assert.equal(h.api.quickPreviewForSource("C:\\test\\other.litematic"), null);
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

test("快速预览：原生 Esc/关窗 Hidden 快照阻止迟到布局自动显示", async () => {
  const h = fixture(); const view = await h.api.startQuickPreview("sample.litematic");
  await h.api.presentQuickPreview(view.previewId, overlay);
  h.setCurrent({ ...view, presentation: { mode: "hidden", visible: false, placement: null }, visible: false });
  await h.api.refreshQuickPreview(); const count = h.calls.length;
  assert.equal(await h.api.updateQuickPreviewPlacement(view.previewId, rect, overlay), false);
  assert.equal(h.calls.length, count);
  assert.equal(h.api.getQuickPreviewState().visible, false);
});
