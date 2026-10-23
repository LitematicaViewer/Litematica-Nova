import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import { JSDOM } from "jsdom";

// F.9 packaged export (tasks.md F.9 / E.5 / 5.9.1.4–5.9.1.7 / 裁决 3/4/16).
// TS owns the flat archive naming, dedup and the material-list text; Rust only writes
// bytes. This suite pins: flat structure, always-included body, manifest params and the
// default `txt` material list, plus completion/failure surfacing.
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

function loadStore({ backend = {}, tasks = {}, stats = {} } = {}) {
  let exportParams = null;
  const appOperations = { inspectProject: async () => ({ metadata: {}, regions: [] }), editProjectMetadata: async () => ({}) };
  const indexClient = { queryIndex: async () => ({ entries: [], ready: true }), submitIndexBuild: async () => "task", subscribeIndexStatus: async () => () => {} };
  const taskClient = {
    submitLibraryTask: async () => "task", cancelLibraryTask: async () => true, subscribeLibraryTask: async () => () => {},
    submitAndAwait: async () => ({ taskId: "task", kind: "export-zip", status: "completed", summary: {}, failures: [] }),
    awaitLibraryTask: async () => ({ taskId: "task", kind: "export-zip", status: "completed", summary: {}, failures: [] }),
    ...tasks,
  };
  const statsService = { formatMaterialQuantity: (material, count) => String(count), loadStructureStats: async () => ({ materials: [] }), ...stats };
  const backendPath = {
    readUserConfigFile: async () => { throw new Error("no disk"); }, writeUserConfigFile: async () => {},
    checkFileExists: async () => true, listLitematicFileEntriesInDirectory: async () => [],
    copyFileToDirectory: async () => ({}), computeLitematicChecksum: async () => ({ sha256: "x", byte_count: 1 }),
    getUserConfigFilePath: async () => "C:/builtin/import", libraryTrashMove: async () => [],
    libraryTrashRestore: async () => [], libraryTrashPurge: async () => [],
    libraryExportZip: async (params) => { exportParams = params; return { taskId: "zip-1" }; },
    ...backend,
  };
  const store = compile("../src/services/libraryStore.ts", (name) => {
    if (name === "./libraryCategoryIcons") return icons;
    if (name === "./filePathIdentity") return paths;
    if (name === "./backend") return backendPath;
    if (name === "../platform/applicationOperations") return appOperations;
    if (name === "./libraryMigration") return migration;
    if (name === "./libraryContent") return contentModule;
    if (name === "./libraryTasks") return taskClient;
    if (name === "./statsService") return statsService;
    if (name === "./minecraftVersions") return { minecraftVersionLabel: (value) => `v${value}` };
    if (name === "./libraryIndexClient") return indexClient;
    throw new Error(`Unexpected store dependency: ${name}`);
  });
  return { store, getExportParams: () => exportParams };
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
const options = (over = {}) => ({ includePreview: false, includeMaterialList: false, includeManifest: true, outputPath: "C:/out.zip", ...over });

test("缺少导出目标路径时拒绝导出", async () => {
  const { store } = loadStore();
  await assert.rejects(store.exportLibraryZip(state([entry("e1")], [replica("r1", "e1")]), ["e1"], options({ outputPath: "   " })), /请选择导出目标路径/);
});

test("没有匹配的 active 条目时拒绝导出", async () => {
  const { store } = loadStore();
  await assert.rejects(store.exportLibraryZip(state([entry("e1", { status: "retired" })], [replica("r1", "e1")]), ["e1"], options()), /没有可导出的条目/);
});

test("扁平导出：archiveName 去扩展、本体恒含 sourcePath、条目元数据随本体", async () => {
  const { store, getExportParams } = loadStore();
  const subject = state(
    [entry("e1", { displayName: "红石机.litematic", author: "作者", tags: ["红石"], checksum: "deadbeef" })],
    [replica("r1", "e1", { path: "C:/lib/深层/红石机.litematic", fileName: "红石机.litematic" })],
  );
  const result = await store.exportLibraryZip(subject, ["e1"], options());
  assert.equal(result.status, "completed");
  const params = getExportParams();
  assert.equal(params.outputPath, "C:/out.zip");
  assert.equal(params.includeManifest, true);
  assert.match(params.exportedAt, /^\d{4}-\d{2}-\d{2}T/);
  assert.equal(params.entries.length, 1);
  const [only] = params.entries;
  assert.equal(only.archiveName, "红石机", "归档名去掉 .litematic 扩展（扁平，不保留文件夹层级）");
  assert.equal(only.sourcePath, "C:/lib/深层/红石机.litematic", "投影本体恒定随导出（include.projection 恒 true）");
  assert.equal(only.displayName, "红石机.litematic");
  assert.equal(only.author, "作者");
  assert.deepEqual(only.tags, ["红石"]);
  assert.equal(only.checksum, "deadbeef");
});

test("扁平去重：同名条目追加序号后缀", async () => {
  const { store, getExportParams } = loadStore();
  const subject = state(
    [entry("e1", { displayName: "同名" }), entry("e2", { displayName: "同名" })],
    [replica("r1", "e1"), replica("r2", "e2")],
  );
  await store.exportLibraryZip(subject, ["e1", "e2"], options());
  assert.deepEqual(getExportParams().entries.map((item) => item.archiveName), ["同名", "同名_2"]);
});

test("可选内容：includePreview 命中存在的预览，includeMaterialList 生成 txt 材料清单", async () => {
  const materials = [
    { materialStatus: "item", name: "红石", id: "minecraft:redstone", totalCount: 128 },
    { materialStatus: "block", name: "石头", id: "minecraft:stone", totalCount: 5 },
    { materialStatus: "item", name: "钻石", id: "minecraft:diamond", totalCount: 64 },
  ];
  const { store, getExportParams } = loadStore({
    backend: { checkFileExists: async (path) => path === "C:/preview/e1.png" },
    stats: { loadStructureStats: async () => ({ materials }) },
  });
  const subject = state([entry("e1", { preview: { preview_image_path: "C:/preview/e1.png" } })], [replica("r1", "e1")]);
  await store.exportLibraryZip(subject, ["e1"], options({ includePreview: true, includeMaterialList: true }));
  const [only] = getExportParams().entries;
  assert.equal(only.previewPath, "C:/preview/e1.png");
  assert.equal(only.materialListText, "红石\t128\r\n钻石\t64", "默认 txt：仅 item，行以 \\r\\n 连接");
});

test("未勾选预览/材料清单时不产生对应内容；预览不存在时回退为空", async () => {
  const { store, getExportParams } = loadStore({ backend: { checkFileExists: async () => false }, stats: { loadStructureStats: async () => ({ materials: [{ materialStatus: "item", name: "红石", totalCount: 1 }] }) } });
  const subject = state([entry("e1", { preview: { preview_image_path: "C:/missing.png" } })], [replica("r1", "e1")]);
  await store.exportLibraryZip(subject, ["e1"], options());
  const [only] = getExportParams().entries;
  assert.equal(only.previewPath, undefined);
  assert.equal(only.materialListText, undefined);
  const previewOnly = loadStore({ backend: { checkFileExists: async () => false } });
  await previewOnly.store.exportLibraryZip(subject, ["e1"], options({ includePreview: true }));
  assert.equal(previewOnly.getExportParams().entries[0].previewPath, undefined, "预览文件不存在时不导出预览");
});

test("导出结果透传任务摘要；失败时给出 errors", async () => {
  const { store } = loadStore({ tasks: { awaitLibraryTask: async () => ({ taskId: "zip-1", kind: "export-zip", status: "completed", summary: { outputPath: "C:/out.zip", entryCount: 2, files: ["a.litematic", "b.litematic"] }, failures: [] }) } });
  const subject = state([entry("e1"), entry("e2")], [replica("r1", "e1"), replica("r2", "e2")]);
  const result = await store.exportLibraryZip(subject, ["e1", "e2"], options());
  assert.equal(result.status, "completed");
  assert.equal(result.outputPath, "C:/out.zip");
  assert.equal(result.entryCount, 2);
  assert.deepEqual(result.files, ["a.litematic", "b.litematic"]);
  assert.deepEqual(result.errors, []);

  const failed = loadStore({ tasks: { awaitLibraryTask: async () => ({ taskId: "zip-1", kind: "export-zip", status: "failed", summary: { message: "目标不可写" }, failures: [{ path: "C:/out.zip", reason: "目标不可写" }] }) } });
  const failure = await failed.store.exportLibraryZip(subject, ["e1"], options());
  assert.equal(failure.status, "failed");
  assert.deepEqual(failure.errors, ["目标不可写"]);
  assert.deepEqual(failure.failures, [{ path: "C:/out.zip", reason: "目标不可写" }]);
});

test("manifest schema 参数随导出传递（裁决 3）", async () => {
  const { store, getExportParams } = loadStore();
  const subject = state([entry("e1", { displayName: "红石机", author: "作者", tags: ["红石"], checksum: "abc" })], [replica("r1", "e1", { fileName: "红石机.litematic" })]);
  await store.exportLibraryZip(subject, ["e1"], options({ includeManifest: true }));
  const params = getExportParams();
  const projected = {
    outputPath: params.outputPath, exportedAt: params.exportedAt, includeManifest: params.includeManifest,
    entries: params.entries.map((item) => ({ displayName: item.displayName, author: item.author, tags: item.tags, checksum: item.checksum, fileName: item.fileName })),
  };
  assert.deepEqual(projected, {
    outputPath: "C:/out.zip",
    exportedAt: params.exportedAt,
    includeManifest: true,
    entries: [{ displayName: "红石机", author: "作者", tags: ["红石"], checksum: "abc", fileName: "红石机.litematic" }],
  });
});