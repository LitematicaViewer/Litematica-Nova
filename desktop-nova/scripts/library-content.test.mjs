import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

// F.5 identity & content-change kernel (design 2.1.4(2) / tasks.md F.5).
// Everything in `libraryContent.ts` is a pure transformation, so the assertions
// pin the content sync/diverge semantics, automatic merge, retirement/revival,
// the redirect chain and dirty-replica extraction without needing the IPC host.
function compile(path, dependencies) {
  const compiled = ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } });
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${compiled.outputText}})`)(dependencies, module, module.exports);
  return module.exports;
}
const paths = compile("../src/services/filePathIdentity.ts", () => { throw new Error("Unexpected path import"); });
const content = compile("../src/services/libraryContent.ts", (name) =>
  name === "./filePathIdentity" ? paths : name === "./libraryStore" ? {} : (() => { throw new Error(`Unexpected dependency: ${name}`); })());

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

test("buildChecksumIndex：覆盖 retired 供复活，排除 merged，冲突时 active/更早/更小 id 优先", () => {
  const index = content.buildChecksumIndex(state([
    entry("retired", { checksum: "A", status: "retired" }),
    entry("active", { checksum: "A", status: "active" }),
    entry("merged", { checksum: "B", status: "merged" }),
    entry("early", { checksum: "C", status: "active", createdAt: 1 }),
    entry("late", { checksum: "C", status: "active", createdAt: 5 }),
    entry("id-a", { checksum: "D", status: "active", createdAt: 7 }),
    entry("id-b", { checksum: "D", status: "active", createdAt: 7 }),
    entry("none", { checksum: null }),
  ], []));
  assert.equal(index.get("A"), "active", "有 active 时不得指向 retired");
  assert.equal(index.has("B"), false, "merged 条目不参与 checksum 索引");
  assert.equal(index.get("C"), "early", "同 active 时 createdAt 更早者优先");
  assert.equal(index.get("D"), "id-a", "createdAt 完全相等时取 id 字典序较小者");
  assert.equal(index.has("undefined"), false);
  assert.equal(index.has("null"), false);
});

test("resolveEntryId：沿更替链解析并具备环路防护", () => {
  const chained = state([], [], { redirects: [{ fromId: "a", toId: "b", at: 1 }, { fromId: "b", toId: "c", at: 2 }] });
  assert.equal(content.resolveEntryId(chained, "a"), "c");
  assert.equal(content.resolveEntryId(chained, "c"), "c", "链尾返回自身");
  assert.equal(content.resolveEntryId(chained, "unknown"), "unknown");
  const cyclic = state([], [], { redirects: [{ fromId: "x", toId: "y", at: 1 }, { fromId: "y", toId: "x", at: 2 }] });
  assert.equal(typeof content.resolveEntryId(cyclic, "x"), "string", "环路必须终止");
});

test("entryActiveReplicas：仅统计该条目的 active 副本", () => {
  const subject = state([entry("e1"), entry("e2")], [
    replica("r1", "e1"), replica("r2", "e1", { status: "missing" }), replica("r3", "e2"),
  ]);
  assert.deepEqual(content.entryActiveReplicas(subject, "e1").map((item) => item.id), ["r1"]);
});

test("applyContentSync：整体重算 checksum 且条目 ID 不变，retired 复活", () => {
  const subject = state([
    entry("e1", { checksum: "old", status: "retired" }),
    entry("e2", { checksum: "keep" }),
  ], [
    replica("r1", "e1", { checksum: "old" }),
    replica("r2", "e1", { checksum: "old" }),
    replica("r3", "e1", { checksum: "old", status: "missing" }),
  ]);
  const next = content.applyContentSync(subject, "e1", "new");
  assert.equal(next.entries.find((item) => item.id === "e1").checksum, "new");
  assert.equal(next.entries.find((item) => item.id === "e1").status, "active", "同步后 retired 条目复活");
  assert.deepEqual(next.replicas.filter((item) => item.entryId === "e1" && item.status === "active").map((item) => item.checksum), ["new", "new"]);
  assert.equal(next.replicas.find((item) => item.id === "r3").checksum, "old", "非 active 副本不被重算");
  assert.equal(next.entries.some((item) => item.id === "e1"), true, "同步不改变条目 ID");
  assert.equal(subject.entries.find((item) => item.id === "e1").checksum, "old", "纯函数：入参不被改写");
});

test("applyContentDiverge：变动副本迁出到新条目，源条目无活跃副本则退役", () => {
  const subject = state([entry("e1", { checksum: "X" })], [replica("r1", "e1", { checksum: "Y" })]);
  const result = content.applyContentDiverge(subject, "r1", 42);
  assert.ok(result);
  const moved = result.state.replicas.find((item) => item.id === "r1");
  assert.equal(moved.entryId, result.toEntryId);
  assert.notEqual(result.toEntryId, "e1");
  const created = result.state.entries.find((item) => item.id === result.toEntryId);
  assert.equal(created.status, "active");
  assert.equal(created.checksum, "Y");
  assert.equal(result.state.entries.find((item) => item.id === "e1").status, "retired");
  assert.equal(result.sourceEntryRetired, true);
});

test("applyContentDiverge：源条目仍有其它活跃副本时不退役", () => {
  const subject = state([entry("e1", { checksum: "X" })], [
    replica("r1", "e1", { checksum: "Y" }),
    replica("r2", "e1", { checksum: "X" }),
  ]);
  const result = content.applyContentDiverge(subject, "r1", 42);
  assert.equal(result.state.entries.find((item) => item.id === "e1").status, "active");
  assert.equal(result.sourceEntryRetired, false);
  assert.equal(result.state.replicas.find((item) => item.id === "r2").entryId, "e1");
});

test("applyContentDiverge：checksum 未变或副本不存在时返回 null（不改状态）", () => {
  const unchanged = state([entry("e1", { checksum: "X" })], [replica("r1", "e1", { checksum: "X" })]);
  assert.equal(content.applyContentDiverge(unchanged, "r1", 1), null);
  assert.equal(content.applyContentDiverge(unchanged, "missing", 1), null);
});

test("mergeEntries：保留更早条目、分类并集、副本改挂、追加更替链", () => {
  const subject = state([
    entry("keep", { categoryIds: ["a"], createdAt: 1 }),
    entry("discard", { categoryIds: ["b", "a"], createdAt: 3 }),
  ], [replica("r1", "discard"), replica("r2", "keep")]);
  const next = content.mergeEntries(subject, "keep", "discard", 99);
  assert.equal(next.entries.find((item) => item.id === "discard").status, "merged");
  assert.deepEqual([...next.entries.find((item) => item.id === "keep").categoryIds].sort(), ["a", "b"]);
  assert.equal(next.replicas.find((item) => item.id === "r1").entryId, "keep", "被合并条目的副本改挂保留者");
  assert.deepEqual(next.redirects.at(-1), { fromId: "discard", toId: "keep", at: 99 });
  assert.equal(content.mergeEntries(subject, "keep", "keep", 1), subject, "自合并为 no-op");
});

test("autoMergeByChecksum：同 checksum 合并，keeper 取 createdAt 更早、相等取 id 更小", () => {
  const subject = state([
    entry("b-late", { checksum: "H", createdAt: 5 }),
    entry("a-early", { checksum: "H", createdAt: 1, categoryIds: ["x"] }),
    entry("tie-z", { checksum: "K", createdAt: 2 }),
    entry("tie-a", { checksum: "K", createdAt: 2 }),
    entry("solo", { checksum: "S", createdAt: 0 }),
  ], []);
  const { state: next, merges } = content.autoMergeByChecksum(subject, 7);
  assert.equal(next.entries.find((item) => item.id === "b-late").status, "merged");
  assert.equal(next.entries.find((item) => item.id === "a-early").status, "active");
  assert.equal(next.entries.find((item) => item.id === "tie-a").status, "active", "createdAt 相等取 id 较小者");
  assert.equal(next.entries.find((item) => item.id === "tie-z").status, "merged");
  assert.equal(next.entries.find((item) => item.id === "solo").status, "active", "无冲突条目不合并");
  assert.equal(merges.length, 2);
  assert.ok(merges.every((item) => next.redirects.some((redirect) => redirect.fromId === item.fromId && redirect.toId === item.toId)));
});

test("reconcileDirtyReplicas：条目内 checksum 不一致的活跃副本迁出并回报", () => {
  const subject = state([entry("e1", { checksum: "X" })], [
    replica("good", "e1", { checksum: "X" }),
    replica("dirty", "e1", { checksum: "Y" }),
  ]);
  const { state: next, moved } = content.reconcileDirtyReplicas(subject);
  assert.equal(moved.length, 1);
  assert.equal(moved[0].entryId, "e1");
  assert.equal(moved[0].replicaId, "dirty");
  assert.equal(moved[0].fromChecksum, "X");
  assert.equal(moved[0].toChecksum, "Y");
  const dirty = next.replicas.find((item) => item.id === "dirty");
  assert.equal(dirty.entryId, moved[0].toEntryId);
  assert.notEqual(dirty.entryId, "e1", "脏副本必须迁出，不能静默置 missing");
  assert.equal(next.replicas.find((item) => item.id === "good").entryId, "e1");
  assert.equal(next.entries.find((item) => item.id === "e1").status, "active", "仍有活跃副本，源条目不退役");
});

test("reconcileDirtyReplicas：幂等——收敛后再次运行不再迁出", () => {
  const subject = state([entry("e1", { checksum: "X" })], [replica("dirty", "e1", { checksum: "Y" })]);
  const once = content.reconcileDirtyReplicas(subject);
  const twice = content.reconcileDirtyReplicas(once.state);
  assert.equal(once.moved.length, 1);
  assert.equal(twice.moved.length, 0);
  // Semantic idempotence: cloneEntries backfills a `regionNames: undefined` key,
  // so compare identity/state tuples rather than raw structure.
  assert.deepEqual(
    twice.state.replicas.map((item) => [item.id, item.entryId, item.checksum, item.status]),
    once.state.replicas.map((item) => [item.id, item.entryId, item.checksum, item.status]),
  );
  assert.deepEqual(
    twice.state.entries.map((item) => [item.id, item.checksum, item.status]).sort(),
    once.state.entries.map((item) => [item.id, item.checksum, item.status]).sort(),
  );
});

test("auditRedundancy：按文件夹分组、仅计活跃副本、排除 merged 条目", () => {
  const subject = state([
    entry("e1", { displayName: "投影一" }),
    entry("merged", { status: "merged" }),
  ], [
    replica("r1", "e1", { folderPath: "C:/lib" }),
    replica("r2", "e1", { folderPath: "C:/lib" }),
    replica("r3", "e1", { folderPath: "C:/other" }),
    replica("r4", "e1", { folderPath: "C:/lib", status: "missing" }),
  ]);
  const report = content.auditRedundancy(subject);
  assert.equal(report.entries.length, 1, "merged 条目不出现在审计中");
  const [only] = report.entries;
  assert.equal(only.totalActiveReplicas, 3);
  assert.equal(only.duplicateFolders, 1, "仅同文件夹出现多副本才算重复");
  assert.equal(only.folders.find((folder) => folder.count === 2).folderPath, "C:/lib");
  assert.equal(report.totalActiveReplicas, 3);
});