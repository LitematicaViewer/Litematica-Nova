import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import { JSDOM } from "jsdom";

// F.8 background index (tasks.md F.8 / 4.1.4 / 5.8.1.5 / 5.8.3.1 / 裁决 4).
// The index carrier is Rust (`library_index.rs`); this suite pins the TS projection
// (`libraryIndexEntries`), the incremental submit, query/readiness passthrough, the
// client-side readiness fill-in, and the fixed persistence file name.
const window = new JSDOM("").window;
function compile(path, dependencies) {
  const compiled = ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } });
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports,DOMParser,XMLSerializer,TextEncoder,TextDecoder,atob,btoa){${compiled.outputText}})`)(dependencies, module, module.exports, window.DOMParser, window.XMLSerializer, TextEncoder, TextDecoder, atob, btoa);
  return module.exports;
}

const icons = compile("../src/services/libraryCategoryIcons.ts", (name) => name === "./i18n" ? { t: (key) => key } : (() => { throw new Error("Unexpected icon import"); })());
const paths = compile("../src/services/filePathIdentity.ts", () => { throw new Error("Unexpected path import"); });
const migration = compile("../src/services/libraryMigration.ts", (name) =>
  name === "./filePathIdentity" ? paths : name === "./libraryCategoryIcons" ? icons : (() => { throw new Error(`Unexpected migration dependency: ${name}`); })());
const contentModule = compile("../src/services/libraryContent.ts", (name) =>
  name === "./filePathIdentity" ? paths : name === "./libraryStore" ? {} : (() => { throw new Error(`Unexpected content dependency: ${name}`); })());

function loadStore({ index = {} } = {}) {
  const appOperations = { inspectProject: async () => ({ metadata: {}, regions: [] }), editProjectMetadata: async () => ({}) };
  const indexClient = { queryIndex: async () => ({ entries: [], ready: true }), submitIndexBuild: async () => "task", subscribeIndexStatus: async () => () => {}, ...index };
  const taskClient = {
    submitLibraryTask: async () => "task", cancelLibraryTask: async () => true, subscribeLibraryTask: async () => () => {},
    submitAndAwait: async () => ({ taskId: "task", kind: "index", status: "completed", summary: {}, failures: [] }),
    awaitLibraryTask: async () => ({ taskId: "task", kind: "index", status: "completed", summary: {}, failures: [] }),
  };
  const backendPath = {
    readUserConfigFile: async () => { throw new Error("no disk"); }, writeUserConfigFile: async () => {},
    checkFileExists: async () => true, listLitematicFileEntriesInDirectory: async () => [],
    copyFileToDirectory: async () => ({}), computeLitematicChecksum: async () => ({ sha256: "x", byte_count: 1 }),
    getUserConfigFilePath: async () => "C:/builtin/import", libraryTrashMove: async () => [],
    libraryTrashRestore: async () => [], libraryTrashPurge: async () => [], libraryExportZip: async () => ({ taskId: "task" }),
  };
  return compile("../src/services/libraryStore.ts", (name) => {
    if (name === "./libraryCategoryIcons") return icons;
    if (name === "./filePathIdentity") return paths;
    if (name === "./backend") return backendPath;
    if (name === "../platform/applicationOperations") return appOperations;
    if (name === "./libraryMigration") return migration;
    if (name === "./libraryContent") return contentModule;
    if (name === "./libraryTasks") return taskClient;
    if (name === "./statsService") return { formatMaterialQuantity: () => "", loadStructureStats: async () => ({ materials: [] }) };
    if (name === "./minecraftVersions") return { minecraftVersionLabel: (value) => `v${value}` };
    if (name === "./libraryIndexClient") return indexClient;
    throw new Error(`Unexpected store dependency: ${name}`);
  });
}

const entry = (id, over = {}) => ({
  id, createdAt: 0, checksum: null, status: "active", categoryIds: [], displayName: id,
  internalName: undefined, author: "", description: "", tags: [], preview: {},
  stats: { totalBlocks: 0, totalVolume: 0, regionCount: 0, enclosingSize: { x: 0, y: 0, z: 0 }, minecraftDataVersion: 0 },
  sortIndex: 0, lastAnalyzedAt: 0, lastError: "", ...over,
});
const replica = (id, entryId, over = {}) => ({
  id, entryId, path: `C:/lib/${id}.litematic`, fileName: `${id}.litematic`, folderPath: "C:/lib",
  checksum: null, fileSize: 0, mtime: 0, status: "active", lastAnalyzedAt: 0, lastError: "", ...over,
});
const state = (entries, replicas, over = {}) => ({
  schemaVersion: 5, entries, replicas, folders: [], categories: [], trash: [], redirects: [], favorites: [], recent: [], ...over,
});
const category = (id, name, parentId = null) => ({ id, name, parentId, sort_index: 0, icon: { kind: "lucide", name: "Folder" } });

test("索引投影：仅收纳 active 条目，字段映射含 tags 与 categoryPath", () => {
  const store = loadStore();
  const subject = state(
    [
      entry("e1", { displayName: "红石机", author: "作者", description: "说明", tags: ["红石", "自动化"], categoryIds: ["child"], stats: { ...entry().stats, minecraftDataVersion: 3953 } }),
      entry("e2", { status: "retired" }),
      entry("e3", { status: "merged" }),
    ],
    [replica("r1", "e1", { fileName: "红石机.litematic" }), replica("r2", "e2", { status: "missing" })],
    { categories: [category("root", "机器"), category("child", "红石", "root")] },
  );
  const entries = store.libraryIndexEntries(subject);
  assert.equal(entries.length, 1, "retired / merged 条目不进入索引");
  const [only] = entries;
  assert.equal(only.entryId, "e1");
  assert.equal(only.displayName, "红石机");
  assert.equal(only.fileName, "红石机.litematic");
  assert.equal(only.author, "作者");
  assert.equal(only.description, "说明");
  assert.deepEqual(only.tags, ["红石", "自动化"], "tags 为库内权威镜像，直接进入索引");
  assert.equal(only.categoryPath, "机器 / 红石");
  assert.equal(only.version, "v3953");
});

test("索引投影：version 未知为空串（不产生索引噪声），fileName 取首个活跃副本", () => {
  const store = loadStore();
  const subject = state([entry("e1", { stats: { ...entry().stats, minecraftDataVersion: 0 } })], [
    replica("r1", "e1", { fileName: "first.litematic", status: "missing" }),
    replica("r2", "e1", { fileName: "second.litematic", status: "active" }),
  ]);
  const [only] = store.libraryIndexEntries(subject);
  assert.equal(only.version, "", "0 版本不产生标签");
  assert.equal(only.fileName, "second.litematic", "优先取活跃副本的文件名");
});

test("refreshLibraryIndex 只提交当前 active 条目集（增量，不传退役项）", async () => {
  let received;
  const store = loadStore({ index: { submitIndexBuild: async (entries) => { received = entries; return "idx-1"; } } });
  const subject = state([entry("e1"), entry("e2", { status: "retired" })], [replica("r1", "e1"), replica("r2", "e2", { status: "missing" })]);
  const taskId = await store.refreshLibraryIndex(subject);
  assert.equal(taskId, "idx-1");
  assert.deepEqual(received.map((item) => item.entryId), ["e1"]);
});

test("queryLibraryIndex 透传查询，未就绪时返回已就绪的部分", async () => {
  let received;
  const store = loadStore({ index: { queryIndex: async (query) => { received = query; return { entries: ["e1"], ready: false }; } } });
  const result = await store.queryLibraryIndex({ text: "红石", fields: ["displayName", "tags"] });
  assert.deepEqual(received, { text: "红石", fields: ["displayName", "tags"] });
  assert.deepEqual(result, { entries: ["e1"], ready: false });
});

test("索引客户端：提交 index 任务、查询/就绪透传，完成时补齐就绪状态", async () => {
  const handlers = new Map();
  let submitted;
  let ready = false;
  const files = {
    libraryIndexQuery: async (query) => ({ entries: ["e1"], ready, echo: query }),
    libraryIndexStatus: async () => ({ entries: 3, ready }),
    libraryTaskSubmit: async (kind, params) => { submitted = { kind, params }; return { taskId: "idx-1" }; },
  };
  const taskClient = { subscribeLibraryTask: async (subscriptions) => { handlers.set("finished", subscriptions.onFinished); return () => handlers.delete("finished"); } };
  const client = compile("../src/services/libraryIndexClient.ts", (name) =>
    name === "../platform/files" ? files : name === "./libraryTasks" ? taskClient : (() => { throw new Error(`Unexpected index client dependency: ${name}`); })());

  const seen = [];
  await client.subscribeIndexStatus((status) => seen.push(status.ready));
  assert.deepEqual(seen, [false], "订阅时应立即发出当前就绪状态");
  ready = true;
  handlers.get("finished")({ taskId: "idx-1", kind: "index", status: "completed", summary: {}, failures: [] });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(seen, [false, true], "index 任务完成后应重新查询并就绪");
  handlers.get("finished")({ taskId: "x", kind: "scan", status: "completed", summary: {}, failures: [] });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(seen, [false, true], "非 index 任务不应触发补查");

  const taskId = await client.submitIndexBuild([{ entryId: "e1" }]);
  assert.equal(taskId, "idx-1");
  assert.equal(submitted.kind, "index");
  assert.deepEqual(submitted.params, { entries: [{ entryId: "e1" }] });
  assert.deepEqual(await client.queryIndex({ text: "x" }), { entries: ["e1"], ready: true, echo: { text: "x" } });
});

test("索引持久化固定文件名为 projection-library/index/index.json（裁决 4）", () => {
  const source = readFileSync(new URL("../src-tauri/src/application/library_index.rs", import.meta.url), "utf8");
  assert.match(source, /projection-library\/index/);
  assert.match(source, /index\.json/);
});