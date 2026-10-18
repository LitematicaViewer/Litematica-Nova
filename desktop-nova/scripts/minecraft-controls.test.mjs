import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import test from 'node:test';
import postcss from 'postcss';
import { collect, themeOf, winner } from './fixtures/css-cascade.mjs';

// Source/CSS cascade contracts for the textured Minecraft controls. No browser and no
// visual acceptance: this asserts which declaration *wins*, not merely which exist.
//
// Both bugs this file guards against were observed in the running app while every
// existing check stayed green, because a theme rule that declares the right value can
// still lose to a more specific shared rule:
//   * the shared layer rounds the control-layout radio into a 50% circle, clipping
//     the square Minecraft radio texture;
//   * the shared layer fills a checked checkbox with `--primary`, which showed through
//     the transparent centre of the Minecraft checkbox texture.
// The limit of this test is that it ranks only rules that name the slot in their
// rightmost compound, and it does not resolve `var()` values.

const root = new URL('../', import.meta.url);
const parse = (path) => postcss.parse(readFileSync(new URL(path, root), 'utf8'), { from: path });

// The cascade model (LAYER_ORDER, specificity, collect, winner) lives in
// `fixtures/css-cascade.mjs`, shared with `button-selected-state.test.mjs` so both
// suites rank declarations identically instead of drifting apart.
const rules = collect([
  'ui/styles/controls.css',
  'ui/styles/widgets.css',
  'ui/themes/minecraft/appearance.css',
  // One file now: Phase 5 merged nav-icons.css and every widget/*.css into it.
  'ui/themes/minecraft/widgets.css',
], root);

test('the cascade filter rejects rules pinned to another theme', () => {
  // Guards the guard: without this the radio assertion passes on 8bitcn's rule.
  assert.equal(themeOf('html[data-theme="8bitcn"] [data-slot="radio-group-item"]'), '8bitcn');
  assert.equal(themeOf('html[data-theme="minecraft"] button[data-slot="checkbox"]'), 'minecraft');
  assert.equal(themeOf('html[data-theme] [data-slot="checkbox"]'), null);
  assert.equal(themeOf('[data-slot="checkbox"]'), undefined);
  // `[data-theme]` with no value is the shared layer and must stay in the running;
  // treating it as "another theme" silently emptied the candidate set.
  const shared = rules.filter((entry) => themeOf(entry.selector) === null);
  assert.ok(shared.length > 20, `expected the shared layer to be eligible, got ${shared.length} selectors`);
});

test('Minecraft 单选框贴图不被共享层的圆形裁切', () => {
  const control = { slot: 'radio-group-item', tag: 'button', attrs: { 'data-choice-layout': 'control', 'data-state': 'checked' } };
  const rounded = winner(rules, control, 'border-radius');
  assert.equal(rounded.decl.value, '0', `[data-choice-layout="control"] is clipped by ${rounded.selector} in ${rounded.path}`);
});

test('Minecraft 复选框贴图不被共享层的圆角裁切', () => {
  // A background/texture is clipped by the border radius, so a stray 4px radius shows
  // the square texture with rounded corners. The shared base rule carried a redundant
  // `button` element, which made it (0,2,2) - more specific than every theme override at
  // (0,2,1), so the radius could not be zeroed. The radio never had this because its
  // shared rule has no element selector.
  const unchecked = { slot: 'checkbox', tag: 'button' };
  const rounded = winner(rules, unchecked, 'border-radius');
  assert.equal(rounded.decl.value, '0', `the texture is clipped by ${rounded.selector} in ${rounded.path}`);

  // The checksum: the shared rule must still round the other themes' checkboxes, so the
  // fix has to be specificity, not deleting the declaration.
  assert.ok(
    rules.some(
      (entry) =>
        /\[data-slot="checkbox"\]/.test(entry.selector) &&
        entry.decls.some((decl) => decl.prop === 'border-radius' && decl.value === '4px'),
    ),
    'the shared 4px radius disappeared instead of losing the specificity contest',
  );
});

test('Minecraft 复选框贴图中心不被共享层的 --primary 填充', () => {
  const control = { slot: 'checkbox', tag: 'button', attrs: { 'data-state': 'checked' } };
  const fill = winner(rules, control, 'background-color');
  assert.equal(fill.decl.value, 'transparent', `the checked fill leaks through from ${fill.selector} in ${fill.path}`);
});

test('Minecraft 下拉触发器采用输入框外观而非通用按钮贴图', () => {
  // `button.css` styles every <button> with a texture, and the trigger is a <button>.
  const control = { slot: 'select-trigger', tag: 'button' };
  assert.equal(winner(rules, control, 'border-image').decl.value, 'none', 'the generic button texture still applies to the trigger');
  assert.equal(winner(rules, control, 'background-color').decl.value, 'var(--ln-minecraft-input-background)');
  // Relative-prefix agnostic: the theme files were merged, which moved every asset path
  // from `../resource/` to `./resource/`. What matters is the target, not the prefix.
  assert.match(winner(rules, control, 'background-image').decl.value, /url\("[./]*resource\/form\/spin_down\.png"\)/);

  // Hover and active have to restate the field colours, because the generic
  // `button:hover` / `button:active` rules are otherwise more specific and would paint
  // the trigger button-yellow.
  const hover = { ...control, pseudos: ['hover'] };
  assert.equal(winner(rules, hover, 'color').decl.value, 'var(--ln-minecraft-input-color)');
  assert.equal(winner(rules, hover, 'border-color').decl.value, 'var(--ln-minecraft-input-border)');
});

test('共享层只声明填充色，不重置贴图', () => {
  // A `background` shorthand here would reset `background-image`, which is how the
  // theme ended up out-specifying the image while still losing the colour.
  for (const rule of rules.filter((entry) => entry.path === 'ui/styles/controls.css')) {
    if (!rule.selector.includes('[data-slot="checkbox"]')) continue;
    assert.ok(!rule.decls.some((node) => node.prop === 'background'), `${rule.selector} resets background-image`);
  }
});

// --- button texture sets -----------------------------------------------------
// A variant only supplies its state images; the state rules resolve
// `var(--ln-minecraft-button-<state>-image, <base>)`. Two failure modes are silent
// there: a variant missing a state (it keeps the base texture for that state) and a
// filename typo (the var resolves to nothing and the fallback wins). Both are asserted
// here rather than discovered by eye.
//
// `disabled` is optional on purpose: a disabled button looking the same in every variant
// is a sound default, so a variant may deliberately fall back to the shared disabled
// plate (destructive does). Every other state is required, because those are the ones
// where a variant must look different from the base.

const BUTTON_REQUIRED_STATES = ['normal', 'hover', 'check', 'check-hover'];
const BUTTON_ALL_STATES = [...BUTTON_REQUIRED_STATES, 'disabled'];
const buttonCss = parse('ui/themes/minecraft/widgets.css');

test('Minecraft 按钮变体的状态贴图完整且文件存在', () => {
  const sets = new Map();
  buttonCss.walkRules((rule) => {
    const variant = /\[data-variant="([^"]+)"\]/.exec(rule.selector);
    if (!variant) return;
    const defined = new Map();
    for (const decl of rule.nodes.filter((node) => node.type === 'decl')) {
      const state = /^--ln-minecraft-button-(.+)-image$/.exec(decl.prop);
      if (state) defined.set(state[1], decl.value);
    }
    if (!defined.size) return;
    sets.set(variant[1], defined);
    assert.deepEqual(
      [...defined.keys()].filter((state) => !BUTTON_ALL_STATES.includes(state)),
      [],
      `${variant[1]} 声明了未知状态`,
    );
  });

  assert.ok(sets.size >= 3, `预期至少三个变体有贴图集，实际 ${sets.size}`);
  for (const [variant, defined] of sets) {
    const missing = BUTTON_REQUIRED_STATES.filter((state) => !defined.has(state));
    assert.deepEqual(missing, [], `${variant} 缺少状态: ${missing.join(', ')}`);
    for (const [state, value] of defined) {
      const file = /url\("([^"]+)"\)/.exec(value)?.[1];
      assert.ok(file, `${variant}/${state} 不是 url(): ${value}`);
      const resolved = new URL(file, new URL('ui/themes/minecraft/', root));
      assert.ok(existsSync(resolved), `${variant}/${state} 贴图不存在: ${file}`);
    }
  }
});

test('Minecraft 按钮的基础回退贴图文件存在', () => {
  const fallbacks = new Set();
  buttonCss.walkDecls(/^--ln-minecraft-button-image$/, (decl) => {
    for (const match of decl.value.matchAll(/url\("([^"]+)"\)/g)) fallbacks.add(match[1]);
  });
  assert.ok(fallbacks.size >= 5, `预期至少 5 张回退贴图，实际 ${fallbacks.size}`);
  for (const file of fallbacks) {
    const resolved = new URL(file, new URL('ui/themes/minecraft/', root));
    assert.ok(existsSync(resolved), `回退贴图不存在: ${file}`);
  }
});