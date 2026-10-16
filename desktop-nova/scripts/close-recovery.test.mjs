import assert from "node:assert/strict";
import test from "node:test";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

function fixture({ windowLabel = "main" } = {}) {
  const values = new Map(), events = new Map(), logs = [], effects = [], states = [], cleanups = [];
  let failWrite = false, confirmResolve, holdConfirm = false, pending = null, requests = 0;
  const api = {};
  const storage = { getItem: key => values.get(key) || null, setItem: (key, value) => { logs.push("store"); if (failWrite) throw Error("quota exceeded"); values.set(key, value); } };
  const source = fs.readFileSync(new URL("../ui/windows/main/updateRecovery.ts", import.meta.url), "utf8");
  vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText,
    { exports: api, localStorage: storage, console: { error() {}, info() {} }, require: id => {
      if (id === "react") return { useRef: current => ({ current }), useEffect: fn => effects.push(fn), useState: value => [value, next => states.push(next)] };
      if (id === "../../../src/platform/nativeWindowGeometry") return {
        getApplicationWindowLabel: () => windowLabel,
        subscribeApplicationWindowCloseRequested: async fn => { events.set("native-close", fn); return () => events.delete("native-close"); },
      };
      if (id === "../../../src/platform/events") return { listenEvent: async (name, fn) => { events.set(name, fn); return () => events.delete(name); } };
      if (id === "../../../src/platform/applicationOperations") return {
        getApplicationCloseRequest: async () => pending,
        confirmApplicationClose: async (requestId, draftsPreserved) => { logs.push({ requestId, draftsPreserved }); if (holdConfirm) await new Promise(resolve => { confirmResolve = resolve; }); },
        requestApplicationClose: async () => { requests++; },
      };
      throw Error(id);
    } });
  return { api, logs, states, values, events, fail: () => { failWrite = true; }, recover: () => { failWrite = false; }, hold: () => { holdConfirm = true; }, release: () => confirmResolve?.(),
    pending: request => { pending = request; }, mount: () => { for (const fn of effects.splice(0)) cleanups.push(fn()); }, dispose: () => cleanups.forEach(fn => fn?.()), requests: () => requests };
}
const tick = () => new Promise(resolve => setTimeout(resolve, 0));

test("normal X synchronously preserves the latest live draft before Host ACK, never writes the source", async () => {
  const f = fixture(); f.api.useUpdateDraft("properties:file", { form: { name: "未保存" } }); f.mount();
  await f.api.preserveDraftsForClose(7);
  assert.equal(f.logs[0], "store"); assert.equal(f.logs[1].requestId, 7); assert.equal(f.logs[1].draftsPreserved, true);
  assert.equal(f.api.takeUpdateDraft("properties:file").form.name, "未保存");
});
test("quota failure sends false ACK, keeps drafts intact and permits a new close attempt", async () => {
  const f = fixture(); f.api.useUpdateDraft("generate", { text: "draft" }); f.mount(); f.fail();
  await assert.rejects(f.api.preserveDraftsForClose(1), /草稿保留失败/);
  assert.equal(f.logs[1].draftsPreserved, false); assert.equal(f.values.size, 0);
  f.recover(); await f.api.preserveDraftsForClose(2);
  assert.equal(f.logs.at(-1).draftsPreserved, true); assert.equal(f.api.readUpdateDraft("generate").text, "draft");
});
test("repeated native events for one request preserve and ACK only once", async () => {
  const f = fixture(); f.hold();
  const first = f.api.preserveDraftsForClose(11); await f.api.preserveDraftsForClose(11);
  assert.equal(f.logs.filter(item => item === "store").length, 1); assert.equal(f.logs.filter(item => typeof item === "object").length, 1);
  f.release(); await first; await f.api.preserveDraftsForClose(11); assert.equal(f.logs.length, 2);
});
test("early independent listener recovers a pending X, explicit cancellation is actionable and unmount removes callbacks", async () => {
  const f = fixture(); f.pending({ requestId: 31 }); const hook = f.api.useApplicationCloseRecovery(); f.mount(); await tick();
  assert.equal(f.logs.at(-1).requestId, 31);
  f.events.get("nova-close-cancelled")({ payload: { message: "请重试关闭" } }); assert.equal(f.states.at(-1), "请重试关闭");
  await hook.retry(); assert.equal(f.requests(), 1);
  f.dispose(); await tick(); assert.equal(f.events.size, 0);
});

test("a delayed Host warning and repeated X keep one preserved draft and the same in-flight ACK", async () => {
  const f = fixture(); f.api.useUpdateDraft("shell", { currentFile: "1700w苦情巨树.litematic", route: "render" });
  f.api.useApplicationCloseRecovery(); f.mount(); await tick(); f.pending({ requestId: 51 }); f.hold();
  f.events.get("nova-close-requested")({ payload: { requestId: 51 } }); await tick();
  assert.equal(f.api.readUpdateDraft("shell").route, "render");
  assert.equal(f.logs.at(-1).requestId, 51); assert.equal(f.logs.at(-1).draftsPreserved, true);
  // The Host warning is not a cancellation: request 51 remains authoritative
  // even while its already-sent confirmation is queued behind render IPC.
  f.events.get("nova-close-delayed")({ payload: { requestId: 51, message: "正在保留草稿并关闭" } });
  f.events.get("native-close")();
  f.events.get("nova-close-requested")({ payload: { requestId: 51 } }); await tick();
  assert.equal(f.logs.filter(item => item === "store").length, 1);
  assert.equal(f.logs.filter(item => item?.requestId === 51).length, 1);
  f.release(); await tick(); const messages = f.states.length;
  f.events.get("nova-close-delayed")({ payload: { requestId: 51, message: "迟到警告" } });
  await tick(); assert.equal(f.states.length, messages, "a delivered ACK cannot be turned into a new close error by a stale warning");
  f.dispose(); await tick(); assert.equal(f.events.size, 0);
});

test("native X recovers the same Host request when the custom event is absent, without waiting for a render", async () => {
  const f = fixture(); f.api.useUpdateDraft("shell", { currentFile: "1700w苦情巨树.litematic" });
  f.api.useApplicationCloseRecovery(); f.mount(); await tick();
  // No nova-close-requested delivery. A pending render never participates in
  // the close protocol, which only preserves edits and acknowledges the Host.
  f.pending({ requestId: 42 }); f.hold();
  f.events.get("native-close")(); await tick();
  assert.equal(f.logs.filter(item => item?.requestId === 42).length, 1);
  assert.equal(f.logs.at(-1).draftsPreserved, true);
  assert.equal(f.api.readUpdateDraft("shell").currentFile, "1700w苦情巨树.litematic");
  f.events.get("nova-close-requested")({ payload: { requestId: 42 } });
  await tick(); assert.equal(f.logs.filter(item => item?.requestId === 42).length, 1);
  f.release(); await tick(); f.dispose(); await tick(); assert.equal(f.events.size, 0);
});

test("auxiliary windows do not install the main-window close gate", async () => {
  const f = fixture({ windowLabel: "enumerator" });
  const hook = f.api.useApplicationCloseRecovery(); f.mount(); await tick();
  assert.equal(f.events.size, 0); await hook.retry(); assert.equal(f.requests(), 0);
});

test("native close platform seam prevents API auto-destroy and leaves exit to Host", async () => {
  const api = {}; let handler, prevented = false, notified = false;
  const code = ts.transpileModule(fs.readFileSync(new URL("../src/platform/nativeWindowGeometry.ts", import.meta.url), "utf8"),
    { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(code, { exports: api, require: id => {
    assert.equal(id, "@tauri-apps/api/window");
    return { getCurrentWindow: () => ({ onCloseRequested: async listener => { handler = listener; return () => {}; } }) };
  } });
  await api.subscribeApplicationWindowCloseRequested(() => { assert.equal(prevented, true); notified = true; });
  handler({ preventDefault: () => { prevented = true; } }); assert.equal(notified, true);
});
