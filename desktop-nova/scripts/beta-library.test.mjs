import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import React from "react";
import ts from "typescript";

// F.1 rewrite: the library page is now an orchestration container (D.3) that composes
// LibraryToolbar / LibraryCategoryTree / LibraryGrid / LibraryList / LibraryDetail /
// LibraryGrouping / LibraryColumnPicker / LibraryTaskCenter and the EntryCard binding.
// The suite loads the real container with its real service layer and drives the new
// props contract through probe components, so nothing here depends on a real file on
// disk (the old suite hard-coded `C:\Users\...\16核刷铁机.litematic`).
const root = new URL("../", import.meta.url);
const source = (path) => readFileSync(new URL(path, root), "utf8");
const tick = () => new Promise((resolve) => setImmediate(resolve));

const moduleCache = new Map();
function load(path, dependencies, fresh = false) {
  if (!fresh && moduleCache.has(path)) return moduleCache.get(path);
  const code = ts.transpileModule(source(path), { fileName: path, compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${code}})`)(dependencies, module, module.exports);
  if (!fresh) moduleCache.set(path, module.exports);
  return module.exports;
}
const paths = load("src/services/filePathIdentity.ts", () => { throw new Error("unexpected"); });
const icons = load("src/services/libraryCategoryIcons.ts", () => { throw new Error("unexpected"); });
const migration = load("src/services/libraryMigration.ts", (name) => name === "./filePathIdentity" ? paths : name === "./libraryCategoryIcons" ? icons : (() => { throw new Error(`migration dep ${name}`); })());
const content = load("src/services/libraryContent.ts", (name) => name === "./filePathIdentity" ? paths : name === "./libraryStore" ? {} : (() => { throw new Error(`content dep ${name}`); })());

const probe = (name) => (props) => React.createElement(name, props);
const label = (value) => value == null || typeof value === "boolean" ? "" : Array.isArray(value) ? value.map(label).join("") : typeof value === "object" ? label(value.props?.children) : String(value);

function createHarness(state, overrides = {}) {
  const disk = { text: JSON.stringify(state) };
  const calls = { selected: [], saved: [], events: [], opened: [] };
  const backend = {
    readUserConfigFile: async () => disk.text,
    writeUserConfigFile: async (_path, next) => { disk.text = next; calls.saved.push(JSON.parse(next)); },
    checkFileExists: async () => true,
    listLitematicFileEntriesInDirectory: async () => [],
    copyFileToDirectory: async () => ({}),
    computeLitematicChecksum: async () => ({ sha256: "x", byte_count: 1 }),
    getUserConfigFilePath: async () => "C:/builtin/import",
    libraryTrashMove: async (items) => items.map((item) => ({ sourcePath: item.sourcePath, trashPath: `C:/trash/${item.trashName}`, ok: true })),
    libraryTrashRestore: async (items) => items.map((item) => ({ trashPath: item.trashPath, targetPath: item.targetPath, ok: true })),
    libraryTrashPurge: async (items) => items.map((item) => ({ trashPath: item.trashPath, ok: true })),
    libraryExportZip: async () => ({ taskId: "zip" }),
    ...(overrides.backend || {}),
  };
  const store = load("src/services/libraryStore.ts", (name) => {
    if (name === "./libraryCategoryIcons") return icons;
    if (name === "./filePathIdentity") return paths;
    if (name === "./backend") return backend;
    if (name === "../platform/applicationOperations") return { inspectProject: async () => ({ metadata: {}, regions: [] }), editProjectMetadata: async () => ({}) };
    if (name === "./libraryMigration") return migration;
    if (name === "./libraryContent") return content;
    if (name === "./libraryTasks") return { submitLibraryTask: async () => "t", cancelLibraryTask: async () => true, subscribeLibraryTask: async () => () => {}, submitAndAwait: async () => ({ taskId: "t", kind: "x", status: "completed", summary: {}, failures: [] }), awaitLibraryTask: async () => ({ taskId: "t", kind: "x", status: "completed", summary: {}, failures: [] }), ...(overrides.tasks || {}) };
    if (name === "./statsService") return { formatMaterialQuantity: () => "", loadStructureStats: async () => ({ materials: [] }) };
    if (name === "./minecraftVersions") return { minecraftVersionLabel: (value) => `v${value}` };
    if (name === "./libraryIndexClient") return {
      queryIndex: async (query) => ({ entryIds: overrides.indexMatches ? overrides.indexMatches(query) : [], ready: true }),
      submitIndexBuild: async () => "idx",
      subscribeIndexStatus: async () => () => {},
    };
    throw new Error(`store dep ${name}`);
  }, true);
  const facade = {
    ...store,
    windowsPathKey: paths.windowsPathKey,
    displayPathText: paths.displayPathText,
    loadUserConfigMigratingLocalStorage: async () => ({ config: {} }),
    generatePreviewImage: async (path) => ({ preview_path: `${path}.png`, data_url: "data:generated" }),
    readProjectionPreviewImage: async (path) => { calls.previews = (calls.previews || []); calls.previews.push(path); return overrides.preview ? overrides.preview(path) : { data_url: `data:preview,${path}` }; },
    getPathInfo: async (path) => ({ path }),
    openFileParentDir: async (path) => { calls.opened.push(path); },
    selectLitematicFile: async () => overrides.picked ?? null,
    ...(overrides.facade || {}),
  };
  const libraryEntryView = load("ui/windows/main/pages/library/libraryEntryView.ts", (name) => {
    if (name.endsWith("src/business/facade")) return facade;
    if (name.endsWith("src/services/minecraftVersions")) return { minecraftVersionLabel: (value) => `v${value}` };
    if (name === "./library.css") return {};
    throw new Error(`entryView dep ${name}`);
  }, true);
  const elementStubs = {
    "./LibraryToolbar": { LibraryToolbar: "LibraryToolbar" },
    "./LibraryCategoryTree": { LibraryCategoryTree: "LibraryCategoryTree" },
    "./LibraryGrid": { LibraryGrid: "LibraryGrid" },
    "./LibraryList": { LibraryList: "LibraryList" },
    "./LibraryDetail": { LibraryDetail: "LibraryDetail" },
    "./LibraryGrouping": { LibraryGrouping: "LibraryGrouping" },
    "./LibraryColumnPicker": { LibraryColumnPicker: "LibraryColumnPicker" },
    "./LibraryTaskCenter": { LibraryTaskCenter: "LibraryTaskCenter" },
    "./sendDialog": { SendProjectionDialog: "SendProjectionDialog" },
    "./DropImportDialog": { DropImportDialog: "DropImportDialog" },
    "./ExportDialog": { ExportDialog: "ExportDialog" },
    "./ReplicaNameEditor": { ReplicaNameEditor: "ReplicaNameEditor" },
    "./categoryIcon": { CategoryIconDialog: "CategoryIconDialog" },
    "../../../local_library_folders": { LocalLibraryFoldersDialog: "LocalLibraryFoldersDialog", openLocalLibraryFoldersWithWindowBehavior: async () => {} },
    "../../../local_library_folders/TrashPanel": { TrashPanel: "TrashPanel" },
    "../../../reden_library": { RedenLibraryDialog: "RedenLibraryDialog", openRedenLibraryWithWindowBehavior: async () => {} },
    "../../../../components/Dialog": { Dialog: "dialog" },
    "../../../../components/Dropdown": { Dropdown: "dropdown" },
    "../../../../components/ThemeIcon": { ThemeIcon: "theme-icon" },
    "../../../../components/ui/button": { Button: "button" },
    "../../../../components/ui/input": { Input: "input" },
    "../../../../components/ui/checkbox": { Checkbox: "checkbox" },
    "../../../../components/ui/scroll-area": { ScrollArea: "scroll-area" },
    "./libraryEntryView": libraryEntryView,
    "./function": { ensureLitematicFileName: (value) => value ? `${value}.litematic` : value },
    "./library.css": {},
    "../../../../../src/services/libraryTasks": { submitAndAwait: async () => ({ taskId: "t", kind: "x", status: "completed", summary: {}, failures: [] }) },
    "../../../../../src/business/facade": facade,
    "../../../../../src/platform/events": {
      listenEvent: async (event, callback) => { (listeners.get(event) ?? listeners.set(event, [])).push(callback); return () => {}; },
      emitEvent: async (event, payload) => { calls.events.push({ event, payload }); },
    },
    "../../../libraryEvents": { projectionLibraryImportedEvent: "imported", projectionLibraryStateChangedEvent: "changed" },
  };
  const listeners = new Map();
  const slots = []; const effects = []; let cursor = 0;
  const hooks = {
    ...React,
    useState(initial) { const i = cursor++; if (!(i in slots)) slots[i] = typeof initial === "function" ? initial() : initial; return [slots[i], (next) => { slots[i] = typeof next === "function" ? next(slots[i]) : next; }]; },
    useRef(initial) { const i = cursor++; return slots[i] ??= { current: initial }; },
    useCallback(fn) { return fn; },
    useMemo(compute, deps) { const i = cursor++; if (!slots[i] || !deps || deps.some((dep, n) => dep !== slots[i].deps?.[n])) slots[i] = { deps, value: compute() }; return slots[i].value; },
    useEffect(callback, deps) { const i = cursor++; if (!slots[i] || !deps || deps.some((dep, n) => dep !== slots[i].deps?.[n])) { slots[i]?.cleanup?.(); slots[i] = { deps }; effects.push(() => { slots[i].cleanup = callback(); }); } },
  };
  const compiledPage = ts.transpileModule(source("ui/windows/main/pages/library/LibraryPage.tsx"), { fileName: "LibraryPage.tsx", compilerOptions: { jsx: ts.JsxEmit.React, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText;
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${compiledPage}\n})`)((name) => {
    if (name === "react") return hooks;
    if (name === "lucide-react") return new Proxy({}, { get: (_, key) => `icon-${String(key)}` });
    if (name in elementStubs) return elementStubs[name];
    throw new Error(`Unexpected dependency: ${name}`);
  }, module, module.exports);

  let tree;
  const props = { currentFile: "", setCurrentFile: (value) => { calls.selected.push(value); props.currentFile = value; }, setRoute: () => {} };
  const h = {
    calls, props,
    render() { cursor = 0; tree = module.exports.LibraryPage(props); while (effects.length) effects.shift()(); return tree; },
    nodes() { const list = []; const visit = (value) => { if (Array.isArray(value)) return value.forEach(visit); if (!React.isValidElement(value)) return; list.push(value); visit(value.props.children); visit(value.props.footer); }; visit(tree); return list; },
    find(predicate) { return this.nodes().find(predicate); },
    component(type) { return this.find((node) => node.type === type); },
    components(type) { return this.nodes().filter((node) => node.type === type); },
    button(text) { return this.find((node) => node.type === "button" && label(node.props.children) === text); },
    input(ariaLabel) { return this.find((node) => node.props["aria-label"] === ariaLabel); },
    async settle(rounds = 3) { for (let i = 0; i < rounds; i += 1) { await tick(); this.render(); } },
    async search(value) { h.component("LibraryToolbar").props.onSearchChange(value); h.render(); await new Promise((resolve) => setTimeout(resolve, 260)); h.render(); },
  };
  h.render();
  return h;
}

const entry = (id, over = {}) => ({
  id, createdAt: 0, checksum: null, status: "active", categoryIds: [], displayName: `${id}.litematic`,
  internalName: undefined, author: "作者", description: "说明", tags: [], preview: {},
  stats: { totalBlocks: 100, totalVolume: 300, regionCount: 1, enclosingSize: { x: 4, y: 5, z: 6 }, minecraftDataVersion: 5000 },
  sortIndex: 0, lastAnalyzedAt: 0, lastError: "", ...over,
});
const replica = (id, entryId, over = {}) => ({
  id, entryId, path: `C:/library/${id}.litematic`, fileName: `${id}.litematic`, folderPath: "C:/library",
  checksum: null, fileSize: 1024, mtime: 1, status: "active", lastAnalyzedAt: 0, lastError: "", ...over,
});
const library = (entries, replicas, over = {}) => ({
  schemaVersion: 5, entries, replicas, folders: [{ path: "C:/library", type: "tracked", enabled: true, recursive: true, sort_index: 0, lastSyncedAt: 0, lastScanCount: 0, lastError: "" }], categories: [], trash: [], redirects: [], favorites: [], recent: [], ...over,
});
const category = (id, name, parentId = null) => ({ id, name, parentId, sort_index: 0, icon: { kind: "lucide", name: "Folder" } });

test("加载：容器把 active 条目按目录交给网格视图，主名称保留完整文件名", async () => {
  const h = createHarness(library([entry("e1"), entry("e2")], [replica("r1", "e1"), replica("r2", "e2")]));
  await h.settle();
  const grid = h.component("LibraryGrid");
  assert.ok(grid, `网格视图应渲染；nodes=${h.nodes().map((node) => (typeof node.type === "string" ? node.type : node.type?.name || "?")).join(",")}`);
  assert.equal(grid.props.entries.length, 2);
  assert.deepEqual(grid.props.entries.map((view) => view.displayName).sort(), ["e1.litematic", "e2.litematic"]);
  assert.equal(grid.props.entries[0].primary.path.startsWith("C:/library/"), true);
  assert.equal(h.component("LibraryToolbar").props.search, "");
});

test("搜索：经后台索引过滤条目；索引未就绪时角标提示构建中", async () => {
  const h = createHarness(
    library([entry("e1", { displayName: "红石机" }), entry("e2", { displayName: "建筑示例" })], [replica("r1", "e1"), replica("r2", "e2")]),
    { indexMatches: (query) => query.text === "红石" ? ["e1"] : [] },
  );
  await h.settle();
  await h.search("红石");
  const grid = h.component("LibraryGrid");
  assert.deepEqual(grid.props.entries.map((view) => view.entryId), ["e1"], "搜索命中只保留索引匹配项");
  assert.equal(h.component("LibraryToolbar").props.search, "红石");
  h.component("LibraryToolbar").props.onSearchChange("");
  await h.settle();
  assert.equal(h.component("LibraryToolbar").props.search, "");
  assert.equal(h.component("LibraryGrid").props.entries.length, 2, "清除搜索后恢复全部条目");
});

test("选择：勾选条目出现批量工具栏并显示已选数量，取消选择恢复", async () => {
  const h = createHarness(library([entry("e1"), entry("e2")], [replica("r1", "e1"), replica("r2", "e2")]));
  await h.settle();
  const grid = h.component("LibraryGrid");
  grid.props.binding.onToggle(grid.props.entries[0], true);
  h.render();
  assert.ok(h.button("移动到分类…"), "选中后出现批量工具栏");
  assert.ok(h.button("移入回收区"));
  assert.match(label(h.find((node) => node.props.className === "library-select-all").props.children), /已选 1/);
  h.button("取消选择").props.onClick();
  h.render();
  assert.equal(h.button("移动到分类…"), undefined);
});

test("视图切换：网格 → 列表 → 详情使用对应子组件", async () => {
  const h = createHarness(library([entry("e1")], [replica("r1", "e1")]));
  await h.settle();
  h.component("LibraryToolbar").props.onViewChange("list");
  h.render();
  assert.ok(h.component("LibraryList"), "列表视图应渲染 LibraryList");
  h.component("LibraryToolbar").props.onViewChange("detail");
  h.render();
  const detail = h.components("LibraryDetail").find((node) => node.props.variant === "full");
  assert.ok(detail, "详情视图应渲染 variant=full 的 LibraryDetail");
  assert.equal(h.component("LibraryGrid"), undefined, "详情视图不再渲染网格");
});

test("分类筛选：选择未分类只保留无归类条目", async () => {
  const h = createHarness(library([entry("e1", { categoryIds: ["c1"] }), entry("e2")], [replica("r1", "e1"), replica("r2", "e2")], { categories: [category("c1", "仓库")] }));
  await h.settle();
  h.component("LibraryCategoryTree").props.onSelectUntagged();
  h.render();
  assert.deepEqual(h.component("LibraryGrid").props.entries.map((view) => view.entryId), ["e2"]);
  h.component("LibraryCategoryTree").props.onSelectCategory("c1");
  h.render();
  assert.deepEqual(h.component("LibraryGrid").props.entries.map((view) => view.entryId), ["e1"]);
});

test("排序：按名称排序改变条目顺序", async () => {
  const h = createHarness(library([entry("e1", { displayName: "Gamma" }), entry("e2", { displayName: "Alpha" }), entry("e3", { displayName: "Beta" })], [replica("r1", "e1"), replica("r2", "e2"), replica("r3", "e3")]));
  await h.settle();
  h.component("LibraryToolbar").props.onSortModeChange("name");
  h.render();
  assert.deepEqual(h.component("LibraryGrid").props.entries.map((view) => view.displayName), ["Alpha", "Beta", "Gamma"]);
});

test("分页：每页 20 条并提供下一页", async () => {
  const entries = Array.from({ length: 21 }, (_, index) => entry(`e${index}`, { sortIndex: index }));
  const h = createHarness(library(entries, entries.map((item) => replica(`r-${item.id}`, item.id))));
  await h.settle();
  assert.equal(h.component("LibraryGrid").props.entries.length, 20);
  const next = h.button("下一页");
  assert.ok(next);
  next.props.onClick();
  h.render();
  assert.equal(h.component("LibraryGrid").props.entries.length, 1);
});

test("刷新失败提示并保留原列表，不写盘不丢条目", async () => {
  const h = createHarness(library([entry("e1"), entry("e2")], [replica("r1", "e1"), replica("r2", "e2")]), {
    tasks: { submitAndAwait: async () => { throw new Error("磁盘不可用"); } },
  });
  await h.settle();
  const savesBefore = h.calls.saved.length;
  await h.component("LibraryToolbar").props.onRefresh();
  await h.settle();
  const alert = h.find((node) => node.props.role === "alert");
  assert.ok(alert);
  assert.match(label(alert.props.children), /刷新失败，原有列表已保留/);
  assert.equal(h.component("LibraryGrid").props.entries.length, 2, "失败不得清空列表");
  assert.equal(h.calls.saved.length, savesBefore, "刷新失败不得新增写盘");
});