import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

// Content transformation cases now run in Rust library_content::tests.
// This suite verifies read-only projections and actual IPC forwarding.
function compile(path, dependencies) {
  const compiled = ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } });
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${compiled.outputText}})`)(dependencies, module, module.exports);
  return module.exports;
}
const paths = compile("../src/services/filePathIdentity.ts", () => { throw new Error("Unexpected path import"); });
const commands = []; let acknowledged;
const files = { libraryContentCommand: async request => { commands.push(request); return acknowledged; } };
const content = compile("../src/services/libraryContent.ts", (name) =>
  name === "../platform/files" ? files : name === "./filePathIdentity" ? paths : name === "./libraryStore" ? {} : (() => { throw new Error(`Unexpected dependency: ${name}`); })());

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

test("content mutations forward unchanged state to Rust and only return its acknowledged result", async () => {
  const input=state([entry("a",{checksum:"A"})],[replica("r","a",{checksum:"A"})]);
  const snapshot=JSON.stringify(input); acknowledged={state:{entries:[],replicas:[]},moved:[]};
  assert.equal(await content.reconcileDirtyReplicas(input),acknowledged);assert.equal(commands.at(-1).action,"reconcile");
  acknowledged={A:"a"}; assert.equal((await content.buildChecksumIndex(input)).get("A"),"a");
  for (const [method,args,action] of [["normalizeLibraryContent",[],"normalize"],["mergeEntries",["a","b",7],"merge"],["autoMergeByChecksum",[7],"mergeAll"],["applyContentDiverge",["r",7],"diverge"],["applyContentSync",["a","B",["r"]],"sync"]]) {
    acknowledged={result:action};assert.equal(await content[method](input,...args),acknowledged);assert.equal(commands.at(-1).action,action);assert.equal(commands.at(-1).state,input);
  }
  assert.deepEqual(commands.at(-1).replicaIds,["r"]);assert.equal(JSON.stringify(input),snapshot);
});
