import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { posix } from 'node:path';
import test from 'node:test';
import vm from 'node:vm';
import { JSDOM } from 'jsdom';

// CLI DOM interaction tests, not browser/GUI or visual acceptance.
const dom = new JSDOM('<!doctype html><html><body></body></html>', { url: 'http://localhost' });
for (const name of ['window', 'document', 'HTMLElement', 'HTMLInputElement', 'Element', 'Node', 'Event', 'KeyboardEvent', 'MouseEvent', 'CustomEvent', 'MutationObserver']) globalThis[name] = dom.window[name];
globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const React = require('react');
const { act } = React;
const { createRoot } = require('react-dom/client');
const ts = require('typescript');
const root = new URL('../', import.meta.url);
const source = path => readFileSync(new URL(path, root), 'utf8');
const cache = new Map();
function load(path) {
  if (cache.has(path)) return cache.get(path);
  const compiled = ts.transpileModule(source(path), { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.React, esModuleInterop: true } });
  const module = { exports: {} };
  vm.runInThisContext(`(function(require,module,exports){${compiled.outputText}\n})`)((name) => {
    if (!name.startsWith('.')) return require(name);
    const relative = posix.join(posix.dirname(path), name);
    return load([relative, `${relative}.tsx`, `${relative}.ts`].find(candidate => existsSync(new URL(candidate, root))));
  }, module, module.exports);
  cache.set(path, module.exports);
  return module.exports;
}
const { Checkbox } = load('ui/components/ui/checkbox.tsx');
const { RadioGroup, RadioGroupItem } = load('ui/components/ui/radio-group.tsx');
const h = React.createElement;
async function mount(element, run) {
  const host = document.createElement('div');
  document.body.append(host);
  const renderer = createRoot(host);
  const render = async element => { await act(async () => renderer.render(element)); };
  try { await render(element); await run(host, render); }
  finally { await act(async () => renderer.unmount()); host.remove(); }
}
async function click(node) { await act(async () => node.click()); }
async function key(node, value) {
  const event = new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true });
  await act(async () => { node.dispatchEvent(event); await new Promise(resolve => setTimeout(resolve, 5)); });
  await act(async () => node.dispatchEvent(new KeyboardEvent('keyup', { key: value, bubbles: true })));
  return event;
}
function group(props = {}) {
  return h(RadioGroup, { 'aria-label': '模式', ...props },
    h(RadioGroupItem, { value: 'a', 'aria-label': 'A' }),
    h(RadioGroupItem, { value: 'b', 'aria-label': 'B', disabled: true }),
    h(RadioGroupItem, { value: 'c', 'aria-label': 'C' }));
}

test('Checkbox controlled parent拒绝、随后异步同步，mixed始终取primitive真实状态', async () => {
  const changes = [];
  const props = { checked: false, onCheckedChange: value => changes.push(value), 'aria-label': '统计容器' };
  await mount(h(Checkbox, props), async (host, render) => {
    const input = () => host.querySelector('[role=checkbox]');
    await click(input());
    assert.deepEqual(changes, [true]);
    assert.equal(input().getAttribute('aria-checked'), 'false');
    await render(h(Checkbox, { ...props, checked: 'indeterminate' }));
    assert.equal(input().getAttribute('aria-checked'), 'mixed');
    assert.equal(host.querySelector('[data-slot=checkbox-indicator]').dataset.state, 'indeterminate');
    assert.ok(host.querySelector('[data-slot=checkbox-mixed]'));
    await render(h(Checkbox, { ...props, checked: true }));
    assert.equal(input().getAttribute('aria-checked'), 'true');
  });
});

test('Checkbox uncontrolled mixed、label激活、disabled与调用方阻止修改', async () => {
  await mount(h('label', null, h(Checkbox, { defaultChecked: 'indeterminate' }), '材料'), async (host, render) => {
    assert.equal(host.querySelector('[role=checkbox]').getAttribute('aria-checked'), 'mixed');
    assert.ok(host.querySelector('[data-slot=checkbox-mixed]'));
    await click(host.querySelector('label'));
    assert.equal(host.querySelector('[role=checkbox]').getAttribute('aria-checked'), 'true');
    await render(h(Checkbox, { checked: false, disabled: true, onCheckedChange: () => assert.fail('disabled changed') }));
    await click(host.querySelector('[role=checkbox]'));
    await render(h(Checkbox, { key: 'readonly', defaultChecked: false, onClick: event => event.preventDefault(), 'aria-readonly': true }));
    await click(host.querySelector('[role=checkbox]'));
    assert.equal(host.querySelector('[role=checkbox]').getAttribute('aria-checked'), 'false');
  });
});

test('Checkbox保留Space原生活动键，不以Enter误提交表单', async () => {
  await mount(h(Checkbox, { 'aria-label': '选择' }), async host => {
    const button = host.querySelector('[role=checkbox]');
    assert.equal(button.type, 'button');
    assert.equal((await key(button, 'Enter')).defaultPrevented, true);
    // jsdom does not synthesize a native Space click; verify the event remains available.
    assert.equal((await key(button, ' ')).defaultPrevented, false);
  });
});

test('RadioGroup互斥、controlled拒绝后同步，以及disabled不触发', async () => {
  const changes = [];
  await mount(group({ value: 'a', onValueChange: value => changes.push(value) }), async (host, render) => {
    await click(host.querySelector('[aria-label=C]'));
    assert.deepEqual(changes, ['c']);
    assert.equal(host.querySelector('[aria-checked=true]').getAttribute('aria-label'), 'A');
    await render(group({ value: 'c', onValueChange: value => changes.push(value) }));
    assert.equal(host.querySelectorAll('[role=radio][aria-checked=true]').length, 1);
    assert.equal(host.querySelector('[aria-checked=true]').getAttribute('aria-label'), 'C');
    await click(host.querySelector('[aria-label=B]'));
    assert.deepEqual(changes, ['c']);
  });
});

test('RadioGroup方向键roving焦点、跳过禁用项、循环并同步唯一选中', async () => {
  await mount(group({ defaultValue: 'a', orientation: 'horizontal' }), async host => {
    const first = host.querySelector('[aria-label=A]');
    const last = host.querySelector('[aria-label=C]');
    await act(async () => first.focus());
    assert.equal(first.tabIndex, 0);
    await key(first, 'ArrowRight');
    assert.equal(document.activeElement, last);
    assert.equal(last.getAttribute('aria-checked'), 'true');
    assert.equal(first.tabIndex, -1);
    assert.equal(last.tabIndex, 0);
    await key(last, 'ArrowRight');
    assert.equal(document.activeElement, first);
    assert.equal(first.getAttribute('aria-checked'), 'true');
    assert.equal(host.querySelectorAll('[role=radio][aria-checked=true]').length, 1);
  });
});

test('主题和明暗用同一受控RadioGroup，三主题样式排除动作按钮污染', () => {
  const settings = source('ui/windows/main/pages/settings/SettingsPage.tsx');
  assert.match(settings, /<RadioGroup className="settings-theme-options"[\s\S]*?value=\{normalizeTheme\(theme\)\} onValueChange=\{handleTheme\}/);
  assert.match(settings, /<RadioGroup className="settings-color-options"[\s\S]*?value=\{normalizeColorMode\(colorMode\)\} onValueChange=\{setColorMode\}/);
  const css = source('ui/styles/controls.css');
  assert.match(css, /\[data-state="indeterminate"\] \[data-slot="checkbox-check"\] \{ display: none;/);
  assert.match(css, /border-image: none; box-shadow: none; transform: none;/);
  assert.match(source('ui/themes/8bitcn/theme.css'), /:not\(:where\(\[role="checkbox"\], \[role="radio"\]\)\)/);
  const minecraft = source('ui/themes/minecraft/appearance.css');
  for (const texture of ['radio_default', 'radio_hover', 'radio_checked', 'radio_checked_hover']) assert.ok(minecraft.includes(`${texture}.png`));
  assert.match(css, /\[data-slot="radio-group-item"\]:focus-visible/);
});
