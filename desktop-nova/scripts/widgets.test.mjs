import assert from 'node:assert/strict';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { posix } from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import test from 'node:test';

const require = createRequire(import.meta.url);
const React = require('react');
const { renderToStaticMarkup } = require('react-dom/server');
const ts = require('typescript');
const root = new URL('../', import.meta.url);
const source = (path) => readFileSync(new URL(path, root), 'utf8');

function loadComponent(path, react = React, dependencies = {}, cache = new Map()) {
  if (cache.has(path)) return cache.get(path);
  const compiled = ts.transpileModule(source(path).replaceAll('import.meta', '__import_meta'), {
    fileName: path,
    reportDiagnostics: true,
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React, esModuleInterop: true },
  });
  assert.deepEqual(compiled.diagnostics?.filter((entry) => entry.category === ts.DiagnosticCategory.Error), []);
  const module = { exports: {} };
  const run = vm.runInThisContext(`(function(require,module,exports){${compiled.outputText}\n})`, {
    filename: fileURLToPath(new URL(path, root)),
  });
  cache.set(path, module.exports);
  // `import.meta.glob` is a Vite build-time feature; the stub returns an empty asset
  // map so component modules load without a bundler (themes simply fall back to lucide).
  const previousGlob = globalThis.__import_meta;
  globalThis.__import_meta = { glob: () => ({}) };
  try {
    run((name) => {
      if (name === 'react') return react;
      if (name in dependencies) return dependencies[name];
      if (name.startsWith('.') || name.startsWith('@/')) {
        const relative = name.startsWith('@/') ? `ui/${name.slice(2)}` : posix.join(posix.dirname(path), name);
        if (relative.endsWith('.css')) return {};
        const target = [relative, `${relative}.tsx`, `${relative}.ts`, `${relative}/index.tsx`, `${relative}/index.ts`].find((candidate) => {
          try { return statSync(new URL(candidate, root)).isFile(); } catch { return false; }
        });
        assert.ok(target, `Missing local test dependency: ${relative}`);
        return loadComponent(target, react, dependencies, cache);
      }
      return require(name);
    }, module, module.exports);
  } finally {
    globalThis.__import_meta = previousGlob;
  }
  return module.exports;
}

function elements(tree) {
  if (Array.isArray(tree)) return tree.flatMap(elements);
  if (!React.isValidElement(tree)) return [];
  return [tree, ...elements(tree.props.children)];
}

// Runs the actual component event handlers without adding a browser/test framework dependency.
// Radix focus/keyboard behavior and the three theme layouts are left for manual acceptance.
function componentHarness(path, name, props, dependencies = {}, initialSlots = []) {
  const slots = [...initialSlots];
  let cursor = 0;
  const hooks = {
    ...React,
    useState(initial) {
      const index = cursor++;
      if (!(index in slots)) slots[index] = typeof initial === 'function' ? initial() : initial;
      return [slots[index], (next) => { slots[index] = typeof next === 'function' ? next(slots[index]) : next; }];
    },
    useRef(initial) {
      const index = cursor++;
      return slots[index] ??= { current: initial };
    },
    useId: () => `dropdown-${cursor++}`,
    useEffect: () => {},
    useMemo: (compute) => compute(),
    useSyncExternalStore: (_subscribe, getSnapshot) => getSnapshot(),
  };
  const component = loadComponent(path, hooks, dependencies)[name];
  return () => { cursor = 0; return component(props); };
}

const options = [
  { value: 'a', label: '第一项' }, { value: 'b', label: '第二项' }, { value: 'c', label: '第三项' },
];

test('方块搜索弹层脱离页面滚动容器，保留搜索与键盘提交', () => {
  const { Popover } = require('radix-ui');
  const changes = [];
  const render = componentHarness('ui/components/BlockSearchSelect.tsx', 'BlockSearchSelect', {
    value: 'stone', onChange: (value) => changes.push(value),
  }, {
    './BlockIcon': { BlockIcon: () => null },
    '../../src/business/facade': {
      getAllBlocks: () => ['minecraft:stone', 'minecraft:redstone_wire'],
      translateBlockId: (id) => id === 'minecraft:redstone_wire' ? '红石粉' : '石头',
    },
  });
  let tree = elements(render());
  const root = tree.find((element) => element.type === Popover.Root);
  root.props.onOpenChange(true);
  tree = elements(render());
  assert.equal(tree.find((element) => element.type === Popover.Root).props.open, true);
  const portal = tree.find((element) => element.type === Popover.Portal);
  const content = elements(portal.props.children).find((element) => element.type === Popover.Content);
  assert.equal(content.props.collisionPadding, 8);
  assert.equal(content.props.align, 'start');
  const input = tree.find((element) => element.props['aria-label'] === '搜索方块');
  assert.equal(input.props.onBlur, undefined);
  input.props.onChange({ target: { value: '红石' } });
  tree = elements(render());
  assert.equal(tree.filter((element) => element.props.className?.startsWith('block-search-item')).length, 1);
  let prevented = false;
  tree.find((element) => element.props['aria-label'] === '搜索方块').props.onKeyDown({ key: 'Enter', preventDefault() { prevented = true; } });
  assert.equal(prevented, true);
  assert.deepEqual(changes, ['minecraft:redstone_wire']);
  tree = elements(render());
  assert.equal(tree.find((element) => element.type === Popover.Root).props.open, false);
  assert.equal(tree.find((element) => element.props['aria-label'] === '搜索方块').props.value, '');
  assert.match(source('ui/styles/widgets.css'), /--radix-popover-content-available-height/);
});

test('下拉框由官方 Radix Select 处理交互，空值和带前缀值可无损往返', () => {
  const changes = [];
  const entries = [{ value: '', label: '全部' }, { value: 'value:', label: '合法前缀值' }, ...options];
  const render = componentHarness('ui/components/Dropdown.tsx', 'Dropdown', {
    value: '', options: entries, onChange: (value) => changes.push(value), ariaLabel: '材料范围',
  });
  const tree = elements(render());
  const select = tree.find((element) => element.props.onValueChange);
  assert.equal(select.props.value, 'value:');
  const content = tree.find((element) => element.props.position === 'popper');
  assert.equal(content.props.collisionPadding, 8);
  const items = elements(content.props.children);
  assert.ok(items.every((element) => element.props.value !== ''));
  for (const item of items) select.props.onValueChange(item.props.value);
  assert.deepEqual(changes, entries.map((entry) => entry.value));
  assert.match(source('ui/components/ui/select.tsx'), /Select as SelectPrimitive.*radix-ui/);
  assert.doesNotMatch(source('ui/components/Dropdown.tsx'), /onKeyDown|activeIndex|scrollIntoView/);
});

test('下拉框保留中文可访问名称和禁用状态', () => {
  const { Dropdown } = loadComponent('ui/components/Dropdown.tsx');
  const html = renderToStaticMarkup(React.createElement(Dropdown, { value: 'b', options, onChange() {}, disabled: true, ariaLabel: '材料范围' }));
  assert.match(html, /role="combobox"/);
  assert.match(html, /aria-label="材料范围"/);
  assert.match(html, /aria-expanded="false"/);
  assert.match(html, /disabled=""/);
  assert.match(html, /第二项/);
  const empty = renderToStaticMarkup(React.createElement(Dropdown, { value: '', options: [], onChange() {} }));
  assert.match(empty, /disabled=""/);
  assert.match(empty, /请选择/);
});

test('弹窗保留标题、说明、忙碌关闭保护和原页面接口', () => {
  let closed = 0;
  const render = componentHarness('ui/components/Dialog.tsx', 'Dialog', {
    title: '复制投影', subtitle: '原文件保持不变', busy: true, onClose() { closed++; },
    children: React.createElement('input', { 'aria-label': '文件名' }),
  });
  const tree = elements(render());
  const root = tree.find((element) => element.props.onOpenChange);
  root.props.onOpenChange(false);
  assert.equal(closed, 0);
  const content = tree.find((element) => element.props.onEscapeKeyDown);
  assert.equal(content.props['aria-busy'], true);
  // A real description uses Radix's shared context ID, not a wrapper override.
  assert.equal(Object.hasOwn(content.props, 'aria-describedby'), false);
  const description = tree.find((element) => element.props.children === '原文件保持不变');
  assert.ok(description);
  assert.equal(Object.hasOwn(description.props, 'id'), false);
  const event = { prevented: false, preventDefault() { this.prevented = true; } };
  content.props.onEscapeKeyDown(event);
  assert.equal(event.prevented, true);
  event.prevented = false;
  content.props.onInteractOutside(event);
  assert.equal(event.prevented, true);
  assert.equal(tree.find((element) => element.props['aria-label'] === '关闭对话框').props.disabled, true);
  assert.match(source('ui/components/ui/dialog.tsx'), /Dialog as DialogPrimitive.*radix-ui/);
  assert.match(source('ui/components/ui/dialog.tsx'), /<DialogPortal>/);
  assert.doesNotMatch(source('ui/components/Dialog.tsx'), /document.addEventListener/);
  const idle = componentHarness('ui/components/Dialog.tsx', 'Dialog', { title: '删除', onClose() { closed++; } });
  const noDescription = elements(idle()).find((element) => element.props.onEscapeKeyDown);
  assert.equal(Object.hasOwn(noDescription.props, 'aria-describedby'), true);
  assert.equal(noDescription.props['aria-describedby'], undefined);
  elements(idle()).find((element) => element.props.onOpenChange).props.onOpenChange(false);
  assert.equal(closed, 1);
});

test('四主题共用官方控件，Minecraft/Metro10 资源与控件身份保留', () => {
  const settings = source('ui/windows/main/pages/settings/SettingsPage.tsx');
  // The picker is generated from the theme manifest; the list itself lives there.
  assert.match(settings, /const NOVA_THEME_OPTIONS = THEMES\.map\(/);
  const manifest = source('src/services/themeManifest.ts');
  const themeList = manifest.match(/export const THEMES = \[([\s\S]*?)\] as const;/)?.[1] ?? '';
  assert.ok(themeList, 'THEMES array not found in themeManifest.ts');
  for (const theme of ['shadcn', '8bitcn', 'Minecraft', 'Metro10', 'aero7']) assert.ok(themeList.includes(`id: "${theme}"`), theme);
  assert.doesNotMatch(themeList, /WebDefault|Bootstrap5|QTDefault/);
  assert.match(source('ui/themes/minecraft/widgets.css'), /border-image-source: var\(--ln-minecraft-button-image\)/);
  assert.match(source('ui/themes/minecraft/widgets.css'), /unifont-16\.0\.04\.ttf/);
  assert.match(source('ui/themes/metro10/widgets.css'), /\[data-slot="checkbox"\]/);
  assert.match(source('ui/themes/metro10/widgets.css'), /checkbox_default\.svg/);
  // SVG nav artwork is served through <img>, so `currentColor` cannot inherit;
  // dark mode has to invert the monochrome set to match the label colour.
  assert.match(source('ui/themes/metro10/widgets.css'), /:root\[data-color-scheme="dark"\] \.theme-metro10 img\.nav-icon/);
  assert.match(source('ui/components/ui/button.tsx'), /button-decorations/);
  assert.match(source('ui/components/ui/button.tsx'), /Slot.Slottable/);
  assert.match(source('ui/shell/themeRuntime.ts'), /subscribeToThemeChanges/);
  for (const path of ['button', 'input', 'table', 'select', 'dialog']) {
    assert.doesNotMatch(source(`ui/components/ui/${path}.tsx`), /useTheme|themeId|data-theme=/);
  }
});

test('官方按钮保留单个交互节点，像素边框不进入可访问内容', () => {
  const { Button } = loadComponent('ui/components/ui/button.tsx');
  const html = renderToStaticMarkup(React.createElement(Button, { disabled: true }, '分析投影'));
  assert.equal((html.match(/<button\b/g) || []).length, 1);
  assert.match(html, /type="button"/);
  assert.match(html, /disabled=""/);
  assert.match(html, /aria-hidden="true" class="nova-pixel-decoration/);
  const link = renderToStaticMarkup(React.createElement(Button, { asChild: true }, React.createElement('a', { href: '#help' }, '帮助')));
  assert.equal((link.match(/<a\b/g) || []).length, 1);
  assert.doesNotMatch(link, /<button\b/);
  assert.match(link, /href="#help"/);
});

test('输入与材料表保留原生语义，不添加破坏布局的包装', () => {
  const { Input } = loadComponent('ui/components/ui/input.tsx');
  const html = renderToStaticMarkup(React.createElement(Input, { 'aria-label': '文件名', defaultValue: '投影', readOnly: true }));
  assert.match(html, /^<input\b/);
  assert.match(html, /aria-label="文件名"/);
  assert.match(html, /readonly=""/);
  assert.match(html, /value="投影"/);
  const { Table, TableHeader, TableRow, TableHead, TableBody, TableCell } = loadComponent('ui/components/ui/table.tsx');
  const table = renderToStaticMarkup(React.createElement(Table, null,
    React.createElement(TableHeader, null, React.createElement(TableRow, null, React.createElement(TableHead, { scope: 'col' }, '材料'))),
    React.createElement(TableBody, null, React.createElement(TableRow, null, React.createElement(TableCell, null, '石头'))),
  ));
  assert.match(table, /^<table\b/);
  assert.match(table, /<thead\b/);
  assert.match(table, /scope="col"/);
  assert.match(table, /<tbody\b/);
  assert.doesNotMatch(table, /<div\b/);
});

test('短窗口只压缩滚动主导航，标题和底部选项保留空间', () => {
  const navigation = source('ui/shell/widget/nav.css');
  const controls = source('ui/styles/controls.css');
  // The scrollable main navigation now lives inside a ScrollArea; that wrapper owns
  // the shrink/scroll behaviour, and the pinned rows must not be compressed.
  const scrollArea = controls.match(/\.sidebar > \.nav-scroll-area\s*\{([^}]+)\}/)?.[1];
  assert.ok(scrollArea, '`.sidebar > .nav-scroll-area` contract should exist');
  assert.match(scrollArea, /flex:\s*1;/);
  assert.match(scrollArea, /min-height:\s*0;/);
  const pinnedNavigation = navigation.match(/\.sidebar > \.nav-expand,\s*\.sidebar > \.nav-list-bottom\s*\{([^}]+)\}/)?.[1];
  assert.ok(pinnedNavigation);
  assert.match(pinnedNavigation, /flex:\s*0 0 auto;/);
});

// --- library page harness (F.1 companion) --------------------------------------
// LibraryPage is an orchestration container (D.3): its child components are stubbed
// to string types so the composed props contract is observable without a renderer,
// while the real store / entry-view modules run over an in-memory disk.
const libModuleCache = new Map();
function libLoad(path, dependencies, fresh = false) {
  if (!fresh && libModuleCache.has(path)) return libModuleCache.get(path);
  const code = ts.transpileModule(source(path), { fileName: path, compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${code}})`)(dependencies, module, module.exports);
  if (!fresh) libModuleCache.set(path, module.exports);
  return module.exports;
}
const libPaths = libLoad('src/services/filePathIdentity.ts', () => { throw new Error('unexpected path import'); });
const libIcons = libLoad('src/services/libraryCategoryIcons.ts', (name) => name === './i18n' ? { t: (key) => key } : (() => { throw new Error('unexpected icon import'); })());
const libMigration = libLoad('src/services/libraryMigration.ts', (name) => name === './filePathIdentity' ? libPaths : name === './libraryCategoryIcons' ? libIcons : (() => { throw new Error(`migration dep ${name}`); })());
const libContent = libLoad('src/services/libraryContent.ts', (name) => name === './filePathIdentity' ? libPaths : name === './libraryStore' ? {} : (() => { throw new Error(`content dep ${name}`); })());

const libEntryFixture = (id, over = {}) => ({
  id, createdAt: 0, checksum: null, status: 'active', categoryIds: [], displayName: `${id}.litematic`, internalName: undefined,
  author: '作者', description: '说明', tags: [], preview: {},
  stats: { totalBlocks: 100, totalVolume: 300, regionCount: 1, enclosingSize: { x: 2, y: 3, z: 2 }, minecraftDataVersion: 5000 },
  sortIndex: 0, lastAnalyzedAt: 0, lastError: '', ...over,
});
const libReplicaFixture = (id, entryId, over = {}) => ({
  id, entryId, path: `C:/library/${id}.litematic`, fileName: `${id}.litematic`, folderPath: 'C:/library',
  checksum: null, fileSize: 2048, mtime: 1, status: 'active', lastAnalyzedAt: 0, lastError: '', ...over,
});
const libStateFixture = (entries, replicas) => ({
  schemaVersion: 5, entries, replicas,
  folders: [{ path: 'C:/library', type: 'tracked', enabled: true, recursive: true, sort_index: 0, lastSyncedAt: 0, lastScanCount: 0, lastError: '' }],
  categories: [], trash: [], redirects: [], favorites: [], recent: [],
});

function libraryHarness(overrides = {}) {
  const disk = { text: JSON.stringify(overrides.state) };
  const saved = [];
  const selected = [];
  const backend = {
    readUserConfigFile: async () => disk.text,
    writeUserConfigFile: async (_path, next) => { disk.text = next; saved.push(JSON.parse(next)); },
    checkFileExists: async () => true,
    listLitematicFileEntriesInDirectory: async () => [],
    copyFileToDirectory: async () => ({}),
    computeLitematicChecksum: async () => ({ sha256: 'x', byte_count: 1 }),
    getUserConfigFilePath: async () => 'C:/builtin/import',
    libraryTrashMove: async (items) => items.map((item) => ({ sourcePath: item.sourcePath, trashPath: `C:/trash/${item.trashName}`, ok: true })),
    libraryTrashRestore: async (items) => items.map((item) => ({ trashPath: item.trashPath, targetPath: item.targetPath, ok: true })),
    libraryTrashPurge: async (items) => items.map((item) => ({ trashPath: item.trashPath, ok: true })),
    libraryExportZip: async () => ({ taskId: 'zip' }),
    ...(overrides.backend || {}),
  };
  const store = libLoad('src/services/libraryStore.ts', (name) => {
    if (name === './libraryCategoryIcons') return libIcons;
    if (name === './filePathIdentity') return libPaths;
    if (name === './backend') return backend;
    if (name === '../platform/applicationOperations') return { inspectProject: async () => ({ metadata: {}, regions: [] }), editProjectMetadata: async () => ({}) };
    if (name === './libraryMigration') return libMigration;
    if (name === './libraryContent') return libContent;
    if (name === './libraryTasks') return { submitLibraryTask: async () => 't', cancelLibraryTask: async () => true, subscribeLibraryTask: async () => () => {}, submitAndAwait: async () => ({ taskId: 't', kind: 'x', status: 'completed', summary: {}, failures: [] }), awaitLibraryTask: async () => ({ taskId: 't', kind: 'x', status: 'completed', summary: {}, failures: [] }), ...(overrides.tasks || {}) };
    if (name === './statsService') return { formatMaterialQuantity: () => '', loadStructureStats: async () => ({ materials: [] }) };
    if (name === './minecraftVersions') return { minecraftVersionLabel: (value) => `v${value}` };
    if (name === './libraryIndexClient') return { queryIndex: async (query) => ({ entryIds: overrides.indexMatches ? overrides.indexMatches(query) : [], ready: true }), submitIndexBuild: async () => 'idx', subscribeIndexStatus: async () => () => {} };
    throw new Error(`store dep ${name}`);
  }, true);
  const facade = {
    ...store,
    windowsPathKey: libPaths.windowsPathKey,
    displayPathText: libPaths.displayPathText,
    loadUserConfigMigratingLocalStorage: async () => ({ config: {} }),
    generatePreviewImage: async (path) => ({ preview_path: `${path}.png`, data_url: 'data:generated' }),
    readProjectionPreviewImage: async (path) => ({ data_url: `data:preview,${path}` }),
    getPathInfo: async (path) => ({ path }),
    openFileParentDir: async () => {},
    selectLitematicFile: async () => null,
    ...(overrides.facade || {}),
  };
  const entryView = libLoad('ui/windows/main/pages/library/libraryEntryView.ts', (name) => {
    if (name.endsWith('src/business/facade')) return facade;
    if (name.endsWith('src/services/minecraftVersions')) return { minecraftVersionLabel: (value) => `v${value}` };
    if (name === './library.css') return {};
    throw new Error(`entryView dep ${name}`);
  }, true);
  const stubs = {
    './LibraryToolbar': { LibraryToolbar: 'LibraryToolbar' },
    './LibraryCategoryTree': { LibraryCategoryTree: 'LibraryCategoryTree' },
    './LibraryGrid': { LibraryGrid: 'LibraryGrid' },
    './LibraryList': { LibraryList: 'LibraryList' },
    './LibraryDetail': { LibraryDetail: 'LibraryDetail' },
    './LibraryGrouping': { LibraryGrouping: 'LibraryGrouping' },
    './LibraryColumnPicker': { LibraryColumnPicker: 'LibraryColumnPicker' },
    './LibraryTaskCenter': { LibraryTaskCenter: 'LibraryTaskCenter' },
    './sendDialog': { SendProjectionDialog: 'SendProjectionDialog' },
    './DropImportDialog': { DropImportDialog: 'DropImportDialog' },
    './ExportDialog': { ExportDialog: 'ExportDialog' },
    './ReplicaNameEditor': { ReplicaNameEditor: 'ReplicaNameEditor' },
    './categoryIcon': { CategoryIconDialog: 'CategoryIconDialog' },
    '../../../local_library_folders': { LocalLibraryFoldersDialog: 'LocalLibraryFoldersDialog', openLocalLibraryFoldersWithWindowBehavior: async () => {} },
    '../../../local_library_folders/TrashPanel': { TrashPanel: 'TrashPanel' },
    '../../../reden_library': { RedenLibraryDialog: 'RedenLibraryDialog', openRedenLibraryWithWindowBehavior: async () => {} },
    '../../../../components/Dialog': { Dialog: 'dialog' },
    '../../../../components/Dropdown': { Dropdown: 'dropdown' },
    '../../../../components/ThemeIcon': { ThemeIcon: 'theme-icon' },
    '../../../../components/ui/button': { Button: 'button' },
    '../../../../components/ui/input': { Input: 'input' },
    '../../../../components/ui/checkbox': { Checkbox: 'checkbox' },
    '../../../../components/ui/scroll-area': { ScrollArea: 'scroll-area' },
    './libraryEntryView': entryView,
    './function': { ensureLitematicFileName: (value) => value },
    './library.css': {},
    '../../../../../src/services/libraryTasks': { submitAndAwait: async () => ({ taskId: 't', kind: 'x', status: 'completed', summary: {}, failures: [] }) },
    '../../../../../src/business/facade': facade,
    '../../../../../src/platform/events': { listenEvent: async () => () => {}, emitEvent: async () => {} },
    '../../../libraryEvents': { projectionLibraryImportedEvent: 'imported', projectionLibraryStateChangedEvent: 'changed' },
  };
  const slots = []; const effects = []; let cursor = 0; let tree;
  const hooks = { ...React,
    useState(initial) { const index = cursor++; if (!(index in slots)) slots[index] = typeof initial === 'function' ? initial() : initial; return [slots[index], (next) => { slots[index] = typeof next === 'function' ? next(slots[index]) : next; }]; },
    useRef(initial) { const index = cursor++; return slots[index] ??= { current: initial }; },
    useCallback(fn) { return fn; },
    useMemo(compute, deps) { const index = cursor++; if (!slots[index] || !deps || deps.some((dep, offset) => dep !== slots[index].deps?.[offset])) slots[index] = { deps, value: compute() }; return slots[index].value; },
    useEffect(callback, deps) { const index = cursor++; if (!slots[index] || !deps || deps.some((dep, offset) => dep !== slots[index].deps?.[offset])) { slots[index]?.cleanup?.(); slots[index] = { deps }; effects.push(() => { slots[index].cleanup = callback(); }); } },
  };
  const compiled = ts.transpileModule(source('ui/windows/main/pages/library/LibraryPage.tsx'), { fileName: 'LibraryPage.tsx', compilerOptions: { jsx: ts.JsxEmit.React, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText;
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${compiled}\n})`)((name) => {
    if (name === 'react') return hooks;
    if (name === 'lucide-react') return new Proxy({}, { get: (_, key) => `icon-${String(key)}` });
    if (name in stubs) return stubs[name];
    throw new Error(`Unexpected dependency: ${name}`);
  }, module, module.exports);
  const props = { currentFile: '', setCurrentFile: (value) => { selected.push(value); props.currentFile = value; }, setRoute() {} };
  const text = (value) => value == null || typeof value === 'boolean' ? '' : Array.isArray(value) ? value.map(text).join('') : typeof value === 'object' ? text(value.props?.children) : String(value);
  const nodes = () => { const list = []; const visit = (value) => { if (Array.isArray(value)) return value.forEach(visit); if (!React.isValidElement(value)) return; list.push(value); visit(value.props.children); visit(value.props.footer); }; visit(tree); return list; };
  const render = () => { cursor = 0; tree = module.exports.LibraryPage(props); while (effects.length) effects.shift()(); return tree; };
  render();
  return {
    render, props, saved, selected, text,
    nodes, find: (predicate) => nodes().find(predicate),
    component: (type) => nodes().find((node) => node.type === type),
    components: (type) => nodes().filter((node) => node.type === type),
    button: (label) => nodes().find((node) => node.type === 'button' && text(node.props.children) === label),
    async settle(rounds = 3) { for (let i = 0; i < rounds; i += 1) { await new Promise((resolve) => setImmediate(resolve)); render(); } },
    async search(value) { this.component('LibraryToolbar').props.onSearchChange(value); render(); await new Promise((resolve) => setTimeout(resolve, 260)); render(); },
  };
}

test('投影库空白点击保留当前文件，筛选可直接清除', async () => {
  const library = libraryHarness({ state: libStateFixture([libEntryFixture('e1')], [libReplicaFixture('r1', 'e1')]), indexMatches: () => [] });
  await library.settle();
  assert.equal(library.render().props.onClick, undefined);
  await library.search('不存在');
  assert.ok(library.button('清除筛选'), '搜索无结果时应提供清除筛选');
  library.button('清除筛选').props.onClick();
  library.render();
  assert.equal(library.component('LibraryToolbar').props.search, '');
  assert.deepEqual(library.selected, []);
});

test('投影库刷新失败恢复按钮，不覆盖原列表', async () => {
  const library = libraryHarness({ state: libStateFixture([libEntryFixture('e1')], [libReplicaFixture('r1', 'e1')]), tasks: { submitAndAwait: async () => { throw new Error('磁盘不可用'); } } });
  await library.settle();
  await library.component('LibraryToolbar').props.onRefresh();
  await library.settle();
  assert.equal(library.component('LibraryToolbar').props.isRefreshing, false);
  const alert = library.find((element) => element.props.role === 'alert');
  assert.ok(alert);
  assert.match(library.text(alert.props.children), /原有列表已保留/);
  assert.equal(library.component('LibraryGrid').props.entries.length, 1);
});

test('移除投影需要确认，只保存库记录而不删除文件', async () => {
  const library = libraryHarness({ state: libStateFixture([libEntryFixture('e1')], [libReplicaFixture('r1', 'e1')]) });
  await library.settle();
  const savesBefore = library.saved.length;
  const grid = library.component('LibraryGrid');
  grid.props.binding.onToggle(grid.props.entries[0], true);
  library.render();
  assert.equal(library.saved.length, savesBefore, '仅选择不写盘');
  library.button('移入回收区').props.onClick();
  library.render();
  const dialog = library.component('dialog');
  assert.ok(dialog);
  assert.match(dialog.props.title, /移入回收区/);
  assert.match(dialog.props.subtitle, /可随时还原|不会被物理删除/);
  assert.equal(library.saved.length, savesBefore, '确认前不写盘');
  const confirm = library.find((element) => element.type === 'button' && element.props.variant === 'destructive');
  await confirm.props.onClick();
  await library.settle();
  assert.equal(library.saved.length, savesBefore + 1);
  const latest = library.saved[library.saved.length - 1];
  assert.equal(latest.trash.length, 1, '记录进入回收区');
  assert.equal(latest.replicas.every((replica) => replica.status === 'trashed'), true, '不物理删除文件（仅标记副本）');
});
