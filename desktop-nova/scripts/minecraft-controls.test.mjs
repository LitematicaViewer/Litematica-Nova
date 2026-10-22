import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import test from 'node:test';
import postcss from 'postcss';
import { collect, layerRank, themeOf, winner } from './fixtures/css-cascade.mjs';

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

test('主题声明结构生效：components.theme 之上的规则胜过更具体的共享规则', () => {
  // The theme layer is `components.theme`, a sublayer of `components`. Rules directly in
  // the parent (`styles/**`, page CSS) rank below it, so a theme declaration wins by
  // layer instead of by specificity — this is the contract that lets a theme say
  // "the 9-slice texture owns radius/border/colour" without out-specifying every shared rule.
  assert.ok(layerRank('components.theme') > layerRank('components'));
  assert.ok(layerRank('components.page') > layerRank('components.shared'));

  const focus = winner(rules, { slot: 'button', tag: 'button', pseudos: ['focus-visible'] }, 'outline', 'minecraft');
  assert.match(focus.path, /^ui\/themes\//, `shared rule ${focus.selector} still owns the focus ring`);
  assert.equal(focus.rank.layer, layerRank('components.theme'));
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
// Files follow `button_<light|dark>_<variant>_<state>_min.png`. The CVA `default`
// variant is the primary action, so its filename segment is `primary`; the
// no-variant / `secondary` plate is the `default` set. `check_hover` was renamed to
// `checkhover`. Four textured variants exist per scheme; `ghost` and `link` are
// borderless and deliberately declare no set.
//
// Every set must declare all five states, and every `url()` must name the exact file
// for its scheme/variant/state. Two failure modes are silent: a state that falls back
// to the `default` plate, and a filename typo that makes the var resolve to nothing.
// Both are asserted here rather than discovered by eye.

const BUTTON_TEXTURE_VARIANTS = {
  secondary: 'default',
  default: 'primary',
  outline: 'outline',
  destructive: 'destructive',
};
const BUTTON_STATES = ['normal', 'hover', 'check', 'checkhover', 'disabled'];
const BUTTON_SCHEMES = ['dark', 'light'];
const buttonCss = parse('ui/themes/minecraft/widgets.css');

const imageStateOf = (prop) => /^--ln-minecraft-button-(.+)-image$/.exec(prop)?.[1] ?? null;
const variantOf = (selector) => /\[data-variant="([^"]+)"\]/.exec(selector)?.[1] ?? 'secondary';
const schemeOf = (selector) => (/data-color-scheme="light"/.test(selector) ? 'light' : 'dark');

test('Minecraft 按钮贴图按 明暗/变体/状态 命名且文件存在', () => {
  const sets = new Map();
  buttonCss.walkRules((rule) => {
    if (!rule.selector.includes('[data-slot="button"]')) return;
    const defined = new Map();
    for (const decl of rule.nodes.filter((node) => node.type === 'decl')) {
      const state = imageStateOf(decl.prop);
      if (state) defined.set(state, decl.value);
    }
    if (!defined.size) return;
    const key = `${schemeOf(rule.selector)}:${variantOf(rule.selector)}`;
    assert.ok(!sets.has(key), `${key} 的贴图集重复声明`);
    sets.set(key, defined);
  });

  const expectedKeys = BUTTON_SCHEMES.flatMap((scheme) =>
    Object.keys(BUTTON_TEXTURE_VARIANTS).map((variant) => `${scheme}:${variant}`),
  ).sort();
  assert.deepEqual([...sets.keys()].sort(), expectedKeys, '贴图集必须覆盖两种明暗 × 四个变体，ghost/link 不声明贴图');

  for (const [key, defined] of sets) {
    const [scheme, variant] = key.split(':');
    const segment = BUTTON_TEXTURE_VARIANTS[variant];
    assert.deepEqual([...defined.keys()].sort(), [...BUTTON_STATES].sort(), `${key} 的状态不完整`);
    for (const [state, value] of defined) {
      assert.equal(
        value,
        `url("./resource/button/button_${scheme}_${segment}_${state}_min.png")`,
        `${key}/${state} 的文件名不符合命名规则`,
      );
      const resolved = new URL(/url\("([^"]+)"\)/.exec(value)[1], new URL('ui/themes/minecraft/', root));
      assert.ok(existsSync(resolved), `${key}/${state} 贴图不存在: ${resolved.pathname}`);
    }
  }
});

// --- button label colours ----------------------------------------------------
// `appearance.css` owns the palette; the button rules reference it through `var()`.
// The tests resolve one level so the contract stays about the visible colour.

const PALETTE = new Map();
parse('ui/themes/minecraft/appearance.css').walkDecls(/^--ln-minecraft-palette-/, (decl) => {
  PALETTE.set(decl.prop, decl.value);
});

const resolvePalette = (value) => {
  const token = /^var\((--ln-minecraft-palette-[a-z-]+)\)$/.exec(value)?.[1];
  return token ? PALETTE.get(token) : value;
};

const BUTTON_LABEL_COLOURS = {
  dark: {
    secondary: ['white', 'white', 'gray'],
    default: ['white', 'white', 'gray'],
    outline: ['white', 'white', 'gray'],
    destructive: ['white', 'white', 'red'],
    ghost: ['white', 'white', 'gray'],
    link: ['purple', 'purple', 'gray'],
  },
  light: {
    secondary: ['dark-gray', 'dark-gray', 'gray'],
    default: ['white', 'white', 'gray'],
    outline: ['dark-gray', 'dark-gray', 'gray'],
    destructive: ['white', 'white', 'red'],
    ghost: ['dark-gray', 'dark-gray', 'gray'],
    link: ['green', 'green', 'gray'],
  },
};

test('Minecraft 按钮文字色符合 明暗/变体/状态 调色板表', () => {
  assert.deepEqual(Object.fromEntries(PALETTE), {
    '--ln-minecraft-palette-white': '#ffffff',
    '--ln-minecraft-palette-dark-gray': '#3f3f3f',
    '--ln-minecraft-palette-gray': '#808080',
    '--ln-minecraft-palette-yellow': '#ffff00',
    '--ln-minecraft-palette-red': '#ff5555',
    '--ln-minecraft-palette-green': '#3c8527',
    '--ln-minecraft-palette-purple': '#7345e5',
  });

  const declared = new Map();
  buttonCss.walkRules((rule) => {
    if (!rule.selector.includes('[data-slot="button"]')) return;
    if (/:(hover|active|disabled)|aria-pressed|data-state/.test(rule.selector)) return;
    const colours = {};
    rule.walkDecls(/^--ln-minecraft-button-(normal|hover|disabled)-color$/, (decl) => {
      colours[decl.prop.replace('--ln-minecraft-button-', '').replace('-color', '')] = decl.value;
    });
    if (!Object.keys(colours).length) return;
    declared.set(`${schemeOf(rule.selector)}:${variantOf(rule.selector)}`, colours);
  });

  for (const [scheme, variants] of Object.entries(BUTTON_LABEL_COLOURS)) {
    for (const [variant, expected] of Object.entries(variants)) {
      const colours = declared.get(`${scheme}:${variant}`);
      assert.ok(colours, `${scheme}:${variant} 没有声明文字色`);
      for (const [index, state] of ['normal', 'hover', 'disabled'].entries()) {
        assert.equal(
          resolvePalette(colours[state]),
          PALETTE.get(`--ln-minecraft-palette-${expected[index]}`),
          `${scheme}:${variant}/${state} 文字色不符合调色板表`,
        );
      }
    }
  }

  // The state rules must read the support variables; a hardcoded colour there would
  // bypass the palette table above while still looking correct in one state.
  const declaration = (selector, property) => {
    let value;
    buttonCss.walkRules((rule) => {
      if (!rule.selectors.includes(selector)) return;
      rule.walkDecls(property, (decl) => { value = decl.value; });
    });
    assert.ok(value, `${selector} 没有声明 ${property}`);
    return value;
  };
  assert.match(declaration('[data-slot="button"]', 'color'), /^var\(--ln-minecraft-button-normal-color/);
  assert.match(declaration('[data-slot="button"]:hover:not(:disabled)', 'color'), /^var\(--ln-minecraft-button-hover-color/);
  assert.match(declaration('[data-slot="button"]:active:not(:disabled)', 'color'), /^var\(--ln-minecraft-button-hover-color/);
  assert.match(declaration('[data-slot="button"]:disabled', 'color'), /^var\(--ln-minecraft-button-disabled-color/);
  assert.equal(declaration('[data-slot="button"]:hover:not(:disabled)', '--ln-minecraft-button-image'), 'var(--ln-minecraft-button-hover-image)');
  assert.equal(declaration('[data-slot="button"]:active:not(:disabled)', '--ln-minecraft-button-image'), 'var(--ln-minecraft-button-normal-image)');
  assert.equal(declaration('[data-slot="button"]:disabled', '--ln-minecraft-button-image'), 'var(--ln-minecraft-button-disabled-image)');
});

test('Minecraft 选中与选中悬停始终是调色板黄色', () => {
  const colours = [];
  buttonCss.walkRules((rule) => {
    if (!/^\[data-slot="button"\]\[(aria-pressed|data-state)/.test(rule.selector)) return;
    rule.walkDecls('color', (decl) => colours.push(resolvePalette(decl.value)));
  });
  assert.ok(colours.length >= 2, `预期选中态与选中悬停两条规则，实际 ${colours.length}`);
  for (const colour of colours) assert.equal(colour, PALETTE.get('--ln-minecraft-palette-yellow'));
});
