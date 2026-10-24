import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import { JSDOM } from "jsdom";

// F.2 rewrite (tasks.md F.2 / design 2.7.2): the suite now exercises the two-level
// model (`schemaVersion 5` + `entries`/`replicas`) and must load `libraryStore.ts`
// with its real runtime collaborators. The previous vm stub covered only
// `./libraryCategoryIcons` / `./backend`, so it threw for `./filePathIdentity`,
// `./libraryMigration`, `./libraryContent`, `./libraryTasks`, etc. Those stubs are
// provided below with their real modules where the behaviour is under test.
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
const content = compile("../src/services/libraryContent.ts", (name) =>
  name === "./filePathIdentity" ? paths : name === "./libraryStore" ? {} : (() => { throw new Error(`Unexpected content dependency: ${name}`); })());
const tasks = compile("../src/services/libraryTasks.ts", (name) => {
  if (name === "../platform/events") return { listenEvent: async () => () => {} };
  if (name === "../platform/files") return { libraryTaskSubmit: async () => ({ taskId: "test" }), libraryTaskCancel: async () => ({ cancelled: true }), libraryTaskStatus: async () => ({ pending: [], running: [] }) };
  throw new Error(`Unexpected tasks dependency: ${name}`);
});
const statsService = { formatMaterialQuantity: () => "", loadStructureStats: async () => ({}) };
const minecraftVersions = { minecraftVersionLabel: (value) => String(value) };
const indexClient = { queryIndex: async () => ({}), submitIndexBuild: async () => "task", subscribeIndexStatus: async () => () => {} };

/** Loads `libraryStore.ts` with injectable backend / applicationOperations collaborators. */
export function categoryStore(backend = {}, operations = {}) {
  const appOperations = {
    inspectProject: async () => ({ metadata: { name: "新名称", total_blocks: 44 }, regions: [{ name: "第一选区" }, { name: "第二选区" }] }),
    editProjectMetadata: async () => ({}),
    ...operations,
  };
  return compile("../src/services/libraryStore.ts", (name) => {
    if (name === "./libraryCategoryIcons") return icons;
    if (name === "./filePathIdentity") return paths;
    if (name === "./backend") return backend;
    if (name === "../platform/applicationOperations") return appOperations;
    if (name === "./libraryMigration") return migration;
    if (name === "./libraryContent") return content;
    if (name === "./libraryTasks") return tasks;
    if (name === "./statsService") return statsService;
    if (name === "./minecraftVersions") return minecraftVersions;
    if (name === "./libraryIndexClient") return indexClient;
    throw new Error(`Unexpected store dependency: ${name}`);
  });
}

const icon = (name = "Folder") => ({ kind: "lucide", name });
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
const legacyRecord = (over = {}) => ({
  id: "legacy:A", path: "C:\\投影\\A.litematic", fileName: "A.litematic", displayName: "A.litematic",
  author: "作者", description: "保留描述", totalBlocks: 12, totalVolume: 18, regionCount: 1,
  enclosingSize: { x: 2, y: 3, z: 3 }, minecraftDataVersion: 5, fileSize: 456, mtime: 789, tags: [],
  status: "ok", lastAnalyzedAt: 1, lastError: "", preview_image_path: "真实预览.png", sort_index: 4, ...over,
});

test("归一：schemaVersion 更新为 5，结构为 entries/replicas，且幂等", () => {
  const store = categoryStore();
  const raw = state([entry("e1", { displayName: "A.litematic" })], [replica("r1", "e1")], {
    folders: [{ path: "C:/lib", type: "tracked", enabled: true, recursive: true, sort_index: 0 }],
  });
  const next = store.normalizeLibraryState(raw);
  assert.equal(next.schemaVersion, 5);
  assert.equal(next.entries.length, 1);
  assert.equal(next.replicas.length, 1);
  assert.equal("records" in next, false);
  assert.equal(next.replicas[0].entryId, "e1");
  assert.deepEqual(store.normalizeLibraryState(next), next, "归一化必须幂等");
});

test("旧单层数据在归一时自动迁移到两级模型", () => {
  const store = categoryStore();
  const migrated = store.normalizeLibraryState({ records: [legacyRecord()], folders: [{ path: "C:/投影", sort_index: 0 }] });
  assert.equal(migrated.schemaVersion, 5);
  assert.equal("records" in migrated, false);
  assert.equal(migrated.entries.length, 1);
  assert.equal(migrated.replicas.length, 1);
  assert.equal(migrated.replicas[0].path, "C:\\投影\\A.litematic");
  assert.equal(migrated.entries[0].description, "保留描述");
  assert.deepEqual(migrated.entries[0].stats.enclosingSize, { x: 2, y: 3, z: 3 });
});

test("displayName 缺省时由首个副本文件名回填，完整文件名保留", () => {
  const store = categoryStore();
  const next = store.normalizeLibraryState(state([entry("e1", { displayName: undefined })], [replica("r1", "e1", { fileName: "完整名字.litematic" })]));
  assert.equal(next.entries[0].displayName, "完整名字.litematic");
  // A replica missing its folderPath derives it from the path.
  assert.equal(next.replicas[0].folderPath, "C:/lib");
  assert.equal(next.replicas[0].fileName, "完整名字.litematic");
});

test("悬空分类引用被改写为恢复分类，不产生悬空 ID", () => {
  const store = categoryStore();
  const next = store.normalizeLibraryState(state([entry("e1", { categoryIds: ["ghost"] })], []));
  const restoredId = next.entries[0].categoryIds[0];
  assert.notEqual(restoredId, "ghost");
  const restored = next.categories.find((category) => category.id === restoredId);
  assert.ok(restored);
  assert.equal(restored.name, "恢复的分类");
});

test("损坏父引用、循环与非法图标确定性恢复", () => {
  const store = categoryStore();
  const next = store.normalizeLibraryState(state([entry("e1", { categoryIds: ["lost"] })], [], {
    categories: [
      { id: "x", name: "X", parentId: "y", icon: { kind: "image", dataUrl: "javascript:alert(1)" } },
      { id: "y", name: "Y", parentId: "x" },
    ],
  }));
  const x = next.categories.find((category) => category.id === "x");
  assert.equal(x.parentId, null, "循环父引用被断开");
  assert.equal(x.icon.kind, "lucide", "非法图标回退为内置图标");
  assert.ok(next.categories.find((category) => category.name === "恢复的分类"));
  assert.deepEqual(store.normalizeLibraryState(next), next, "损坏恢复必须幂等");
});

test("分类树：新建/重命名/移动保持稳定 ID，禁止自循环及子孙循环", () => {
  const store = categoryStore();
  let tree = store.createLibraryCategory(state([], []), "父分类", null, "parent");
  tree = store.createLibraryCategory(tree, "子分类", "parent", "child");
  tree = store.createLibraryCategory(tree, "目标", null, "target");
  tree = store.updateLibraryCategory(tree, "child", { name: "子类新名" });
  assert.equal(store.categoryPath(tree, "child"), "父分类 / 子类新名");
  assert.throws(() => store.updateLibraryCategory(tree, "parent", { parentId: "child" }), /自身或子分类/);
  assert.throws(() => store.updateLibraryCategory(tree, "child", { parentId: "child" }), /自身或子分类/);
  tree = store.updateLibraryCategory(tree, "child", { parentId: "target" });
  assert.equal(store.categoryPath(tree, "child"), "目标 / 子类新名");
  assert.deepEqual([...store.categoryDescendants(tree, "parent")], ["parent"]);
  // Same-position duplicate name is rejected; same name under another parent is allowed.
  assert.throws(() => store.createLibraryCategory(tree, "目标", null, "duplicate"), /已存在/);
  const allowed = store.createLibraryCategory(tree, "目标", "parent", "allowed");
  assert.ok(allowed.categories.find((category) => category.id === "allowed"));
});

test("删除分类把受影响副本移入回收区并建回收记录，可还原", () => {
  const store = categoryStore();
  const base = state(
    [entry("e1", { categoryIds: ["child", "grandchild"] }), entry("e2", { categoryIds: ["grandchild"] }), entry("e3")],
    [replica("r1", "e1", { path: "C:/lib/A.litematic" }), replica("r2", "e2", { path: "C:/lib/B.litematic" }), replica("r3", "e3", { path: "C:/lib/C.litematic" })],
    { categories: [
      { id: "parent", name: "父", parentId: null, sort_index: 0, icon: icon() },
      { id: "child", name: "子", parentId: "parent", sort_index: 1, icon: icon() },
      { id: "grandchild", name: "孙", parentId: "child", sort_index: 2, icon: icon() },
    ] },
  );
  const next = store.deleteLibraryCategory(base, "child");
  assert.equal(next.categories.some((category) => category.id === "child" || category.id === "grandchild"), false);
  assert.equal(next.replicas.find((item) => item.id === "r1").status, "trashed");
  assert.equal(next.replicas.find((item) => item.id === "r2").status, "trashed");
  assert.equal(next.entries.find((item) => item.id === "e1").status, "retired");
  assert.equal(next.entries.find((item) => item.id === "e2").status, "retired");
  assert.equal(next.entries.find((item) => item.id === "e3").status, "active", "未受影响的条目保持不变");
  assert.equal(next.trash.length, 1);
  assert.equal(next.trash[0].kind, "category");
  assert.equal(next.trash[0].name, "子");
  assert.equal(next.trash[0].status, "in_trash");

  const restored = store.restoreLibraryTrash(next, next.trash[0].id);
  assert.equal(restored.trash.length, 0);
  assert.equal(restored.replicas.find((item) => item.id === "r1").status, "active");
  assert.equal(restored.entries.find((item) => item.id === "e1").status, "active");
});

test("批量移动分类只改条目归类，副本路径与其它条目不变", () => {
  const store = categoryStore();
  const base = state(
    [entry("e1", { categoryIds: ["from"] }), entry("e2")],
    [replica("r1", "e1", { path: "A" }), replica("r2", "e2", { path: "B" })],
    { categories: [{ id: "to", name: "目标", parentId: null, sort_index: 0, icon: icon() }] },
  );
  const moved = store.moveLibraryRecordsToCategory(base, ["A"], "to");
  assert.deepEqual(moved.entries.find((item) => item.id === "e1").categoryIds, ["to"]);
  assert.deepEqual(moved.entries.find((item) => item.id === "e2").categoryIds, []);
  assert.equal(moved.replicas.find((item) => item.id === "r1").path, "A");
  assert.equal(moved.replicas.find((item) => item.id === "r2").path, "B");
});

test("loadLibrary：迁移只持久化一次；写入失败向调用方暴露且不清空已有数据", async () => {
  let text = JSON.stringify({ records: [legacyRecord({ tags: ["甲", "乙"] })], folders: [] });
  let writes = 0;
  const api = categoryStore({
    readUserConfigFile: async () => text,
    writeUserConfigFile: async (_path, next) => { text = next; writes += 1; },
    getUserConfigFilePath: async () => "C:/builtin/import",
  });
  const first = await api.loadLibrary();
  const second = await api.loadLibrary();
  assert.equal(writes, 1, "v5 结果不得再次写盘");
  assert.equal(first.schemaVersion, 5);
  assert.equal(first.entries.length, 1);
  assert.deepEqual(second, first);

  const denied = categoryStore({
    readUserConfigFile: async () => JSON.stringify({ records: [legacyRecord({ tags: ["甲"] })], folders: [] }),
    writeUserConfigFile: async () => { throw new Error("磁盘写入失败"); },
    getUserConfigFilePath: async () => "C:/builtin/import",
  });
  await assert.rejects(denied.loadLibrary(), /磁盘写入失败/);
});

test("setRecordPreview 更新条目预览并保留归类；reorderLibraryRecords 重排条目", async () => {
  const api = categoryStore({ readUserConfigFile: async () => { throw new Error("no disk"); }, writeUserConfigFile: async () => {} });
  const base = state(
    [entry("e1", { categoryIds: ["test"], sortIndex: 5 })],
    [replica("r1", "e1", { path: "A" })],
    { categories: [{ id: "test", name: "测试", parentId: null, sort_index: 0, icon: icon() }] },
  );
  const previewed = await api.setRecordPreview(base, "A", "新截图.png");
  assert.equal(previewed.entries[0].preview.path, "新截图.png");
  assert.equal(previewed.entries[0].preview.preview_image_path, "新截图.png");
  assert.deepEqual(previewed.entries[0].categoryIds, ["test"]);
  assert.equal(previewed.categories.find((category) => category.id === "test").name, "测试");

  const reordered = await api.reorderLibraryRecords(previewed, ["A"]);
  assert.equal(reordered.entries[0].sortIndex, 0);
});

test("SVG 图标：基础图形可用；脚本/事件/外链/样式/实体/动画一律拒绝", () => {
  assert.match(icons.sanitizeLibrarySvg('<svg viewBox="0 0 20 20"><path d="M0 0L20 20" stroke="currentColor"/></svg>'), /xmlns=/);
  for (const bad of ['<script>alert(1)</script>', '<path onclick="x()"/>', '<image href="https://example.com/a"/>', '<use href="#a"/>', '<style>path{fill:url(https://example.com)}</style>', '<path style="fill:red"/>', '<animate attributeName="x"/>', '<foreignObject><div>hi</div></foreignObject>', '<path fill="url(#a)"/>']) {
    assert.throws(() => icons.sanitizeLibrarySvg(`<svg xmlns="http://www.w3.org/2000/svg">${bad}</svg>`));
  }
  assert.throws(() => icons.sanitizeLibrarySvg('<!DOCTYPE svg [<!ENTITY x SYSTEM "file:///x">]><svg/>'));
});

test("上传图标仅接受有匹配签名的受限 data 图片；真实 Minecraft ID 不换成 SVG", () => {
  const png = "data:image/png;base64," + Buffer.from([137, 80, 78, 71, 13, 10, 26, 10, 0]).toString("base64");
  assert.equal(icons.validateLibraryIconDataUrl(png), png);
  const svg = "data:image/svg+xml;base64," + Buffer.from('<svg><circle cx="5" cy="5" r="4"/></svg>').toString("base64");
  assert.match(icons.validateLibraryIconDataUrl(svg), /^data:image\/svg\+xml;base64,/);
  assert.throws(() => icons.validateLibraryIconDataUrl("data:image/png;base64," + Buffer.from("<script>bad</script>").toString("base64")));
  assert.throws(() => icons.validateLibraryIconDataUrl("data:text/html;base64,SGk="));
  assert.throws(() => icons.validateLibraryIconDataUrl("https://example.com/a.png"));
  assert.deepEqual(icons.normalizeLibraryCategoryIcon({ kind: "minecraft", blockId: "minecraft:chest" }), { kind: "minecraft", blockId: "minecraft:chest" });
  assert.equal(icons.normalizeLibraryCategoryIcon({ kind: "minecraft", blockId: "javascript:alert(1)" }).kind, "lucide");
});