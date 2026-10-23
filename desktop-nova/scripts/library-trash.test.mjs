import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import { JSDOM } from "jsdom";

// F.7 recycle-bin semantics (tasks.md F.7 / 5.6.x / 4.3.2 / C.11 / B.8).
// The physical file work is a Rust command (`library_trash_move/restore/purge`); this
// suite drives the TS orchestration against a stubbed backend and pins the record
// lifecycle: move / restore / purge, conflict rename, legacy cleanup and duplicate
// counting exclusion.
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

function loadStore({ backend = {}, index = {}, tasks = {}, stats = {} } = {}) {
  const appOperations = { inspectProject: async () => ({ metadata: {}, regions: [] }), editProjectMetadata: async () => ({}) };
  const indexClient = { queryIndex: async () => ({ entries: [], ready: true }), submitIndexBuild: async () => "task", subscribeIndexStatus: async () => () => {}, ...index };
  const taskClient = {
    submitLibraryTask: async () => "task", cancelLibraryTask: async () => true, subscribeLibraryTask: async () => () => {},
    submitAndAwait: async () => ({ taskId: "task", kind: "export-zip", status: "completed", summary: {}, failures: [] }),
    awaitLibraryTask: async () => ({ taskId: "task", kind: "export-zip", status: "completed", summary: {}, failures: [] }),
    ...tasks,
  };
  const statsService = { formatMaterialQuantity: () => "", loadStructureStats: async () => ({ materials: [] }), ...stats };
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
    if (name === "./statsService") return statsService;
    if (name === "./minecraftVersions") return { minecraftVersionLabel: (value) => String(value) };
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

test("移入回收区：成功项置 trashed 并建 in_trash 记录，清空活跃副本的条目退役", async () => {
  const store = loadStore({ backend: { libraryTrashMove: async (items) => items.map((item) => ({ sourcePath: item.sourcePath, trashPath: `C:/trash/${item.trashName}`, ok: true })) } });
  const subject = state([entry("e1", { categoryIds: ["c"] }), entry("e2")], [
    replica("r1", "e1", { path: "C:/lib/A.litematic" }),
    replica("r2", "e2", { path: "C:/lib/B.litematic" }),
  ], { categories: [{ id: "c", name: "分类", parentId: null, sort_index: 0, icon: { kind: "lucide", name: "Folder" } }] });
  const { state: next, moved, failures } = await store.moveReplicasToTrash(subject, ["r1"]);
  assert.deepEqual(moved, ["r1"]);
  assert.deepEqual(failures, []);
  assert.equal(next.replicas.find((item) => item.id === "r1").status, "trashed");
  assert.equal(next.replicas.find((item) => item.id === "r2").status, "active");
  assert.equal(next.entries.find((item) => item.id === "e1").status, "retired", "无活跃副本的条目退役");
  assert.equal(next.entries.find((item) => item.id === "e2").status, "active");
  assert.equal(next.trash.length, 1);
  assert.equal(next.trash[0].status, "in_trash");
  assert.equal(next.trash[0].originalPath, "C:/lib/A.litematic");
  assert.equal(next.trash[0].trashPath, "C:/trash/r1.litematic");
  assert.equal(next.trash[0].replicaSnapshot.id, "r1");
});

test("移入回收区：物理移动失败逐项回报，失败副本保持 active 不落库", async () => {
  const store = loadStore({ backend: { libraryTrashMove: async (items) => items.map((item) => ({ sourcePath: item.sourcePath, trashPath: "", ok: false, reason: "occupied" })) } });
  const subject = state([entry("e1")], [replica("r1", "e1", { path: "C:/lib/A.litematic" })]);
  const { state: next, moved, failures } = await store.moveReplicasToTrash(subject, ["r1"]);
  assert.deepEqual(moved, []);
  assert.deepEqual(failures, [{ path: "C:/lib/A.litematic", reason: "occupied" }]);
  assert.equal(next.replicas[0].status, "active", "失败项不得被标记为已移入");
  assert.equal(next.trash.length, 0);
});

test("回收区副本不计重数（审计排除 trashed）", async () => {
  const store = loadStore({ backend: { libraryTrashMove: async (items) => items.map((item) => ({ sourcePath: item.sourcePath, trashPath: `C:/trash/${item.trashName}`, ok: true })) } });
  const subject = state([entry("e1")], [
    replica("r1", "e1", { path: "C:/lib/A.litematic", folderPath: "C:/lib" }),
    replica("r2", "e1", { path: "C:/lib/B.litematic", folderPath: "C:/lib" }),
  ]);
  const before = store.auditLibraryRedundancy(subject);
  assert.equal(before.entries[0].totalActiveReplicas, 2);
  const { state: next } = await store.moveReplicasToTrash(subject, ["r2"]);
  const after = store.auditLibraryRedundancy(next);
  assert.equal(after.entries[0].totalActiveReplicas, 1, "移入回收区的副本不再计入重数");
  assert.equal(after.entries[0].duplicateFolders, 0);
});

test("还原：legacy 记录不可还原", async () => {
  const store = loadStore({ backend: { libraryTrashRestore: async () => [{ trashPath: "", targetPath: "", ok: true }] } });
  const subject = state([], [], { trash: [{ id: "t-legacy", kind: "projection", name: "旧记录", status: "legacy", trashPath: "", originalPath: "C:/old.litematic", replicaSnapshot: null, records: [], categories: [], movedAt: 0, deletedAt: 0, originalFileName: "" }] });
  const result = await store.restoreLibraryTrashAsync(subject, "t-legacy");
  assert.equal(result.ok, false);
  assert.equal(result.reason, "legacy");
  assert.equal(result.state.trash.length, 1, "legacy 记录保留（可清理，不可还原）");
});

test("还原：成功恢复副本到原路径并移除回收记录，条目复活", async () => {
  const store = loadStore({ backend: { libraryTrashRestore: async (items) => items.map((item) => ({ trashPath: item.trashPath, targetPath: item.targetPath, ok: true })) } });
  const subject = state([entry("e1", { status: "retired" })], [replica("r1", "e1", { path: "C:/lib/A.litematic", status: "trashed" })], {
    trash: [{ id: "t1", kind: "projection", name: "A", status: "in_trash", trashPath: "C:/trash/A.litematic", originalPath: "C:/lib/A.litematic", originalFileName: "A.litematic", replicaSnapshot: replica("r1", "e1", { path: "C:/lib/A.litematic", status: "active" }), records: [], categories: [], movedAt: 1, deletedAt: 1 }],
  });
  const result = await store.restoreLibraryTrashAsync(subject, "t1");
  assert.equal(result.ok, true);
  assert.equal(result.state.trash.length, 0);
  const restored = result.state.replicas.find((item) => item.id === "r1");
  assert.equal(restored.status, "active");
  assert.equal(restored.path, "C:/lib/A.litematic");
  assert.equal(result.state.entries.find((item) => item.id === "e1").status, "active");
});

test("还原冲突：目标占用时以 renameTo 改名落点，失败则保留回收记录", async () => {
  let received;
  const store = loadStore({ backend: { libraryTrashRestore: async (items) => { received = items; return items.map((item) => ({ trashPath: item.trashPath, targetPath: item.targetPath, ok: true })); } } });
  const subject = state([entry("e1")], [replica("r1", "e1", { path: "C:/other/B.litematic", status: "trashed" })], {
    trash: [{ id: "t1", kind: "projection", name: "A", status: "in_trash", trashPath: "C:/trash/A.litematic", originalPath: "C:/lib/A.litematic", originalFileName: "A.litematic", replicaSnapshot: replica("r1", "e1", { path: "C:/lib/A.litematic" }), records: [], categories: [], movedAt: 1, deletedAt: 1 }],
  });
  const renamed = await store.restoreLibraryTrashAsync(subject, "t1", "A_已存在.litematic");
  assert.equal(renamed.ok, true);
  assert.equal(received[0].renameTo, "A_已存在.litematic");
  assert.match(received[0].targetPath, /A_已存在\.litematic$/);
  assert.equal(renamed.state.replicas.find((item) => item.id === "r1").path, received[0].targetPath);

  const occupied = loadStore({ backend: { libraryTrashRestore: async () => [{ trashPath: "C:/trash/A.litematic", targetPath: "", ok: false, reason: "occupied" }] } });
  const blocked = await occupied.restoreLibraryTrashAsync(subject, "t1");
  assert.equal(blocked.ok, false);
  assert.equal(blocked.reason, "occupied");
  assert.equal(blocked.state.trash.length, 1, "还原失败时回收记录保留");
});

test("清空：legacy 记录直接清除（无物理文件），物理删除成功项移除", async () => {
  const store = loadStore({ backend: { libraryTrashPurge: async (items) => items.map((item) => ({ trashPath: item.trashPath, ok: true })) } });
  const subject = state([], [], {
    trash: [
      { id: "t-physical", kind: "projection", name: "A", status: "in_trash", trashPath: "C:/trash/A.litematic", originalPath: "C:/lib/A.litematic", originalFileName: "A.litematic", replicaSnapshot: null, records: [], categories: [], movedAt: 1, deletedAt: 1 },
      { id: "t-legacy", kind: "projection", name: "旧", status: "legacy", trashPath: "", originalPath: "", originalFileName: "", replicaSnapshot: null, records: [], categories: [], movedAt: 0, deletedAt: 0 },
    ],
  });
  const { state: next, failures } = await store.purgeLibraryTrashAsync(subject);
  assert.deepEqual(failures, []);
  assert.equal(next.trash.length, 0, "legacy 与成功删除项均被清除");
});

test("清空：物理删除失败项保留并回报，不静默丢弃", async () => {
  const store = loadStore({ backend: { libraryTrashPurge: async (items) => items.map((item) => ({ trashPath: item.trashPath, ok: false, reason: "locked" })) } });
  const subject = state([], [], {
    trash: [{ id: "t1", kind: "projection", name: "A", status: "in_trash", trashPath: "C:/trash/A.litematic", originalPath: "C:/lib/A.litematic", originalFileName: "A.litematic", replicaSnapshot: null, records: [], categories: [], movedAt: 1, deletedAt: 1 }],
  });
  const { state: next, failures } = await store.purgeLibraryTrashAsync(subject);
  assert.deepEqual(failures, [{ trashPath: "C:/trash/A.litematic", reason: "locked" }]);
  assert.equal(next.trash.length, 1, "不可删除项必须保留");
});

test("恢复记录不存在时确定性返回 missing-record", async () => {
  const store = loadStore();
  const result = await store.restoreLibraryTrashAsync(state([], []), "unknown");
  assert.equal(result.ok, false);
  assert.equal(result.reason, "missing-record");
});