import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import React from "react";
import ts from "typescript";

// Tests the actual library handlers with isolated in-memory persistence, not a renderer or visual acceptance.
const source = readFileSync(new URL("../ui/windows/main/pages/library/LibraryPage.tsx", import.meta.url), "utf8");
const compiled = ts.transpileModule(source, { fileName: "LibraryPage.tsx", compilerOptions: {
  jsx: ts.JsxEmit.React, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, esModuleInterop: true,
} }).outputText;
function categoryMethods() {
  const load = (relative, dependencies) => {
    const code = ts.transpileModule(readFileSync(new URL(relative, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
    const module = { exports: {} }; vm.runInThisContext(`(function(require,module,exports){${code}})`)(dependencies, module, module.exports); return module.exports;
  };
  const icons = load("../src/services/libraryCategoryIcons.ts", () => ({}));
  return load("../src/services/libraryStore.ts", (name) => name === "./libraryCategoryIcons" ? icons : {});
}
const categories = categoryMethods();
const tick = () => new Promise((resolve) => setImmediate(resolve));
const record = (index, tags = []) => ({ id: `id-${index}`, path: `C:/library/${index}.litematic`, fileName: `${index}.litematic`,
  displayName: `投影 ${index}`, author: "作者", description: "真实元数据", totalBlocks: 100 + index, totalVolume: 300, regionCount: 1,
  enclosingSize: { x: 4, y: 5, z: 6 }, minecraftDataVersion: 5000, fileSize: 1024, mtime: 123, tags,
  status: "ok", lastAnalyzedAt: index, lastError: "", sort_index: index,
});
function label(value) {
  if (value == null || typeof value === "boolean") return "";
  if (Array.isArray(value)) return value.map(label).join("");
  if (typeof value === "object") return label(value.props?.children);
  return String(value);
}
function createHarness(records, overrides = {}) {
  let library = categories.normalizeLibraryState({ records: structuredClone(records), folders: [{ path: "C:/library", enabled: true, recursive: true }] });
  const calls = { saved: [], previews: [], generated: [], generatedModes: [], images: [], online: 0, selected: [], reordered: [], events: [] };
  const facade = {
    ...categories,
    loadLibrary: async () => structuredClone(library),
    saveLibrary: async (next) => { calls.saved.push(structuredClone(next)); library = structuredClone(next); },
    loadUserConfigMigratingLocalStorage: async () => ({ config: { preview_mode: "normal" } }),
    normalizeDisplayMode: (value) => value,
    readProjectionPreviewImage: async (path) => { calls.previews.push(path); return { data_url: `data:preview,${path}` }; },
    readImageBase64: async (path) => { calls.images.push(path); return `data:saved,${path}`; },
    searchLocalLibraryFolderRecords: async () => [],
    generatePreviewImage: async (path, mode) => { calls.generated.push(path); calls.generatedModes.push(mode); return { preview_path: `${path}.png`, data_url: "data:generated" }; },
    setRecordPreview: async (state, path, previewPath) => ({ ...state, records: state.records.map((item) => item.path === path ? { ...item, previewPath } : item) }),
    reorderLibraryRecords: async (state, paths) => { calls.reordered.push(paths); library = { ...state, records: state.records.map((item) => ({ ...item, sort_index: paths.indexOf(item.path) })) }; return library; },
    activateProjectionRecord: async (state) => state,
    ...overrides,
  };
  const slots = []; const effects = []; let cursor = 0; let tree;
  const hooks = { ...React,
    useState(initial) { const i = cursor++; if (!(i in slots)) slots[i] = typeof initial === "function" ? initial() : initial; return [slots[i], (next) => { slots[i] = typeof next === "function" ? next(slots[i]) : next; }]; },
    useRef(initial) { return slots[cursor++] ??= { current: initial }; },
    useMemo(compute, deps) { const i = cursor++; if (!slots[i] || deps.some((dep, n) => dep !== slots[i].deps[n])) slots[i] = { deps, value: compute() }; return slots[i].value; },
    useEffect(callback, deps) { const i = cursor++; if (!slots[i] || deps.some((dep, n) => dep !== slots[i].deps[n])) { slots[i]?.cleanup?.(); slots[i] = { deps }; effects.push(() => { slots[i].cleanup = callback(); }); } },
  };
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${compiled}\n})`)((name) => {
    if (name === "react") return hooks;
    if (name === "radix-ui") return { ContextMenu: { Root: "context-root", Trigger: "context-trigger", Portal: "context-portal", Content: "context-content", Item: "context-item", Separator: "context-separator" } };
    if (name === "./categoryIcon") return { CategoryIcon: "category-icon", CategoryIconDialog: "category-icon-dialog" };
    if (name === "lucide-react") return new Proxy({}, { get: (_, key) => `icon-${String(key)}` });
    if (name.endsWith("facade")) return facade;
    if (name.endsWith(".css")) return {};
    if (name.endsWith("/ui/button")) return { Button: "button" };
    if (name.endsWith("/ui/input")) return { Input: "input" };
    if (name.endsWith("/ui/checkbox")) return { Checkbox: "checkbox" };
    if (name.endsWith("/ui/scroll-area")) return { ScrollArea: "scroll-area" };
    if (name.endsWith("/Dropdown")) return { Dropdown: "dropdown" };
    if (name.endsWith("/Dialog")) return { Dialog: "dialog" };
    if (name === "./sendDialog") return { SendProjectionDialog: "send-dialog" };
    if (name === "./function") return { ensureLitematicFileName: (value) => `${value}.litematic` };
    if (name.endsWith("/local_library_folders")) return { LocalLibraryFoldersDialog: "folders-dialog", openLocalLibraryFoldersWithWindowBehavior: async () => {} };
    if (name.endsWith("/reden_library")) return { RedenLibraryDialog: "online-dialog", openRedenLibraryWithWindowBehavior: async () => { calls.online++; } };
    if (name.endsWith("/events")) return { listenEvent: async () => () => {}, emitEvent: async (...args) => { calls.events.push(args); } };
    if (name.endsWith("/libraryEvents")) return { projectionLibraryImportedEvent: "imported", projectionLibraryStateChangedEvent: "changed" };
    throw new Error(`Unexpected dependency: ${name}`);
  }, module, module.exports);
  const props = { currentFile: "", setCurrentFile: (value) => { calls.selected.push(value); props.currentFile = value; }, setRoute() {} };
  const h = {
    calls, props, library: () => library,
    render() { cursor = 0; tree = module.exports.LibraryPage(props); while (effects.length) effects.shift()(); return tree; },
    nodes() { const list = []; function visit(value) { if (Array.isArray(value)) return value.forEach(visit); if (!React.isValidElement(value)) return; list.push(value); visit(value.props.children); visit(value.props.footer); } visit(tree); return list; },
    find(predicate) { return this.nodes().find(predicate); },
    button(text) { return this.find((node) => node.type === "button" && label(node.props.children) === text); },
    input(ariaLabel) { return this.find((node) => node.props["aria-label"] === ariaLabel); },
    cards() { return this.nodes().filter((node) => node.type?.name === "ProjectionCard"); },
    category(text) { return this.find((node) => node.props.className === "library-category" && React.Children.toArray(node.props.children).some((child) => child.type === "span" && label(child) === text)); },
    async settle() { await tick(); this.render(); await tick(); this.render(); },
  };
  h.render(); return h;
}

const dragEvent = () => ({ preventDefault() {}, stopPropagation() {}, dataTransfer: { setData() {} } });

test("投影库：默认仅读取可见页真实预览，翻页继续读取，不隐式生成图", async () => {
  const records = Array.from({ length: 23 }, (_, i) => record(i));
  records[0].previewPath = "saved/0.png";
  const h = createHarness(records); await h.settle();
  assert.equal(h.cards().length, 20);
  assert.equal(h.calls.previews.length, 20);
  assert.deepEqual(h.calls.images, ["saved/0.png"]);
  assert.equal(h.cards()[0].props.previewUrl, "data:saved,saved/0.png");
  assert.deepEqual(h.calls.generated, []);
  h.button("下一页").props.onClick(); await h.settle();
  assert.equal(h.cards().length, 3);
  assert.equal(h.calls.previews.length, 23);
  h.button("上一页").props.onClick(); await h.settle();
  assert.equal(h.calls.previews.length, 23);
});

test("投影库：搜索迁移分类、未分类及状态筛选，在线下载使用已有入口", async () => {
  const records = [record(0, ["红石"]), record(1, ["建筑"]), record(2)]; records[2].status = "missing";
  const h = createHarness(records); await h.settle();
  h.input("搜索投影").props.onChange({ target: { value: "红石" } }); await h.settle();
  assert.deepEqual(h.cards().map((card) => card.props.record.path), [records[0].path]);
  h.input("搜索投影").props.onChange({ target: { value: "" } }); h.render();
  h.category("未分类").props.onClick(); h.render();
  assert.deepEqual(h.cards().map((card) => card.props.record.path), [records[2].path]);
  const statusFilter = h.find((node) => node.type === "dropdown" && node.props.ariaLabel === "筛选文件状态");
  assert.ok(statusFilter);
  assert.ok(statusFilter.props.options.some((option) => option.value === "ok"));
  statusFilter.props.onChange("ok"); h.render(); assert.equal(h.cards().length, 0);
  await h.button("下载投影").props.onClick(); assert.equal(h.calls.online, 1);
});

test("投影库：批量移动分类与拖放只改归类，原文件/元数据和隐藏记录保留", async () => {
  const records = [record(0, ["原标签", "旧次标签"]), record(1), record(2, ["目标分类"])];
  const h = createHarness(records); await h.settle();
  h.cards()[0].props.onToggle(true); h.render(); h.cards()[1].props.onToggle(true); h.render();
  h.button("移动到分类…").props.onClick(); h.render();
  const target = h.library().categories.find((item) => item.name === "目标分类").id;
  h.find((node) => node.type === "dropdown" && node.props.ariaLabel === "目标分类").props.onChange(target); h.render();
  await h.button("移动").props.onClick(); await h.settle();
  assert.deepEqual(h.library().records[0].categoryIds, [target]); assert.deepEqual(h.library().records[1].categoryIds, [target]);
  assert.equal("tags" in h.library().records[0], false); assert.equal(h.library().folders[0].path, "C:/library");
  assert.equal(h.library().records[0].description, records[0].description); assert.equal(h.library().records[0].path, records[0].path);
  h.cards()[0].props.onDragStart(dragEvent()); h.render();
  h.category("未分类").props.onDrop(dragEvent()); await h.settle();
  assert.deepEqual(h.library().records[0].categoryIds, []); assert.deepEqual(h.library().records[1].categoryIds, []);
  assert.deepEqual(h.library().records[2].categoryIds, [target]); assert.ok(h.calls.events.some(([event]) => event === "changed"));
});

test("投影库：右键创建子分类、稳定展开树、父级筛选涵盖子分类，预览仍走真实服务", async () => {
  const h = createHarness([record(0)]); await h.settle();
  const create = h.find((node) => node.type === "context-item" && label(node.props.children) === "新建子分类");
  create.props.onSelect(); h.render();
  h.find((node) => node.props.id === "library-category-name").props.onChange({ target: { value: "木头库存" } }); h.render();
  await h.button("保存").props.onClick(); await h.settle();
  const child = h.library().categories.find((item) => item.name === "木头库存");
  assert.equal(child.parentId, "builtin:warehouse"); assert.equal(h.category("木头库存").props["aria-level"], 2);
  h.cards()[0].props.onToggle(true); h.render(); h.button("移动到分类…").props.onClick(); h.render();
  h.find((node) => node.type === "dropdown" && node.props.ariaLabel === "目标分类").props.onChange(child.id); h.render();
  await h.button("移动").props.onClick(); await h.settle(); h.category("仓库").props.onClick(); await h.settle();
  assert.equal(h.cards().length, 1); h.cards()[0].props.onInspect(); h.render();
  await h.button("生成预览图").props.onClick(); await h.settle(); assert.deepEqual(h.calls.generated, [record(0).path]); assert.deepEqual(h.calls.generatedModes, ["fast_experimental"]);
});

test("投影库：批量移除需要确认，只保存剩余记录，不调用文件删除", async () => {
  const records = [record(0), record(1), record(2)]; const h = createHarness(records); await h.settle();
  h.cards()[0].props.onToggle(true); h.render(); h.cards()[1].props.onToggle(true); h.render();
  h.button("从列表移除").props.onClick(); h.render();
  assert.equal(h.calls.saved.length, 0);
  const dialog = h.find((node) => node.type === "dialog");
  assert.match(dialog.props.subtitle, /不会删除/);
  const remove = React.Children.toArray(dialog.props.footer.props.children).find((node) => node.props.variant === "destructive");
  await remove.props.onClick(); await h.settle();
  assert.deepEqual(h.library().records.map((entry) => entry.path), [records[2].path]);
  assert.equal(h.calls.saved.length, 1);
});

test("投影库：筛选内拖动排序保留隐藏记录及相对顺序", async () => {
  const records = [record(0), record(1, ["可见"]), record(2), record(3, ["可见"]), record(4)];
  const h = createHarness(records); await h.settle();
  h.category("可见").props.onClick(); h.render();
  h.cards()[1].props.onDragStart(dragEvent()); h.render();
  h.cards()[0].props.onDrop(dragEvent()); await h.settle();
  assert.deepEqual(h.calls.reordered[0], [records[0].path, records[3].path, records[1].path, records[2].path, records[4].path]);
  assert.equal(h.library().records.length, 5);
});
