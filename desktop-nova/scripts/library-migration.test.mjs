import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import { JSDOM } from "jsdom";

// F.4 schemaVersion 4 → 5 migration (design 2.3.3 / tasks.md F.4).
// The migration is asserted as a pure, deterministic, retry-safe computation:
// idempotent output (guarded by needsLibraryMigration), no data loss, stable ids,
// the createdAt fallback chain and legacy-trash demotion.
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
  name === "./filePathIdentity" ? paths : name === "./libraryCategoryIcons" ? icons : (() => { throw new Error(`Unexpected dependency: ${name}`); })());

const legacyRecord = (overrides = {}) => ({
  id: "legacy:A",
  path: "C:\\投影\\A.litematic",
  fileName: "A.litematic",
  displayName: "A.litematic",
  internalName: "内部名",
  author: "作者",
  description: "描述",
  totalBlocks: 10,
  totalVolume: 20,
  regionCount: 2,
  regionNames: ["选区一", "选区二"],
  enclosingSize: { x: 1, y: 2, z: 3 },
  minecraftDataVersion: 3953,
  fileSize: 4096,
  mtime: 200,
  tags: [],
  status: "ok",
  lastAnalyzedAt: 100,
  lastError: "",
  sort_index: 0,
  ...overrides,
});

test("迁移到 schemaVersion 5 两级模型：条目/副本字段映射完整且无字段丢失", () => {
  const previous = { records: [legacyRecord()], folders: [{ path: "C:/投影", enabled: true, recursive: true, sort_index: 0 }] };
  const next = migration.migrateLibraryState(previous);

  assert.equal(next.schemaVersion, 5);
  assert.equal("records" in next, false, "迁移后不再保留单层 records");
  assert.equal(next.entries.length, 1);
  assert.equal(next.replicas.length, 1);

  const [entry] = next.entries;
  const [replica] = next.replicas;
  assert.equal(entry.id, migration.deterministicEntryId(previous.records[0].path));
  assert.equal(replica.entryId, entry.id);
  assert.equal(replica.path, previous.records[0].path, "副本保留原始路径");
  assert.equal(replica.fileName, "A.litematic");
  assert.equal(replica.folderPath, "C:\\投影");
  assert.equal(replica.status, "active");
  assert.equal(entry.status, "active");

  // A.5: migrated replicas carry no checksum yet - first scan backfills.
  assert.equal(entry.checksum, null);
  assert.equal(replica.checksum, null);

  // Library metadata lands on the entry, never duplicated onto the replica.
  assert.equal(entry.internalName, "内部名");
  assert.equal(entry.author, "作者");
  assert.equal(entry.description, "描述");
  assert.equal(entry.stats.totalBlocks, 10);
  assert.equal(entry.stats.totalVolume, 20);
  assert.equal(entry.stats.regionCount, 2);
  assert.deepEqual(entry.stats.regionNames, ["选区一", "选区二"]);
  assert.deepEqual(entry.stats.enclosingSize, { x: 1, y: 2, z: 3 });
  assert.equal(entry.stats.minecraftDataVersion, 3953);
  assert.equal("author" in replica, false, "副本不复制条目级元数据");
});

test("迁移幂等：v5 结果被 needsLibraryMigration 判定为无需再迁移", () => {
  const previous = { records: [legacyRecord()], folders: [] };
  assert.equal(migration.needsLibraryMigration(previous), true, "单层 records 必须触发迁移");
  const migrated = migration.migrateLibraryState(previous);
  assert.equal(migration.needsLibraryMigration(migrated), false, "v5 输出必须短路迁移，保证 loadLibrary 幂等");
  assert.equal(migration.needsLibraryMigration({ entries: [], replicas: [] }), false);
  assert.equal(migration.needsLibraryMigration({ schemaVersion: 5 }), false);
  assert.equal(migration.needsLibraryMigration({ schemaVersion: 4 }), true);
});

test("确定性：同输入重复迁移结果一致，ID 由路径身份派生且对别名稳定", () => {
  const make = () => ({ records: [legacyRecord(), legacyRecord({ id: "legacy:B", path: "C:/投影/B.litematic", sort_index: 1 })], folders: [] });
  const first = migration.migrateLibraryState(make());
  const second = migration.migrateLibraryState(make());
  assert.deepEqual(first, second, "迁移必须是可重试的纯函数");
  assert.equal(first.entries[0].id, migration.deterministicEntryId("C:\\投影\\A.litematic"));
  assert.notEqual(first.entries[0].id, first.entries[1].id);
  // Case / separator aliases of the same file must derive the same identity.
  assert.equal(migration.deterministicEntryId("C:/投影/A.litematic"), migration.deterministicEntryId("c:\\投影\\a.litematic"));
  assert.equal(migration.deterministicReplicaId("C:/投影/A.litematic"), migration.deterministicReplicaId("c:\\投影\\a.litematic"));
  assert.notEqual(migration.deterministicEntryId("C:/投影/A.litematic"), migration.deterministicReplicaId("C:/投影/A.litematic"));
});

test("createdAt 取值链：lastAnalyzedAt → mtime → 0（确定性兜底）", () => {
  const withAnalysis = migration.migrateLibraryState({ records: [legacyRecord({ lastAnalyzedAt: 777, mtime: 200 })], folders: [] });
  assert.equal(withAnalysis.entries[0].createdAt, 777);
  const withMtime = migration.migrateLibraryState({ records: [legacyRecord({ lastAnalyzedAt: 0, mtime: 200 })], folders: [] });
  assert.equal(withMtime.entries[0].createdAt, 200);
  const bare = migration.migrateLibraryState({ records: [legacyRecord({ lastAnalyzedAt: 0, mtime: 0 })], folders: [] });
  assert.equal(bare.entries[0].createdAt, 0, "两者皆缺时使用确定性兜底而非当前时间，保证重试一致");
});

test("路径别名先归并：同文件的大小写/分隔符别名折叠为单一条目", () => {
  const previous = {
    records: [
      legacyRecord({ id: "x", path: "C:\\投影\\A.litematic", lastAnalyzedAt: 100, categoryIds: ["c1"], status: "parse_error", preview_image_path: "p1.png" }),
      legacyRecord({ id: "y", path: "c:/投影/a.LITEMATIC", lastAnalyzedAt: 300, categoryIds: ["c2"], status: "ok", preview_image_path: "p2.png", previewUpdatedAt: 5 }),
    ],
    folders: [],
    categories: [
      { id: "c1", name: "甲", parentId: null, sort_index: 0, icon: { kind: "lucide", name: "Folder" } },
      { id: "c2", name: "乙", parentId: null, sort_index: 1, icon: { kind: "lucide", name: "Folder" } },
    ],
  };
  const next = migration.migrateLibraryState(previous);
  assert.equal(next.entries.length, 1, "同一文件的别名必须合并为一个条目");
  assert.equal(next.replicas.length, 1);
  assert.deepEqual([...next.entries[0].categoryIds].sort(), ["c1", "c2"], "分类取并集");
  assert.equal(next.entries[0].status, "active", "以最新成功分析的状态为准");
  assert.equal(next.entries[0].preview.preview_image_path, "p2.png", "保留最新预览");
});

test("副本状态映射 ok→active / missing→missing / parse_error→parse_error", () => {
  const records = [
    legacyRecord({ id: "ok", path: "C:/投影/ok.litematic", displayName: "ok.litematic", status: "ok" }),
    legacyRecord({ id: "missing", path: "C:/投影/missing.litematic", displayName: "missing.litematic", status: "missing" }),
    legacyRecord({ id: "parse_error", path: "C:/投影/err.litematic", displayName: "err.litematic", status: "parse_error" }),
  ];
  const next = migration.migrateLibraryState({ records, folders: [] });
  const byPath = new Map(next.replicas.map((replica) => [replica.path, replica]));
  assert.equal(byPath.get("C:/投影/ok.litematic").status, "active");
  assert.equal(byPath.get("C:/投影/missing.litematic").status, "missing");
  assert.equal(byPath.get("C:/投影/err.litematic").status, "parse_error");
  const entryByPath = new Map(next.entries.map((entry) => [entry.displayName, entry]));
  assert.equal(entryByPath.get("ok.litematic").status, "active");
  assert.equal(entryByPath.get("missing.litematic").status, "retired");
  assert.equal(entryByPath.get("err.litematic").status, "retired");
});

test("旧 tags 迁移为多分类成员，去重且补齐缺失分类引用", () => {
  const previous = { records: [legacyRecord({ tags: ["红石", "建筑", "红石"] })], folders: [], categories: [] };
  const next = migration.migrateLibraryState(previous);
  const names = next.entries[0].categoryIds.map((id) => next.categories.find((category) => category.id === id)?.name);
  assert.deepEqual(names.sort(), ["建筑", "红石"]);
  assert.equal(next.categories.filter((category) => category.name === "红石").length, 1, "同名标签只建一个分类");
  assert.equal("tags" in next.entries[0], true);
  assert.deepEqual(next.entries[0].tags, [], "库内权威 tags 镜像迁移时为空，由文件 NovaTags 回填");
});

test("引用了不存在的分类时补建恢复分类，引用不悬空", () => {
  const previous = { records: [legacyRecord({ categoryIds: ["ghost"] })], folders: [], categories: [] };
  const next = migration.migrateLibraryState(previous);
  assert.equal(next.entries[0].categoryIds.length, 1);
  const restoredId = next.entries[0].categoryIds[0];
  assert.notEqual(restoredId, "ghost");
  const restored = next.categories.find((category) => category.id === restoredId);
  assert.ok(restored, "悬空引用必须被改写为存在的分类");
  assert.equal(restored.name, "恢复的分类");
});

test("文件夹迁移：旧文件夹默认 tracked，并自动挂载 builtin_import", () => {
  const next = migration.migrateLibraryState({ records: [legacyRecord()], folders: [{ path: "C:/投影", enabled: true, recursive: true, sort_index: 0 }] });
  assert.equal(next.folders[0].type, "tracked");
  assert.ok(next.folders.some((folder) => folder.type === "builtin_import" && folder.path === "projection-library/import"), "必须挂载内置导入文件夹");
  // Duplicate folder aliases collapse by path identity.
  const deduped = migration.migrateLibraryState({ records: [], folders: [{ path: "C:/投影", sort_index: 0 }, { path: "c:\\投影\\", enabled: false, sort_index: 1 }] });
  assert.equal(deduped.folders.filter((folder) => folder.type === "tracked").length, 1);
});

test("旧回收记录迁移为 legacy：不可还原、保留历史视图、路径信息保留", () => {
  const previous = {
    records: [],
    folders: [],
    trash: [{ id: "trash-1", kind: "projection", name: "旧记录", deletedAt: 123, records: [legacyRecord()], categories: [] }],
  };
  const next = migration.migrateLibraryState(previous);
  assert.equal(next.trash.length, 1);
  const [trash] = next.trash;
  assert.equal(trash.status, "legacy", "旧回收记录无物理文件，标记 legacy 不可还原");
  assert.equal(trash.replicaSnapshot, null);
  assert.equal(trash.trashPath, "");
  assert.equal(trash.originalPath, "");
  assert.equal(trash.movedAt, 123);
  assert.equal(trash.records[0].path, "C:\\投影\\A.litematic");
  assert.equal(trash.records[0].checksum, null);
});

test("recent 由 sort_index 前 20 条推导为 entry 引用", () => {
  const records = Array.from({ length: 25 }, (_, index) =>
    legacyRecord({ id: `l${index}`, path: `C:/投影/${index}.litematic`, sort_index: index }));
  const next = migration.migrateLibraryState({ records, folders: [] });
  assert.equal(next.recent.length, 20);
  assert.ok(next.recent.every((ref) => ref.kind === "entry" && next.entries.some((entry) => entry.id === ref.entryId)));
  assert.equal(next.recent[0].entryId, next.entries.find((entry) => entry.sortIndex === 0).id);
});

test("迁移是纯函数：不修改输入对象（失败可安全重试、不改磁盘）", () => {
  const previous = { records: [legacyRecord({ tags: ["甲"], categoryIds: ["c1"] })], folders: [{ path: "C:/投影", sort_index: 0 }], trash: [] };
  const snapshot = structuredClone(previous);
  migration.migrateLibraryState(previous);
  assert.deepEqual(previous, snapshot, "迁移不得就地改写入参");
  assert.equal("entries" in previous, false);
});

test("空输入确定性收敛，folders 中损坏项被丢弃", () => {
  const empty = migration.migrateLibraryState({});
  assert.equal(empty.schemaVersion, 5);
  assert.deepEqual(empty.entries, []);
  assert.deepEqual(empty.replicas, []);
  assert.equal(empty.folders.some((folder) => folder.type === "builtin_import"), false, "无内容时不挂载内置导入");
  assert.equal(migration.needsLibraryMigration(null), true);
  assert.equal(migration.needsLibraryMigration("nope"), true);
  // A null/empty folder entry is dropped rather than producing a path-less folder.
  const folders = migration.migrateLibraryState({ records: [], folders: [null, { path: "   " }, { path: "C:/投影", sort_index: 0 }] });
  assert.equal(folders.folders.filter((folder) => folder.type === "tracked").length, 1);
  assert.equal(folders.folders.every((folder) => folder.path.trim().length > 0), true);
});