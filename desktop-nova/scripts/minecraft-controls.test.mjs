import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import test from 'node:test';
import postcss from 'postcss';

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

const LAYER_ORDER = ['theme', 'vendor', 'tokens', 'base', 'components', 'utilities'];

function specificity(selector) {
  // `:where()` contributes nothing; `:not()`/`:is()` contribute their argument.
  const flat = selector
    .replace(/:where\([^)]*\)/g, ' ')
    .replace(/:(?:not|is)\(([^)]*)\)/g, ' $1 ');
  const ids = (flat.match(/#[\w-]+/g) ?? []).length;
  const classes =
    (flat.match(/\.[\w-]+/g) ?? []).length +
    (flat.match(/\[[^\]]+\]/g) ?? []).length +
    (flat.match(/:(?!:)[\w-]+/g) ?? []).length;
  const types = (flat.match(/(?:^|[\s>+~,(])([a-z][a-z0-9]*)/g) ?? []).length;
  return ids * 1e6 + classes * 1e3 + types;
}

function layerOf(rule) {
  for (let parent = rule.parent; parent; parent = parent.parent) {
    if (parent.type === 'atrule' && parent.name === 'layer' && parent.params && parent.nodes) return parent.params.trim();
  }
  return null;
}

function collect(files) {
  const rules = [];
  let order = 0;
  for (const path of files) {
    const css = parse(path);
    let relative = path;
    css.walkRules((rule) => {
      for (const selector of rule.selectors) {
        rules.push({ path: relative, selector, decls: rule.nodes.filter((node) => node.type === 'decl'), layer: layerOf(rule), order: order++ });
      }
    });
  }
  return rules;
}

/** The theme a selector is pinned to, or `undefined` when it applies to all themes. */
function themeOf(selector) {
  const m = /\[data-theme(?:="([^"]*)")?\]/.exec(selector);
  if (!m) return undefined;
  return m[1] ?? null;
}

/**
 * The declaration that wins `property` for a Minecraft control, ranked by layer,
 * specificity, then order.
 *
 * `control` is `{ slot, tag, pseudos }`. `pseudos` lists the state being asked about
 * (e.g. `['hover']`); a rule carrying any other state cannot apply. `:not(...)` is
 * ignored: it is a negative condition, and the controls under test are enabled.
 *
 * A rule is eligible when its rightmost compound could actually match the element: it
 * may not pin a *different* slot, and it must either name this slot or be a plain
 * element rule - the generic `button` texture is exactly the competitor that matters
 * for the dropdown trigger.
 */
function winner(rules, control, property) {
  const marker = `[data-slot="${control.slot}"]`;
  const wanted = control.pseudos ?? [];
  let best = null;
  for (const rule of rules) {
    // Only the rightmost compound may introduce the slot, or the rule targets a child.
    const tail = rule.selector.split(/\s+|>/).pop();
    const bare = tail.replace(/:(?:not|is|where)\([^)]*\)/g, " ");
    const slots = [...bare.matchAll(/\[data-slot="([^"]+)"\]/g)].map((m) => m[1]);
    if (slots.some((slot) => slot !== control.slot)) continue;
    const named = bare.includes(marker);
    const element = new RegExp(`(^|[^\\w-])${control.tag}(?![-\\w])`).test(bare);
    if (!named && !element) continue;
    // A class in the rightmost compound means the rule targets a differently-classed
    // element; `button.flake-page__quickbar-slot` can never match a dropdown trigger.
    const classes = [...bare.matchAll(/\.([\w-]+)/g)].map((m) => m[1]);
    if (classes.some((name) => !(control.classes ?? []).includes(name))) continue;
    // Likewise for any attribute beyond the slot itself: `button[aria-pressed="true"]`
    // does not apply to a control that never carries that attribute. A presence-only
    // constraint (`[data-state]`) just requires the control to carry the attribute.
    const attrs = [...bare.matchAll(/\[([\w-]+)(?:="([^"]*)")?\]/g)]
      .filter(([, name]) => name !== "data-slot")
      .map(([, name, value]) => [name, value]);
    if (attrs.some(([name, value]) => {
      const declared = (control.attrs ?? {})[name];
      if (declared === undefined) return true;
      return value !== undefined && declared !== value;
    })) continue;
    const states = [...bare.matchAll(/:([a-z-]+)/g)].map((m) => m[1]);
    if (states.some((state) => !wanted.includes(state))) continue;
    // A selector pinned to another theme can never apply here. `[data-theme]` without
    // a value means "any theme" and must stay eligible, or every shared-layer rule is
    // filtered out and the assertions pass against the theme's own rules alone.
    const theme = themeOf(rule.selector);
    if (theme !== undefined && theme !== null && theme !== "minecraft") continue;
    const decl = rule.decls.find((node) => node.prop === property);
    if (!decl) continue;
    const rank = {
      layer: rule.layer === null ? LAYER_ORDER.length : LAYER_ORDER.indexOf(rule.layer),
      specificity: specificity(rule.selector),
      order: rule.order,
    };
    const better =
      best === null ||
      rank.layer > best.rank.layer ||
      (rank.layer === best.rank.layer && rank.specificity > best.rank.specificity) ||
      (rank.layer === best.rank.layer && rank.specificity === best.rank.specificity && rank.order > best.rank.order);
    if (better) best = { decl, rank, selector: rule.selector, path: rule.path };
  }
  assert.ok(best, `no rule sets ${property} for [data-slot="${control.slot}"]`);
  return best;
}

const rules = collect([
  'ui/styles/controls.css',
  'ui/styles/widgets.css',
  'ui/themes/minecraft/appearance.css',
  'ui/themes/minecraft/widget/button.css',
  'ui/themes/minecraft/widget/input.css',
]);

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
  assert.equal(winner(rules, control, 'background-image').decl.value, 'url("../resource/form/spin_down.png")');

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
const buttonCss = parse('ui/themes/minecraft/widget/button.css');

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
      const resolved = new URL(file, new URL('ui/themes/minecraft/widget/', root));
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
    const resolved = new URL(file, new URL('ui/themes/minecraft/widget/', root));
    assert.ok(existsSync(resolved), `回退贴图不存在: ${file}`);
  }
});