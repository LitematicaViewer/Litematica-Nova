import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import { JSDOM } from "jsdom";

const window = new JSDOM("").window;
function compile(path, dependencies) {
  const compiled = ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } });
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports,DOMParser,XMLSerializer,TextEncoder,TextDecoder,atob,btoa){${compiled.outputText}})`)(dependencies, module, module.exports, window.DOMParser, window.XMLSerializer, TextEncoder, TextDecoder, atob, btoa);
  return module.exports;
}
const icons = compile("../src/services/libraryCategoryIcons.ts", () => { throw new Error("Unexpected icon import"); });
export function categoryStore(backend = {}) {
  return compile("../src/services/libraryStore.ts", (name) => name === "./libraryCategoryIcons" ? icons : name === "./backend" ? backend : name.endsWith("applicationOperations") ? { inspectProject: async () => ({ metadata: { name: "新名称", total_blocks: 44 }, regions: [{ name: "第一选区" }, { name: "第二选区" }] }) } : (() => { throw new Error(name); })());
}
const store = categoryStore();
const record = (path, tags = []) => ({ id: path, path, fileName: path, displayName: path, author: "作者", description: "保留描述", totalBlocks: 12, totalVolume: 18, regionCount: 1, enclosingSize: { x: 2, y: 3, z: 3 }, minecraftDataVersion: 5, fileSize: 456, mtime: 789, tags, status: "ok", lastAnalyzedAt: 1, lastError: "", previewPath: "真实预览.png", sort_index: 4 });
const empty = () => ({ schemaVersion: 2, categories: [], records: [], folders: [] });

test("文件名称：旧缓存按实际路径规范化，完整扩展保留，不猜选区名", () => {
  const original = { ...record("C:\\投影\\真正文件.LITEMATIC"), fileName: "旧文件名", displayName: "旧内部元数据名称" };
  const other = { ...record("/projection/完整名.litematic"), displayName: "另一个内部名称", regionNames: ["北区", "南区"] };
  const next = store.normalizeLibraryState({ ...empty(), records: [original, other] });
  assert.equal(next.records[0].fileName, "真正文件.LITEMATIC"); assert.equal(next.records[0].displayName, "真正文件.LITEMATIC");
  assert.equal(next.records[0].regionNames, undefined); assert.equal(next.records[0].internalName, undefined);
  assert.equal(next.records[1].displayName, "完整名.litematic"); assert.deepEqual(next.records[1].regionNames, ["北区", "南区"]);
  assert.equal(next.records[0].path, original.path); assert.equal(next.records[0].previewPath, original.previewPath);
  assert.equal(original.displayName, "旧内部元数据名称"); assert.deepEqual(store.normalizeLibraryState(next), next);
});

test("分析名称：内部名称与文件名分离，所有选区原名由真实regions映射并持久化", async () => {
  const parsed = { metadata: { name: "NBT内部名称", region_count: 3, author: "作者" }, regions: [{ name: "一号选区" }, { name: "多层完整选区" }, { name: "末选区" }] };
  const original = structuredClone(parsed); const analysis = store.projectionRecordAnalysis(parsed);
  assert.equal(analysis.displayName, undefined); assert.equal(analysis.internalName, "NBT内部名称");
  assert.deepEqual(analysis.regionNames, parsed.regions.map((region) => region.name)); assert.deepEqual(parsed, original);
  let saved; const api = categoryStore({ writeUserConfigFile: async (_, value) => { saved = JSON.parse(value); } });
  const path = "C:/投影/文件名.litematic";
  const next = await api.addOrUpdateRecordFromAnalysis(empty(), path, { ...analysis, displayName: "错误显示名" });
  assert.equal(next.records[0].displayName, "文件名.litematic"); assert.equal(next.records[0].internalName, "NBT内部名称");
  assert.deepEqual(saved.records[0].regionNames, ["一号选区", "多层完整选区", "末选区"]);
  const inspected = await api.analyzeFile(path);
  assert.equal(inspected.displayName, "文件名.litematic"); assert.equal(inspected.internalName, "新名称");
  assert.deepEqual(inspected.regionNames, ["第一选区", "第二选区"]);
  assert.equal(store.projectionRecordAnalysis({ metadata: { name: "不能当作选区名", region_count: 2 } }).regionNames, undefined);
  assert.equal(store.projectionRecordAnalysis({ regions: [{ name: "有效" }, {}] }).regionNames, undefined);
  assert.deepEqual(store.projectionRecordAnalysis({ regions: [] }).regionNames, []);
});

test("旧标签幂等迁移：多分配、预览/元数据、排序与本地文件夹完整保留", () => {
  const old = { records: [record("A", ["建筑", "红石", "红石"]), record("B", ["红石"]), record("C")], folders: [{ path: "C:/投影", enabled: false, recursive: false, sort_index: 0, lastSyncedAt: 19, lastScanCount: 3, lastError: "历史状态" }] };
  const next = store.normalizeLibraryState(old);
  assert.equal(next.schemaVersion, 2); assert.equal(next.categories.length, 14);
  assert.deepEqual(next.records[0].categoryIds.map((id) => next.categories.find((x) => x.id === id).name), ["建筑", "红石"]);
  assert.equal(next.records[1].categoryIds[0], next.records[0].categoryIds[1]); assert.deepEqual(next.records[2].categoryIds, []);
  assert.equal("tags" in next.records[0], false); assert.equal(next.records[0].path, old.records[0].path); assert.equal(next.records[0].description, "保留描述");
  assert.equal(next.records[0].preview_image_path, "真实预览.png"); assert.equal(next.records[0].sort_index, 4); assert.deepEqual(next.folders, old.folders);
  assert.deepEqual(store.normalizeLibraryState(next), next); assert.deepEqual(old.records[0].tags, ["建筑", "红石", "红石"]);
});

test("迁移只持久化一次；写入失败向调用方暴露，不清空已有数据", async () => {
  let text = JSON.stringify({ records: [record("A", ["甲", "乙"])], folders: [] }); let writes = 0;
  const api = categoryStore({ readUserConfigFile: async () => text, writeUserConfigFile: async (_, next) => { text = next; writes++; } });
  const first = await api.loadLibrary(); const second = await api.loadLibrary(); assert.deepEqual(second, first); assert.equal(writes, 1);
  const denied = categoryStore({ readUserConfigFile: async () => JSON.stringify({ records: [record("A", ["甲"])], folders: [] }), writeUserConfigFile: async () => { throw new Error("磁盘写入失败"); } });
  await assert.rejects(denied.loadLibrary(), /磁盘写入失败/);
});

test("树操作：新建/重命名/移动保持稳定ID，禁止自循环及子孙循环", () => {
  let state = store.createLibraryCategory(empty(), "父分类", null, "parent"); state = store.createLibraryCategory(state, "子分类", "parent", "child"); state = store.createLibraryCategory(state, "目标", null, "target");
  state = store.updateLibraryCategory(state, "child", { name: "子类新名" }); assert.equal(store.categoryPath(state, "child"), "父分类 / 子类新名");
  assert.throws(() => store.updateLibraryCategory(state, "parent", { parentId: "child" }), /自身或子分类/); assert.throws(() => store.updateLibraryCategory(state, "child", { parentId: "child" }), /自身或子分类/);
  state = store.updateLibraryCategory(state, "child", { parentId: "target" }); assert.equal(store.categoryPath(state, "child"), "目标 / 子类新名"); assert.deepEqual([...store.categoryDescendants(state, "parent")], ["parent"]);
  assert.throws(() => store.createLibraryCategory(state, "子类新名", "target", "duplicate"), /已存在/);
});

test("删除分类只向上归并引用和子树；批量移动不改文件、不丢其他记录", () => {
  let state = store.createLibraryCategory(empty(), "父", null, "parent"); state = store.createLibraryCategory(state, "子", "parent", "child"); state = store.createLibraryCategory(state, "孙", "child", "grandchild");
  state.records = [ { ...record("A"), categoryIds: ["child", "grandchild"] }, { ...record("B"), categoryIds: ["grandchild"] } ];
  const next = store.deleteLibraryCategory(state, "child"); assert.deepEqual(next.records[0].categoryIds, ["parent", "grandchild"]); assert.equal(next.categories.find((x) => x.id === "grandchild").parentId, "parent");
  const moved = store.moveLibraryRecordsToCategory(next, ["A"], "grandchild"); assert.deepEqual(moved.records[0].categoryIds, ["grandchild"]); assert.equal(moved.records[0].previewPath, "真实预览.png"); assert.equal(moved.records[0].path, "A"); assert.deepEqual(moved.records[1], next.records[1]);
  const deletedRoot = store.deleteLibraryCategory(moved, "parent"); assert.equal(deletedRoot.categories.find((x) => x.id === "grandchild").parentId, null);
  assert.equal(store.normalizeLibraryState({ ...empty(), categories: [] }).categories.length, 0, "deleted presets stay deleted");
});

test("损坏父引用、循环与孤立成员确定性恢复，图标非法值回退", () => {
  const state = store.normalizeLibraryState({ ...empty(), records: [{ ...record("A"), categoryIds: ["lost"] }], categories: [{ id: "x", name: "X", parentId: "y", icon: { kind: "image", dataUrl: "javascript:alert(1)" } }, { id: "y", name: "Y", parentId: "x" }] });
  assert.equal(state.categories[0].parentId, null); assert.equal(state.categories[0].icon.kind, "lucide"); assert.equal(state.categories.find((x) => x.id === "lost").name, "恢复的分类"); assert.deepEqual(store.normalizeLibraryState(state), state);
});

test("分类与真实预览在重新分析、排序和文件夹配置中保留", async () => {
  const api = categoryStore({ writeUserConfigFile: async () => {}, checkFileExists: async () => true });
  let state = store.createLibraryCategory(empty(), "测试", null, "test"); state.records = [{ ...record("A"), categoryIds: ["test"] }];
  state = await api.addOrUpdateRecord(state, "A"); assert.deepEqual(state.records[0].categoryIds, ["test"]); assert.equal(state.records[0].preview_image_path, "真实预览.png");
  state = await api.addLocalLibraryFolder(state, "C:/同步"); state = await api.setRecordPreview(state, "A", "新截图.png"); state = await api.reorderLibraryRecords(state, ["A"]);
  assert.deepEqual(state.records[0].categoryIds, ["test"]); assert.equal(state.categories[0].name, "测试"); assert.equal(state.records[0].previewPath, "新截图.png"); assert.equal(state.folders[0].path, "C:/同步");
});

test("SVG图标：基础图形可用；脚本/事件/外链/样式/实体/动画拒绝", () => {
  assert.match(icons.sanitizeLibrarySvg('<svg viewBox="0 0 20 20"><path d="M0 0L20 20" stroke="currentColor"/></svg>'), /xmlns=/);
  for (const bad of ['<script>alert(1)</script>', '<path onclick="x()"/>', '<image href="https://example.com/a"/>', '<use href="#a"/>', '<style>path{fill:url(https://example.com)}</style>', '<path style="fill:red"/>', '<animate attributeName="x"/>', '<foreignObject><div>hi</div></foreignObject>', '<path fill="url(#a)"/>']) assert.throws(() => icons.sanitizeLibrarySvg(`<svg xmlns="http://www.w3.org/2000/svg">${bad}</svg>`));
  assert.throws(() => icons.sanitizeLibrarySvg('<!DOCTYPE svg [<!ENTITY x SYSTEM "file:///x">]><svg/>'));
});

test("上传图标仅支持有匹配签名的受限data图片，SVG再次校验；真实Minecraft ID不换成SVG", () => {
  const png = 'data:image/png;base64,' + Buffer.from([137,80,78,71,13,10,26,10,0]).toString('base64'); assert.equal(icons.validateLibraryIconDataUrl(png), png);
  const svg = 'data:image/svg+xml;base64,' + Buffer.from('<svg><circle cx="5" cy="5" r="4"/></svg>').toString('base64'); assert.match(icons.validateLibraryIconDataUrl(svg), /^data:image\/svg\+xml;base64,/);
  assert.throws(() => icons.validateLibraryIconDataUrl('data:image/png;base64,' + Buffer.from('<script>bad</script>').toString('base64')));
  assert.throws(() => icons.validateLibraryIconDataUrl('data:text/html;base64,SGk=')); assert.throws(() => icons.validateLibraryIconDataUrl('https://example.com/a.png'));
  assert.deepEqual(icons.normalizeLibraryCategoryIcon({ kind: "minecraft", blockId: "minecraft:chest" }), { kind: "minecraft", blockId: "minecraft:chest" });
  assert.equal(icons.normalizeLibraryCategoryIcon({ kind: "minecraft", blockId: "javascript:alert(1)" }).kind, "lucide");
  assert.throws(() => icons.sanitizeLibrarySvg('<svg><title>' + 'x'.repeat(65536) + '</title></svg>'));
});
