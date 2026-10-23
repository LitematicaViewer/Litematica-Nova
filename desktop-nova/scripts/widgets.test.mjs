import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
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
  const compiled = ts.transpileModule(source(path), {
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
  run((name) => {
    if (name === 'react') return react;
    if (name in dependencies) return dependencies[name];
    if (name.startsWith('.') || name.startsWith('@/')) {
      const relative = name.startsWith('@/') ? `ui/${name.slice(2)}` : posix.join(posix.dirname(path), name);
      if (relative.endsWith('.css')) return {};
      const target = [relative, `${relative}.tsx`, `${relative}.ts`].find((candidate) => existsSync(new URL(candidate, root)));
      assert.ok(target, `Missing local test dependency: ${relative}`);
      return loadComponent(target, react, dependencies, cache);
    }
    return require(name);
  }, module, module.exports);
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
  const css = source('ui/shell/widget/nav.css');
  const mainNavigation = css.match(/\.sidebar > \.nav-list:not\(\.nav-list-bottom\)\s*\{([^}]+)\}/)?.[1];
  assert.ok(mainNavigation);
  assert.match(mainNavigation, /min-height:\s*0;/);
  assert.match(mainNavigation, /flex:\s*0 1 auto;/);
  assert.match(mainNavigation, /overflow-y:\s*auto;/);
  const pinnedNavigation = css.match(/\.sidebar > \.nav-expand,\s*\.sidebar > \.nav-list-bottom\s*\{([^}]+)\}/)?.[1];
  assert.ok(pinnedNavigation);
  assert.match(pinnedNavigation, /flex:\s*0 0 auto;/);
});

function libraryHarness(overrides = {}) {
  const record = { path: 'C:/projection.litematic', displayName: '示例投影', fileName: 'projection.litematic', tags: [], status: 'ok', fileSize: 2048, lastAnalyzedAt: 1, totalBlocks: 10, totalVolume: 12, regionCount: 1, enclosingSize: { x: 2, y: 3, z: 2 }, author: '', description: '' };
  const state = { records: [record], folders: [] };
  const saved = [];
  const selected = [];
  const services = {
    loadLibrary: async () => state,
    saveLibrary: async (next) => { saved.push(next); },
    checkFileExists: async () => true,
    analyzeFile: async () => { throw new Error('读取失败'); },
    ...overrides,
  };
  const render = componentHarness('ui/windows/main/pages/library/LibraryPage.tsx', 'LibraryPage', {
    currentFile: record.path, setCurrentFile: (path) => selected.push(path), setRoute() {},
  }, {
    '../../../../../src/business/facade': services,
    './sendDialog': { SendProjectionDialog: () => null },
    '../../../../components/Dialog': { Dialog: () => null },
    '../../../../components/ui/button': { Button: 'button' },
    '../../../../components/ui/input': { Input: 'input' },
    '../../../../components/ui/checkbox': { Checkbox: 'checkbox' },
    '../../../../components/ui/scroll-area': { ScrollArea: 'scroll-area' },
    './function': { ensureLitematicFileName: (value) => value },
    './library.css': {},
    '../../../local_library_folders': {},
    '../../../reden_library': {},
    '../../../../../src/platform/events': { emitEvent: async () => {} },
    '../../../libraryEvents': {},
  }, [state]);
  const contentText = (value) => Array.isArray(value) ? value.map(contentText).join('') : value && typeof value === 'object' ? contentText(value.props?.children) : value == null || typeof value === 'boolean' ? '' : String(value);
  const button = (label) => elements(render()).find((element) => element.type === 'button' && contentText(element.props.children) === label);
  return { render, button, contentText, record, saved, selected };
}

test('投影库空白点击保留当前文件，筛选可直接清除', () => {
  const library = libraryHarness();
  assert.equal(library.render().props.onClick, undefined);
  const search = elements(library.render()).find((element) => element.props['aria-label'] === '搜索投影');
  search.props.onChange({ target: { value: '不存在' } });
  assert.ok(library.button('清除筛选'));
  library.button('清除筛选').props.onClick();
  const cleared = elements(library.render()).find((element) => element.props['aria-label'] === '搜索投影');
  assert.equal(cleared.props.value, '');
  assert.deepEqual(library.selected, []);
});

test('投影库刷新失败恢复按钮，不覆盖原列表', async () => {
  const library = libraryHarness();
  const refresh = () => elements(library.render()).find((element) => element.props['aria-label'] === '刷新投影库');
  await refresh().props.onClick();
  assert.equal(refresh().props.disabled, false);
  assert.equal(library.saved.length, 0);
  assert.ok(elements(library.render()).some((element) => element.props.role === 'alert' && library.contentText(element.props.children).includes('原有列表已保留')));
  assert.ok(elements(library.render()).some((element) => element.props.record === library.record));
});

test('移除投影需要确认，只保存库记录而不删除文件', async () => {
  const library = libraryHarness();
  const card = elements(library.render()).find((element) => element.props.record === library.record);
  // The card checkbox exposes batch actions; merely selecting never removes a record.
  card.props.onToggle(true);
  assert.equal(library.saved.length, 0);
  library.button('从列表移除').props.onClick();
  const dialog = elements(library.render()).find((element) => element.props.title === '从投影库移除此投影？');
  assert.match(dialog.props.subtitle, /不会删除/);
  assert.equal(library.saved.length, 0);
  const confirm = elements(dialog.props.footer).find((element) => element.props.children === '从列表移除');
  await confirm.props.onClick();
  assert.equal(library.saved.length, 1);
  assert.deepEqual(library.saved[0].records, []);
  assert.deepEqual(library.selected, ['']);
});
