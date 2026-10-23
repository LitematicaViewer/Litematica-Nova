import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import { JSDOM } from "jsdom";

// Wiring-fix coverage (阶段 A / B): the previously counter-only paths now delegate to
// the C.2 scan & merge kernel and the auto-activate / explicit-rescan / activation
// semantics added by the confirmed rulings. Everything is driven against a stubbed
// backend + task queue so no real file on disk is needed.
const window = new JSDOM("").window;
function compile(path, dependencies) {
  const compiled = ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } });
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports,DOMParser,XMLSerializer,TextEncoder,TextDecoder,atob,btoa){${compiled.outputText}})`)(dependencies, module, module.exports, window.DOMParser, window.XMLSerializer, TextEncoder, TextDecoder, atob, btoa);
  return module.exports;
}

const icons = compile("../src/services/libraryCategoryIcons.ts", () => { throw new Error("Unexpected icon import"); });
const paths = compile("../src/services/filePathIdentity.ts", () => { throw new Error("Unexpected path import"); });
const migration = compile("../src/services/libraryMigration.ts", (name) =>
  name === "./filePathIdentity" ? paths : name === "./libraryCategoryIcons" ? icons : (() => { throw new Error(`Unexpected migration dependency: ${name}`); })());
const contentModule = compile("../src/services/libraryContent.ts", (name) =>
  name === "./filePathIdentity" ? paths : name === "./libraryStore" ? {} : (() => { throw new Error(`Unexpected content dependency: ${name}`); })());

/** Loads `libraryStore.ts` with injectable backend / task queue / app operations. */
function loadStore({ backend = {}, tasks = {}, operations = {} } = {}) {
  const appOperations = {
    inspectProject: async () => ({ metadata: { name: "新名称", total_blocks: 4, total_volume: 4, region_count: 1 }, regions: [] }),
    editProjectMetadata: async () => ({}),
    ...operations,
  };
  const taskClient = {
    submitLibraryTask: async () => "task",
    cancelLibraryTask: async () => true,
    subscribeLibraryTask: async () => () => {},
    submitAndAwait: async () => ({ taskId: "task", kind: "scan", status: "completed", summary: {}, failures: [] }),
    awaitLibraryTask: async () => ({ taskId: "task", kind: "scan", status: "completed", summary: {}, failures: [] }),
    ...tasks,
  };
  const backendPath = {
    readUserConfigFile: async () => { throw new Error("no disk"); },
    writeUserConfigFile: async () => {},
    checkFileExists: async () => true,
    listLitematicFileEntriesInDirectory: async () => [],
    copyFileToDirectory: async () => ({}),
    computeLitematicChecksum: async () => ({ sha256: "x", byte_count: 1 }),
    getUserConfigFilePath: async () => "C:/builtin/import",
    libraryTrashMove: async () => [],
    libraryTrashRestore: async () => [],
    libraryTrashPurge: async () => [],
    libraryExportZip: async () => ({ taskId: "task" }),
    ...backend,
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
    if (name === "./minecraftVersions") return { minecraftVersionLabel: (value) => String(value) };
    if (name === "./libraryIndexClient") return { queryIndex: async () => ({ entryIds: [], ready: true }), submitIndexBuild: async () => "task", subscribeIndexStatus: async () => () => {} };
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
const folder = (over = {}) => ({
  path: "C:/lib", type: "tracked", enabled: true, recursive: true, autoActivate: true,
  sort_index: 0, lastSyncedAt: 0, lastScanCount: 0, lastError: "", ...over,
});
const state = (entries, replicas, over = {}) => ({
  schemaVersion: 5, entries, replicas, folders: [folder()], categories: [], trash: [], redirects: [], favorites: [], recent: [], ...over,
});
const scanned = (checksum, name, over = {}) => ({
  path: `C:/lib/${name}`, fileName: name, folderPath: "C:/lib", fileSize: 10, mtimeMs: 1, checksum, ...over,
});

test("A.1 入库接线：syncLocalLibraryFolder 委托 scan 内核建条目（不再只计数）", async () => {
  const calls = [];
  const store = loadStore({
    tasks: { submitAndAwait: async (kind, params) => { calls.push({ kind, params }); return { taskId: "t", kind, status: "completed", summary: { files: [scanned("AA", "A.litematic"), scanned("AA", "B.litematic")] }, failures: [] }; } },
  });
  const result = await store.syncLocalLibraryFolder(state([], []), "C:/lib");
  assert.equal(result.importedCount, 2);
  assert.equal(result.scannedFolderCount, 1);
  assert.equal(result.state.entries.length, 1, "相同校验值归并为一个条目");
  assert.equal(result.state.replicas.length, 2);
  assert.equal(result.state.entries[0].checksum, "AA");
  assert.equal(calls[0].kind, "scan");
  assert.equal(calls[0].params.folders[0].type, "tracked");
  assert.equal(calls[0].params.useCache, true);
});

test("B.1 自动激活关闭：新文件不入库，已有副本仍被跟踪", async () => {
  const store = loadStore({
    tasks: { submitAndAwait: async (kind) => ({ taskId: "t", kind, status: "completed", summary: { files: [scanned("AA", "A.litematic"), scanned("BB", "New.litematic")] }, failures: [] }) },
  });
  const base = state([entry("e1")], [replica("r1", "e1", { path: "C:/lib/A.litematic", fileName: "A.litematic" })], { folders: [folder({ autoActivate: false })] });
  const outcome = await store.scanLibraryFolders(base, {});
  assert.equal(outcome.newReplicas, 0, "自动激活关闭时不得导入新文件");
  assert.equal(outcome.state.entries.length, 1, "仅保留已入库文件的条目");
  assert.equal(outcome.state.replicas.length, 1);
  assert.equal(outcome.state.replicas[0].path, "C:/lib/A.litematic");
  assert.equal(outcome.state.replicas[0].checksum, "AA", "已有副本仍被重算/跟踪");
});

test("B.2 重新扫描全部：默认走缓存，强制重校验绕过缓存并携带 scope", async () => {
  const calls = [];
  const store = loadStore({
    tasks: { submitAndAwait: async (kind, params) => { calls.push(params); return { taskId: "t", kind, status: "completed", summary: { files: [] }, failures: [] }; } },
  });
  await store.scanLibraryFolders(state([], []), { useCache: true, forceRecheck: false });
  await store.scanLibraryFolders(state([], []), { useCache: false, forceRecheck: true });
  assert.equal(calls[0].useCache, true);
  assert.equal(calls[0].forceRecheck, false);
  assert.equal(calls[1].forceRecheck, true);
  assert.deepEqual(calls[1].scope, { kind: "all" });
});

test("B.3 激活：未入库则扫描入库并写最近；已入库仅写最近", async () => {
  let checksum = 0;
  const store = loadStore({
    backend: { computeLitematicChecksum: async () => { checksum += 1; return { sha256: `H${checksum}`, byte_count: 7 }; } },
  });
  const fresh = await store.activateProjectionRecord(state([], []), "C:/lib/New.litematic");
  assert.equal(fresh.entries.length, 1, "未入库文件被扫描入库");
  assert.equal(fresh.entries[0].checksum, "H1", "新条目不得是 checksum=null 的孤儿");
  assert.equal(fresh.replicas.length, 1);
  assert.equal(fresh.replicas[0].checksum, "H1");
  assert.deepEqual(fresh.recent[0], { kind: "entry", entryId: fresh.entries[0].id });

  const known = state([entry("e1", { checksum: "HH" })], [replica("r1", "e1", { path: "C:/lib/Existing.litematic" })]);
  const reused = await store.activateProjectionRecord(known, "C:/lib/Existing.litematic");
  assert.equal(reused.entries.length, 1, "已入库不得重复建条目");
  assert.equal(reused.replicas.length, 1);
  assert.deepEqual(reused.recent[0], { kind: "entry", entryId: "e1" });
});

test("B.3 激活：非归档文件夹仅写 path 最近，不建条目/副本", async () => {
  const store = loadStore();
  const base = state([], [], { folders: [folder({ type: "non_archived", path: "C:/nonarch" })] });
  const next = await store.activateProjectionRecord(base, "C:/nonarch/X.litematic");
  assert.equal(next.entries.length, 0);
  assert.equal(next.replicas.length, 0);
  assert.equal(next.recent[0].kind, "path");
});

test("A.3 回收区状态机：missing 仅删库记录并回报，未知 id 返回可诊断失败", async () => {
  const store = loadStore({ backend: { libraryTrashMove: async (items) => items.map((item) => ({ sourcePath: item.sourcePath, trashPath: `C:/trash/${item.trashName}`, ok: true })) } });
  const base = state([entry("e1"), entry("e2")], [
    replica("rMissing", "e1", { path: "C:/lib/Gone.litematic", status: "missing" }),
    replica("rActive", "e2", { path: "C:/lib/Alive.litematic" }),
  ]);
  const result = await store.moveReplicasToTrash(base, ["rMissing", "rActive", "ghost"]);
  assert.deepEqual(result.moved, ["rActive"], "active 副本物理移入回收区");
  assert.deepEqual(result.removedRecords, ["rMissing"], "missing 副本仅删库记录");
  assert.deepEqual(result.failures, [{ path: "ghost", reason: "record-not-found" }], "未知 id 不再静默返回");
  assert.equal(result.state.replicas.some((item) => item.id === "rMissing"), false);
  assert.equal(result.state.replicas.find((item) => item.id === "rActive").status, "trashed");
});

test("A.3 回收区刷新不重现：已 trashed 副本不被重新标记/复活", async () => {
  const store = loadStore({
    backend: { libraryTrashMove: async (items) => items.map((item) => ({ sourcePath: item.sourcePath, trashPath: `C:/trash/${item.trashName}`, ok: true })) },
    tasks: { submitAndAwait: async (kind) => ({ taskId: "t", kind, status: "completed", summary: { files: [] }, failures: [] }) },
  });
  const base = state([entry("e1")], [replica("r1", "e1", { path: "C:/lib/A.litematic" })]);
  const trashed = await store.moveReplicasToTrash(base, ["r1"]);
  const refreshed = await store.scanLibraryFolders(trashed.state, {});
  const replicaAfter = refreshed.state.replicas.find((item) => item.id === "r1");
  assert.equal(replicaAfter.status, "trashed", "文件已移走，刷新后不得复活也不得重复出现");
  assert.equal(refreshed.state.entries.find((item) => item.id === "e1").status, "retired");
});

test("A.2 分发后归并：按 checksum 重算并合并到既有条目（替代 addOrUpdateRecord）", async () => {
  const store = loadStore({
    tasks: { submitAndAwait: async (kind) => ({ taskId: "t", kind, status: "completed", summary: { files: [scanned("SHARED", "Copy.litematic")] }, failures: [] }) },
  });
  const base = state([entry("e1", { checksum: "SHARED" })], [replica("r1", "e1", { path: "C:/lib/Original.litematic", checksum: "SHARED" })]);
  const outcome = await store.scanLibraryFolders(base, { onlyFolderPath: "C:/lib" });
  assert.equal(outcome.state.entries.length, 1, "复制体按校验值并入既有条目");
  assert.equal(outcome.state.replicas.length, 2);
  assert.equal(outcome.state.replicas.every((item) => item.entryId === "e1"), true);
});

test("C.2 内置导入路径归一：相对+旧基绝对双记录合并为单一当前绝对路径（幂等、保留未知字段）", () => {
  const store = loadStore();
  const base = state([], [], {
    folders: [
      { path: "projection-library/import", type: "builtin_import", enabled: true, recursive: true, autoActivate: true, sort_index: 0, lastSyncedAt: 0, lastScanCount: 0, lastError: "", customFlag: "keep-me" },
      { path: "C:/old-base/import", type: "builtin_import", enabled: true, recursive: true, autoActivate: true, sort_index: 1, lastSyncedAt: 5, lastScanCount: 3, lastError: "旧状态" },
      folder({ path: "C:/lib", sort_index: 2 }),
    ],
  });
  const next = store.ensureBuiltinImportFolder(base, "C:/builtin/import");
  const builtins = next.folders.filter((item) => item.type === "builtin_import");
  assert.equal(builtins.length, 1, "不得并存两个内置导入文件夹");
  assert.equal(builtins[0].path, "C:/builtin/import", "统一替换为当前绝对路径");
  assert.equal(builtins[0].customFlag, "keep-me", "未知字段保留");
  assert.equal(builtins[0].lastSyncedAt, 5, "取最新同步状态");
  assert.equal(builtins[0].lastScanCount, 3);
  assert.equal(next.folders.some((item) => item.path === "projection-library/import"), false, "迁移遗留相对记录被去除");
  assert.equal(next.folders.some((item) => item.path === "C:/old-base/import"), false, "旧基目录记录被合并");
  assert.deepEqual(store.ensureBuiltinImportFolder(next, "C:/builtin/import"), next, "归一化必须幂等");
});

test("C.2 提交绝对路径：相对遗留文件夹不进入 scan 提交（避免整次提交失败）", async () => {
  let refs = null;
  const store = loadStore({
    tasks: { submitAndAwait: async (kind, params) => { refs = params.folders; return { taskId: "t", kind, status: "completed", summary: { files: [] }, failures: [] }; } },
  });
  const base = state([], [], { folders: [folder({ path: "projection-library/import", type: "tracked" }), folder({ path: "C:/lib" })] });
  await store.scanLibraryFolders(base, {});
  assert.equal(refs.some((item) => item.path === "projection-library/import"), false, "相对路径被剔除");
  assert.deepEqual(refs.filter((item) => item.path === "C:/lib"), [{ path: "C:/lib", type: "tracked" }]);
});

test("C.2 扫描前兜底：扫描前把内置导入别名归一到绝对路径并触发目录创建", async () => {
  let derived = 0;
  let refs = null;
  const store = loadStore({
    backend: { getUserConfigFilePath: async () => { derived += 1; return "C:/builtin/import"; } },
    tasks: { submitAndAwait: async (kind, params) => { refs = params.folders; return { taskId: "t", kind, status: "completed", summary: { files: [] }, failures: [] }; } },
  });
  const base = state([], [], { folders: [folder({ path: "projection-library/import", type: "builtin_import" })] });
  await store.scanLibraryFolders(base, {});
  assert.equal(derived, 1, "提交前必须重新派生内置导入绝对路径以触发 create_dir_all");
  assert.deepEqual(refs, [{ path: "C:/builtin/import", type: "tracked" }], "遗留相对路径被归一为绝对路径后才提交");
});

test("C.2 元数据回填：新建条目解析并回填 stats", async () => {
  let analyzeCalls = 0;
  const store = loadStore({
    operations: { inspectProject: async () => { analyzeCalls += 1; return { metadata: { name: "内部名", total_blocks: 42, total_volume: 100, region_count: 2, enclosing_size: { x: 1, y: 2, z: 3 }, minecraft_data_version: 7 }, regions: [{ name: "选区一" }, { name: "选区二" }] }; } },
    tasks: { submitAndAwait: async (kind) => ({ taskId: "t", kind, status: "completed", summary: { files: [scanned("NEW", "N.litematic")] }, failures: [] }) },
  });
  const outcome = await store.scanLibraryFolders(state([], []), {});
  assert.equal(analyzeCalls, 1, "新建条目解析一次");
  assert.equal(outcome.state.entries.length, 1);
  assert.equal(outcome.state.entries[0].internalName, "内部名");
  assert.deepEqual(outcome.state.entries[0].stats, {
    totalBlocks: 42, totalVolume: 100, regionCount: 2, regionNames: ["选区一", "选区二"],
    enclosingSize: { x: 1, y: 2, z: 3 }, minecraftDataVersion: 7,
  });
});

test("C.2 元数据回填：解析失败仅置 parse_error，不使整次扫描失败", async () => {
  const store = loadStore({
    operations: { inspectProject: async () => { throw new Error("坏文件"); } },
    tasks: { submitAndAwait: async (kind) => ({ taskId: "t", kind, status: "completed", summary: { files: [scanned("BAD", "Bad.litematic")] }, failures: [] }) },
  });
  const outcome = await store.scanLibraryFolders(state([], []), {});
  assert.equal(outcome.status, "completed", "解析失败不得使整次扫描失败");
  assert.equal(outcome.state.entries.length, 1, "坏文件仍建条目（parse_error 语义）");
  assert.equal(outcome.state.replicas[0].status, "parse_error");
  assert.equal(outcome.state.replicas[0].lastError, "Error: 坏文件");
});

test("C.2 元数据回填：变更/复活才重解析，未变更条目不重复解析", async () => {
  let analyzeCalls = 0;
  const operations = { inspectProject: async () => { analyzeCalls += 1; return { metadata: { total_blocks: 9 }, regions: [] }; } };
  const unchanged = loadStore({
    operations,
    tasks: { submitAndAwait: async (kind) => ({ taskId: "t", kind, status: "completed", summary: { files: [scanned("AA", "A.litematic")] }, failures: [] }) },
  });
  const base = state([entry("e1", { checksum: "AA" })], [replica("r1", "e1", { path: "C:/lib/A.litematic", checksum: "AA", fileSize: 10, mtime: 1 })]);
  await unchanged.scanLibraryFolders(base, {});
  assert.equal(analyzeCalls, 0, "已入库且未变更不重复解析");

  const changed = loadStore({
    operations,
    tasks: { submitAndAwait: async (kind) => ({ taskId: "t", kind, status: "completed", summary: { files: [scanned("AA", "A.litematic", { fileSize: 20 })] }, failures: [] }) },
  });
  await changed.scanLibraryFolders(base, {});
  assert.equal(analyzeCalls, 1, "size/mtime 变化触发重解析");

  const revived = loadStore({
    operations,
    tasks: { submitAndAwait: async (kind) => ({ taskId: "t", kind, status: "completed", summary: { files: [scanned("AA", "A.litematic")] }, failures: [] }) },
  });
  const missing = state([entry("e1", { checksum: "AA" })], [replica("r1", "e1", { path: "C:/lib/A.litematic", checksum: "AA", fileSize: 10, mtime: 1, status: "missing" })]);
  await revived.scanLibraryFolders(missing, {});
  assert.equal(analyzeCalls, 2, "missing→active 复活触发重解析");
});