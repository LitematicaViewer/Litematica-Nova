import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

// F.6 background-task client (design 2.1.3 / 2.2.2(B) / 2.2.3 / tasks.md F.6).
// The single serial queue itself lives in Rust; this suite drives the TS client
// against a serial fake host and pins submit / progress / cancel / per-item
// failure reporting, the finished-event cache boundary, and subscription teardown.

const PROGRESS = "projection-library-task-progress";
const FINISHED = "projection-library-task-finished";
const QUEUE = "projection-library-task-queue-changed";

function compile(path, dependencies) {
  const compiled = ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } });
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${compiled.outputText}})`)(dependencies, module, module.exports);
  return module.exports;
}
const tick = () => new Promise((resolve) => setImmediate(resolve));

function createHost() {
  const handlers = new Map();
  const plans = [];
  const submitted = [];
  const log = [];
  const queue = [];
  const cancelled = new Set();
  let nextId = 0;
  let running = 0;
  let maxConcurrent = 0;
  let queued = { pending: [], running: [] };
  let immediateFinishId = null;

  const emit = (event, payload) => { const handler = handlers.get(event); if (handler) handler({ payload }); };

  const files = {
    libraryTaskSubmit: async (kind, params) => {
      const taskId = `t${++nextId}`;
      const spec = plans.shift() || {};
      submitted.push({ taskId, kind, params });
      queue.push({ taskId, kind, spec });
      if (spec.immediateFinish) {
        // Finishes inside the submit window, before `submitAndAwait` registers its waiter.
        emit(FINISHED, { taskId, kind, status: "completed", summary: spec.summary || {}, failures: spec.failures || [] });
      }
      return { taskId };
    },
    libraryTaskCancel: async (taskId) => { cancelled.add(taskId); return { cancelled: true }; },
    libraryTaskStatus: async () => ({ pending: queued.pending.map((item) => item.taskId), running: queued.running.map((item) => item.taskId) }),
  };

  const listenEvent = (event, callback) => { handlers.set(event, callback); return () => handlers.delete(event); };

  async function pump() {
    const item = queue.shift();
    if (!item) return;
    running += 1;
    maxConcurrent = Math.max(maxConcurrent, running);
    log.push(`start:${item.taskId}`);
    for (const step of item.spec.progress || []) emit(PROGRESS, { taskId: item.taskId, kind: item.kind, phase: step.phase || "run", ...step });
    const status = cancelled.has(item.taskId) ? "cancelled" : item.spec.fail ? "failed" : "completed";
    emit(FINISHED, { taskId: item.taskId, kind: item.kind, status, summary: item.spec.summary || {}, failures: item.spec.failures || [] });
    log.push(`finish:${item.taskId}`);
    running -= 1;
  }
  async function runAll() { while (queue.length) { await pump(); await tick(); } }

  return {
    files, listenEvent, emit, tick, runAll, plan: (spec) => plans.push(spec),
    submitted, log, cancel: (taskId) => { cancelled.add(taskId); }, emitQueue: (snapshot) => emit(QUEUE, snapshot),
    get maxConcurrent() { return maxConcurrent; },
  };
}

const loadTasks = (host) => compile("../src/services/libraryTasks.ts", (name) => {
  if (name === "../platform/events") return { listenEvent: host.listenEvent };
  if (name === "../platform/files") return host.files;
  throw new Error(`Unexpected dependency: ${name}`);
});

test("submitLibraryTask 透传任务类型与参数并返回任务 id", async () => {
  const host = createHost();
  const tasks = loadTasks(host);
  host.plan({});
  const id = await tasks.submitLibraryTask("scan", { folders: [{ path: "C:/lib", type: "tracked" }], forceRecheck: true });
  assert.equal(id, "t1");
  assert.equal(host.submitted[0].kind, "scan");
  assert.deepEqual(host.submitted[0].params, { folders: [{ path: "C:/lib", type: "tracked" }], forceRecheck: true });
});

test("subscribeLibraryTask 分发 progress / finished / queue-changed，退订后停止", async () => {
  const host = createHost();
  const tasks = loadTasks(host);
  const seen = { progress: [], finished: [], queue: [] };
  const unsubscribe = await tasks.subscribeLibraryTask({
    onProgress: (payload) => seen.progress.push(payload.processed),
    onFinished: (payload) => seen.finished.push(payload.status),
    onQueueChanged: (snapshot) => seen.queue.push(snapshot.running.length),
  });
  host.emitQueue({ pending: [], running: ["t1"] });
  host.plan({ progress: [{ processed: 1, total: 2 }] });
  await tasks.submitLibraryTask("scan", {});
  await host.runAll();
  assert.deepEqual(seen.progress, [1]);
  assert.deepEqual(seen.finished, ["completed"]);
  assert.deepEqual(seen.queue, [1]);
  unsubscribe();
  host.emitQueue({ pending: [], running: [] });
  assert.equal(seen.queue.length, 1, "退订后不再收到事件");
});

test("submitAndAwait 透传逐项失败回报与 summary，不 reject 而是以 failed 终结", async () => {
  const host = createHost();
  const tasks = loadTasks(host);
  host.plan({ fail: true, summary: { copied: 1, failed: 2 }, failures: [{ path: "C:/a.litematic", reason: "permission" }, { path: "C:/b.litematic", reason: "occupied" }] });
  const promise = tasks.submitAndAwait("batch-copy", {});
  await tick();
  await host.runAll();
  const result = await promise;
  assert.equal(result.status, "failed");
  assert.equal(result.kind, "batch-copy");
  assert.deepEqual(result.failures, [{ path: "C:/a.litematic", reason: "permission" }, { path: "C:/b.litematic", reason: "occupied" }]);
  assert.deepEqual(result.summary, { copied: 1, failed: 2 });
});

test("cancelLibraryTask 返回取消结果，submitAndAwait 以 cancelled 终结且保留中间进度", async () => {
  const host = createHost();
  const tasks = loadTasks(host);
  host.plan({ progress: [{ processed: 1, total: 5 }] });
  const progress = [];
  const promise = tasks.submitAndAwait("scan", {}, { onProgress: (payload) => progress.push(payload.processed) });
  await tick();
  const id = host.submitted[0].taskId;
  const confirmed = await tasks.cancelLibraryTask(id);
  assert.equal(confirmed, true);
  await host.runAll();
  const result = await promise;
  assert.equal(result.status, "cancelled");
  assert.deepEqual(progress, [1], "取消前已完成的项中间结果保留");
});

test("提交与注册之间的完成事件不丢失（finished 缓存边界）", async () => {
  const host = createHost();
  const tasks = loadTasks(host);
  host.plan({ immediateFinish: true, summary: { files: 3 } });
  const result = await tasks.submitAndAwait("scan", {});
  assert.equal(result.status, "completed");
  assert.deepEqual(result.summary, { files: 3 });
});

test("awaitLibraryTask 按 taskId 过滤，忽略其它任务的终结事件", async () => {
  const host = createHost();
  const tasks = loadTasks(host);
  const first = await tasks.submitLibraryTask("scan", {});
  const second = await tasks.submitLibraryTask("index", {});
  host.plan({});
  // Re-plan order does not matter: the awaited id is what is resolved.
  const awaited = tasks.awaitLibraryTask(second);
  await tick();
  await host.runAll();
  const result = await awaited;
  assert.equal(result.taskId, second);
  assert.equal(result.kind, "index");
  assert.notEqual(first, second);
});

test("queryLibraryTaskQueue 透传队列快照", async () => {
  const host = createHost();
  const tasks = loadTasks(host);
  const snapshot = await tasks.queryLibraryTaskQueue();
  assert.deepEqual(snapshot, { pending: [], running: [] });
});

test("resetLibraryTaskClientForTests 断开监听，之后重新订阅可再次工作", async () => {
  const host = createHost();
  const tasks = loadTasks(host);
  let hits = 0;
  await tasks.subscribeLibraryTask({ onQueueChanged: () => { hits += 1; } });
  host.emitQueue({ pending: [], running: [] });
  assert.equal(hits, 1);
  tasks.resetLibraryTaskClientForTests();
  host.emitQueue({ pending: [], running: [] });
  assert.equal(hits, 1, "重置后旧监听不再生效");
  await tasks.subscribeLibraryTask({ onQueueChanged: () => { hits += 1; } });
  host.emitQueue({ pending: [], running: [] });
  assert.equal(hits, 2, "重置后可重新订阅");
});

test("单队列串行（集成冒烟）：任务按提交顺序逐个完成，同一时刻仅一个运行", async () => {
  const host = createHost();
  const tasks = loadTasks(host);
  host.plan({ progress: [{ processed: 1, total: 1 }] });
  host.plan({ progress: [{ processed: 1, total: 1 }] });
  const firstId = await tasks.submitLibraryTask("scan", {});
  const first = tasks.awaitLibraryTask(firstId);
  const secondId = await tasks.submitLibraryTask("checksum", {});
  const second = tasks.awaitLibraryTask(secondId);
  await tick();
  await host.runAll();
  const [r1, r2] = await Promise.all([first, second]);
  assert.equal(host.maxConcurrent, 1);
  assert.deepEqual(host.log, ["start:t1", "finish:t1", "start:t2", "finish:t2"]);
  assert.equal(r1.kind, "scan");
  assert.equal(r2.kind, "checksum");
});

// Regression guard: `submitAndAwait` must resolve only on its own terminal
// event (`payload.taskId` match), so concurrent awaits never cross-resolve.
test("并发 submitAndAwait 按 taskId 分辨终结事件，互不错配", async () => {
  const host = createHost();
  const tasks = loadTasks(host);
  host.plan({});
  host.plan({});
  const first = tasks.submitAndAwait("scan", {});
  const second = tasks.submitAndAwait("checksum", {});
  await tick();
  await host.runAll();
  const [r1, r2] = await Promise.all([first, second]);
  assert.equal(r1.kind, "scan", "第一个 await 应解析到自己的 scan 任务");
  assert.equal(r2.kind, "checksum", "第二个 await 应解析到自己的 checksum 任务");
});