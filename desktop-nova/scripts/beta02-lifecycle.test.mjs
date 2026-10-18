import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import vm from "node:vm";
import ts from "typescript";

// CLI source/data checks only. They do not start a Viewer, GPU, GUI, installer or Explorer.
const read = name => readFileSync(new URL(name, import.meta.url), "utf8");
const host = read("../src-tauri/src/main.rs");
const preview = read("../src-tauri/src/application/quick_preview.rs");
const operations = read("../src-tauri/src/application/operations.rs");
const renderFacade = read("../src-tauri/src/application/render_facade.rs");
const native = read("../../tools/viewer-core/src/native_viewer.rs");
const thumbnails = read("../src-tauri/src/application/thumbnails.rs");
const explorer = read("../../tools/windows-thumbnail-provider/thumbnail_provider.cpp");
const shell = read("../src-tauri/src/shell_integration.rs");
const installer = read("../src-tauri/installer/nsis-hooks.nsh");

test("Core实际任务不因耗时被终止，轮询继续等待且保留进程退出和lease失效", () => {
  const run = operations.slice(operations.indexOf("    fn run_core("), operations.indexOf("    fn operation_path("));
  assert(!operations.includes("CORE_OPERATION_TIMEOUT"));
  assert(!run.includes("exceeded its bounded execution time"));
  assert(run.includes("WaitOutcome::TimedOut => continue"));
  assert(run.includes("WaitOutcome::Exited(exit) => break exit"));
  assert(run.includes("Core operation lost its supervised process lease"));
  assert(!run.includes("self.supervisor.terminate"));
  assert(operations.includes("state.reads.values().cloned().collect()"));
});

test("缩略图准备和三模式截图没有工作量耗时失败门槛，真实关闭错误仍结束等待", () => {
  const thumbnail = host.slice(host.indexOf("let temporary = host.thumbnail_session()"), host.indexOf("let launch = prepare_viewer_launch_context", host.indexOf("let temporary = host.thumbnail_session()")));
  assert(!thumbnail.includes("elapsed()"));
  assert(thumbnail.includes('matches!(current.state, "failed" | "closed")'));
  assert(thumbnail.includes('temporary.snapshot()?.ok_or("缩略图预览会话结束")'));
  const quickCapture = preview.slice(preview.indexOf("pub(crate) fn capture_frame"), preview.indexOf("fn stop_session"));
  assert(!quickCapture.includes("elapsed()"));
  assert(quickCapture.includes("session.refresh(&self.supervisor)?"));
  assert(quickCapture.includes('matches!(session.view.state, "closed" | "failed")'));
  const exactCapture = renderFacade.slice(renderFacade.indexOf("pub(crate) async fn capture_frame"), renderFacade.indexOf("pub(crate) async fn headless_camera_stress"));
  assert(!exactCapture.includes("elapsed()"));
  for (const guard of ["record.cancel_requested", "Some(&session_id)", "TryRecvError::Disconnected", "ServiceEvent::ServiceStopping", "ServiceEvent::Error", "error_path.is_file()", "output_path.is_file()"]) assert(exactCapture.includes(guard), guard);
});

function openFileEffectHarness(initial = []) {
  const source = ts.createSourceFile("AppShell.tsx", read("../ui/shell/AppShell.tsx"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  let effect;
  const find = node => {
    if (ts.isCallExpression(node) && node.expression.getText(source) === "useEffect" && node.arguments[0]?.getText(source).includes('"nova-open-projection"')) effect = node.arguments[0];
    ts.forEachChild(node, find);
  };
  find(source); assert(effect, "Open-file effect must subscribe before draining pending startup requests.");
  const pending = [...initial]; const selected = []; const routes = []; const imported = [];
  let library = { records: [] };
  let callback; let closed = false;
  const take = async () => pending.splice(0);
  const module = { exports: {} };
  const code = ts.transpileModule(`exports.run = ${effect.getText(source)};`, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(code, { exports: module.exports, console,
    invoke: async command => { assert.equal(command, "application_take_open_files"); return take(); },
    takeApplicationOpenFiles: take,
    loadLibrary: async () => ({ records: [...library.records] }),
    addOrUpdateRecord: async (state, path) => { imported.push(path); library = { ...state, records: [...state.records.filter(value => value !== path), path] }; return library; },
    setCurrentFile: path => selected.push(path), setRoute: route => routes.push(route),
    listenEvent: async (name, handler) => { assert.equal(name, "nova-open-projection"); callback = handler; return () => { closed = true; }; },
  });
  const cleanup = module.exports.run();
  const tick = () => new Promise(resolve => setImmediate(resolve));
  return { pending, selected, routes, imported, tick, async emit(path) { pending.push(...(Array.isArray(path) ? path : [path])); callback(); await tick(); }, async close() { cleanup(); await tick(); return closed; } };
}

test("真实帧来源是同一渲染输出纹理，GPU readback 后才保存 PNG", () => {
  const capture = native.slice(native.indexOf('label: Some("nova_real_frame_readback")'), native.indexOf("output.present();", native.indexOf('label: Some("nova_real_frame_readback")')));
  assert(capture.includes("texture: &output.texture")); assert(capture.includes("copy_texture_to_buffer"));
  assert(capture.indexOf("self.queue.submit") < capture.indexOf("save_gpu_frame"));
  assert(native.includes("slice.map_async(wgpu::MapMode::Read"));
  assert(native.includes("image::save_buffer_with_format"));
  assert(native.includes("pending_capture = Some"));
  assert(!native.includes("while state.frame_index"), "Capture must not run a seconds-long construction loop on the presentation event loop.");
});

test("缓存和Explorer使用同一源内容SHA256；Explorer无渲染器或启动桌面分支", () => {
  assert(thumbnails.includes("Sha256::new()")); assert(thumbnails.includes("hash.update(&chunk[..count])"));
  assert(thumbnails.includes('join("Litematica-Nova").join("thumbnails")'));
  assert(explorer.includes("BCRYPT_SHA256_ALGORITHM")); assert(explorer.includes('L"\\\\Litematica-Nova\\\\thumbnails\\\\"'));
  assert(explorer.includes("GUID_ContainerFormatPng")); assert(explorer.includes("CachedPngBitmap(path, cx, bitmap)"));
  assert(!/CreateProcess|ShellExecute|viewer|wgpu|Command::new/i.test(explorer));
  const digest = bytes => createHash("sha256").update(bytes).digest("hex");
  assert.equal(digest(Buffer.from("same projection bytes")), digest(Buffer.from("same projection bytes")));
  assert.notEqual(digest(Buffer.from("same projection bytes")), digest(Buffer.from("modified projection bytes")));
});

test("PNG暂存名唯一，缓存绑定截图开始时源内容，结束后再拒绝变化源", () => {
  assert(thumbnails.includes("static IMAGE_SEQUENCE: AtomicU64"));
  assert.equal((thumbnails.match(/IMAGE_SEQUENCE\.fetch_add\(1, Ordering::Relaxed\)/g) ?? []).length, 2);
  assert(thumbnails.includes("if output != expected"));
  assert(thumbnails.indexOf("if output != expected") < thumbnails.indexOf("fs::write(&temp"));
  assert(host.includes("application::thumbnails::temporary_preview_path()"));
  assert(!host.includes('format!("nova-preview-{}.png"'));
  const capture = preview.slice(preview.indexOf("pub(crate) fn capture_frame"), preview.indexOf("fn stop_session"));
  const completed = capture.slice(capture.indexOf("if output.is_file()"));
  assert(completed.includes("metadata.len() != session.source_bytes"));
  assert(completed.includes("metadata.modified().ok() != session.source_modified"));
  assert(capture.includes("session.capture_errors.remove(&sequence)"));
});

test("迟到Presentation回执不能覆盖新隐藏意图，Capture等待不持有状态锁", () => {
  assert(preview.includes("revision > self.presentation_revision"));
  assert(preview.includes("status.sequence + 1 == self.next_command"));
  const capture = preview.slice(preview.indexOf("pub(crate) fn capture_frame"), preview.indexOf("fn stop_session"));
  assert(capture.includes('filter(|session| session.view.preview_id == id)'));
  assert(capture.includes('matches!(session.view.state, "closed" | "failed")'));
  assert(/\}\s*(?:if [^\n]*\n\s*)?if output\.is_file\(\)/.test(capture));
  assert(capture.includes("std::thread::sleep(Duration::from_millis(50))"));
});

test("关闭Host静态边界守卫；实际写入收尾与进程退出由原生行为测试验证", () => {
  const shutdown = host.slice(host.indexOf(".on_window_event("), host.indexOf("app.exit(0)") + 12);
  assert(shutdown.includes("api.prevent_close()")); assert(shutdown.includes("SHUTDOWN_STARTED.swap"));
  assert(shutdown.indexOf("spawn_blocking") < shutdown.indexOf("quick.request_shutdown"));
  assert(shutdown.indexOf("spawn_blocking") < shutdown.indexOf("supervisor.shutdown"));
  assert(shutdown.indexOf("wait_for_writes") < shutdown.indexOf("supervisor.stop_admitting()"));
  assert(host.includes("tauri_plugin_single_instance::init"));
  assert(shell.includes("queue.extend(paths)")); assert(shell.includes("std::mem::take"));
});

test("实际React启动接收hook消费启动请求，后续单实例请求复用主窗口", async () => {
  const h = openFileEffectHarness(["C:/投影测试/炮 台.litematic"]); await h.tick();
  assert.deepEqual(h.selected, ["C:/投影测试/炮 台.litematic"]); assert.deepEqual(h.routes, ["properties"]);
  await h.emit("C:/投影测试/第二个.litematic");
  assert.deepEqual(h.selected, ["C:/投影测试/炮 台.litematic", "C:/投影测试/第二个.litematic"]);
  assert.deepEqual(h.imported, ["C:/投影测试/炮 台.litematic", "C:/投影测试/第二个.litematic"]);
  assert.equal(h.pending.length, 0); assert.equal(await h.close(), true);
});

test("批量启动和后续批量请求每份都串行入库，仅最后一份激活", async () => {
  const h = openFileEffectHarness(["C:/投影测试/甲.litematic", "C:/投影测试/乙.litematic"]); await h.tick();
  assert.deepEqual(h.selected, ["C:/投影测试/乙.litematic"]);
  assert.deepEqual(h.imported, ["C:/投影测试/甲.litematic", "C:/投影测试/乙.litematic"]);
  await h.emit(["C:/投影测试/丙.litematic", "C:/投影测试/丁.litematic"]);
  assert.deepEqual(h.imported, ["C:/投影测试/甲.litematic", "C:/投影测试/乙.litematic", "C:/投影测试/丙.litematic", "C:/投影测试/丁.litematic"]);
  assert.deepEqual(h.selected, ["C:/投影测试/乙.litematic", "C:/投影测试/丁.litematic"]);
  assert.equal(h.pending.length, 0); await h.close();
});

test("NSIS卸载先确认本安装路径，仅注销自有ProgID/CLSID并保留其它默认关联", () => {
  const remove = installer.split("!macro NSIS_HOOK_PREUNINSTALL")[1].split("!macroend")[0];
  assert(remove.indexOf("nova_shell_unregister_done") < remove.indexOf("DeleteRegValue"));
  assert(remove.includes('ReadRegStr $R0 HKLM "Software\\Classes\\${NOVA_PROGID}\\shell\\open\\command"'));
  assert(remove.includes('StrCmp $R0 "${NOVA_PROGID}"'));
  assert(remove.includes("PreviousThumbnailProvider"));
  assert(!remove.includes('DeleteRegKey HKLM "Software\\Classes\\.litematic"'));
  assert(!installer.includes("UserChoice")); assert(!/RMDir\s+\/r/i.test(installer));
});
