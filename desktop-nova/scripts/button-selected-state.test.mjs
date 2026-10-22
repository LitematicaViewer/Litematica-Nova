import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { collect, winner } from './fixtures/css-cascade.mjs';

// Source/CSS contract for the button selected state (docs/UI_ARCHITECTURE.md, V1.21).
//
// Neither shadcn/ui nor 8bitcn/ui declares a selected state on Button - selection is
// published by Toggle there - so Nova has to define one on the shared Button it uses as
// a toggle. The defect this locks down was invisible to every existing check: a rule
// existed (`button[aria-pressed="true"]` in themes/modern.css), it simply never won
// against the (0,4,1) `secondary` variant rule, so a pressed button painted exactly like
// a resting one in both registry themes.
//
// Like minecraft-controls.test.mjs this asserts which declaration *wins*, in the order
// the app loads the stylesheets: shared layer, then the theme sheet the runtime appends.

const root = new URL('../', import.meta.url);
const source = (path) => readFileSync(new URL(path, root), 'utf8');

// `base.css` imports these three in this order; each theme then appends its own sheet,
// and both registry themes @import `modern.css` at the top of theirs.
const SHARED = ['ui/shell/theme.css', 'ui/styles/widgets.css', 'ui/styles/controls.css'];
const THEME_SHEETS = {
  shadcn: [...SHARED, 'ui/themes/modern.css', 'ui/themes/shadcn/theme.css'],
  '8bitcn': [...SHARED, 'ui/themes/modern.css', 'ui/themes/8bitcn/theme.css'],
  minecraft: [...SHARED, 'ui/themes/minecraft/widgets.css', 'ui/themes/minecraft/appearance.css'],
  aero7: [...SHARED, 'ui/themes/aero7/widgets.css', 'ui/themes/aero7/theme.css'],
};
const rules = Object.fromEntries(
  Object.entries(THEME_SHEETS).map(([theme, files]) => [theme, collect(files, root)]),
);

const button = (attrs, extra = {}) => ({ slot: 'button', tag: 'button', attrs, ...extra });
const resting = (variant = 'secondary') => button({ 'data-variant': variant });
const pressed = (variant = 'secondary', attribute = 'aria-pressed') =>
  button({ 'data-variant': variant, [attribute]: attribute === 'aria-pressed' ? 'true' : 'on' });

const declared = (theme, control, property) => winner(rules[theme], control, property, theme);
const value = (theme, control, property) => declared(theme, control, property).decl.value;

test('shadcn：选中的次要/描边按钮换面换字，且由选中规则本身胜出', () => {
  for (const variant of ['secondary', 'outline']) {
    const background = declared('shadcn', pressed(variant), 'background');
    assert.equal(background.decl.value, 'var(--nova-selected-surface)', `${variant} 选中态底色`);
    // The winner has to be the selected rule itself; a resting rule that still won here
    // would paint the same surface as an unselected button.
    assert.match(background.selector, /aria-pressed="true"|data-state="on"/, `${variant} 选中态由常态规则胜出`);
    assert.match(background.path, /ui\/styles\/controls\.css$/, `${variant} 选中态应来自共享层`);
    assert.equal(value('shadcn', pressed(variant), 'color'), 'var(--nova-selected-foreground)');
  }
});

test('shadcn：选中面不是 accent——常态底色与动作色的混合才看得出差别', () => {
  // Measured against this theme's palette: `--accent` (the colour shadcn's Toggle uses)
  // is 1.09:1 from the resting `--secondary` fill, i.e. a selected button would look
  // unselected. The token has to stay a mix that moves away from the page in both modes.
  const css = source('ui/themes/modern.css');
  assert.match(css, /--nova-selected-surface: color-mix\(in oklab, var\(--secondary\) 74%, var\(--primary\)\);/);
  assert.match(css, /--nova-selected-hover: color-mix\(in oklab, var\(--secondary\) 70%, var\(--primary\)\);/);
  assert.match(css, /--nova-selected-foreground: var\(--foreground\);/);
  assert.doesNotMatch(css, /--nova-selected-surface: var\(--accent\)/);
});

test('shadcn：常态与悬停仍是原值，选中态不是靠改常态换来的', () => {
  assert.equal(value('shadcn', resting(), 'background'), 'var(--secondary)');
  assert.equal(value('shadcn', resting(), 'border-color'), 'transparent');
  const hovered = button({ 'data-variant': 'secondary' }, { pseudos: ['hover'] });
  assert.equal(value('shadcn', hovered, 'background'), 'var(--accent)');
  // Selected + hover must not fall back to the resting hover: accent is lighter than the
  // selected fill, so it would read as the button turning itself off under the pointer.
  const selectedHover = button({ 'data-variant': 'secondary', 'aria-pressed': 'true' }, { pseudos: ['hover'] });
  assert.equal(value('shadcn', selectedHover, 'background'), 'var(--nova-selected-hover)');
});

test('shadcn：实心变体与结构按钮不被接管', () => {
  // `default` is filled, and that fill *is* the selected look in the mode switchers.
  assert.equal(value('shadcn', pressed('default'), 'background'), 'var(--primary)');
  // Ghost buttons are navigation rows, library tiles and hotbar slots; their selected
  // look belongs to the page that lays them out, and `controls.css` loads before page CSS.
  assert.equal(value('shadcn', pressed('ghost'), 'background'), 'transparent');
});

test('8bitcn：选中按钮是主色实心板，与 8bitcn 自己的 Toggle 口径一致', () => {
  // The shared rule still spends the token; 8bitcn only redefines the palette and adds
  // its pixel plate treatment.
  assert.equal(value('8bitcn', pressed(), 'background'), 'var(--nova-selected-surface)');
  const css = source('ui/themes/8bitcn/theme.css');
  assert.match(css, /--nova-selected-surface: var\(--primary\);/);
  assert.match(css, /--nova-selected-foreground: var\(--primary-foreground\);/);
  assert.match(css, /--nova-selected-hover: color-mix\(in oklab, var\(--primary\) 86%, var\(--background\)\);/);
  // The raised drop shadow collapses into an inset edge, and the stepped border is
  // recoloured: its `--border` green would otherwise vanish into the primary fill.
  const shadow = declared('8bitcn', pressed(), 'box-shadow');
  assert.match(shadow.decl.value, /^inset /);
  assert.match(shadow.path, /ui\/themes\/8bitcn\/theme\.css$/);
  assert.match(css, /\[data-slot="button-decorations"\] > span \{\s*background-color: var\(--primary-foreground\);/);
  assert.match(css, /span\.nova-pixel-fill-soft \{\s*background-color: color-mix\(in oklab, var\(--primary-foreground\) 30%, transparent\);/);
  // 8bitcn keeps its resting fill; the selected palette must not leak into it.
  assert.equal(value('8bitcn', resting(), 'background'), 'var(--secondary)');
});

test('Minecraft：选中仍走 check 贴图与黄字，不被共享层的填充色接管', () => {
  const pressedTexture = declared('minecraft', pressed(), '--ln-minecraft-button-image');
  assert.match(pressedTexture.decl.value, /--ln-minecraft-button-check-image/, '选中态没有换到 check 贴图');
  assert.equal(value('minecraft', pressed(), 'color'), '#ffff00');
  const restingTexture = declared('minecraft', resting(), '--ln-minecraft-button-image');
  assert.match(restingTexture.decl.value, /--ln-minecraft-button-normal-image/, '常态贴图被选中规则覆盖');
  // The shared layer is scoped away from Minecraft on purpose: a fill would paint over
  // the textured plate.
  assert.equal(value('minecraft', pressed(), 'background'), 'transparent');
});

test('三个主题都同时认 aria-pressed 与 Radix Toggle 的 data-state="on"', () => {
  for (const theme of ['shadcn', '8bitcn', 'minecraft']) {
    for (const property of theme === 'minecraft' ? ['color', '--ln-minecraft-button-image'] : ['background', 'color', 'box-shadow']) {
      assert.equal(
        value(theme, pressed('secondary', 'data-state'), property),
        value(theme, pressed('secondary', 'aria-pressed'), property),
        `${theme} 的 ${property} 在两种属性下不一致`,
      );
    }
  }
});

test('级联模型能分辨选中与常态（护栏的护栏）', () => {
  // Without this, a model that ignores `[aria-pressed]` would make every assertion above
  // pass against the resting declarations alone - the false-green this suite exists for.
  assert.notEqual(value('shadcn', pressed(), 'background'), value('shadcn', resting(), 'background'));
  assert.equal(
    declared('shadcn', pressed(), 'background').rank.specificity > declared('shadcn', resting(), 'background').rank.specificity,
    true,
    '选中规则必须比常态规则更具体或更靠后',
  );
});

test('Aero7：L0 本体恒定 normal，状态由叠加层 opacity 驱动（选中 / 选中悬停 / 禁用）', () => {
  // Aero7 paints every action-button state from a nine-slice texture. Under the P1
  // ruling the artwork no longer swaps on the body: L0 keeps `normal` forever and the
  // remaining states live on constant-texture overlay layers whose `opacity` moves -
  // L1 body `::before` = hover, L2 body `::after` = check,
  // L3 `[data-slot="button-motion-layer"]` = checkhover, L4 that host's `::before`
  // = disabled. This used to assert the body swapped `border-image-source` per state,
  // which P1 removed; the intent (prove every state actually lands) is unchanged.

  // 1. L0 keeps the normal artwork across all five states.
  const states = [
    ['常态', {}],
    ['悬停', { pseudos: ['hover'] }],
    ['选中', { attrs: { 'aria-pressed': 'true' } }],
    ['选中悬停', { attrs: { 'aria-pressed': 'true' }, pseudos: ['hover'] }],
    ['禁用', { pseudos: ['disabled'] }],
  ];
  for (const [label, state] of states) {
    const body = declared(
      'aero7',
      button({ 'data-variant': 'secondary', ...(state.attrs ?? {}) }, { pseudos: state.pseudos }),
      'border-image-source',
    );
    assert.match(body.decl.value, /--ln-aero7-button-normal-image/, `L0 本体在「${label}」下应恒为 normal 贴图`);
  }

  // 2. L1 (body `::before`) is the unselected hover layer: hover texture, opacity 1.
  const hoverLayer = button({ 'data-variant': 'secondary' }, { pseudos: ['before'] });
  assert.match(value('aero7', hoverLayer, 'border-image-source'), /--ln-aero7-button-hover-image/, 'L1 应恒定 hover 贴图');
  assert.equal(
    value('aero7', button({ 'data-variant': 'secondary' }, { pseudos: ['before', 'hover'] }), 'opacity'),
    '1',
    '悬停时 L1 应由 opacity 1 显现',
  );

  // 3. Selected (no hover): L2 (body `::after`) carries check at opacity 1, and stays
  //    lit once the pointer also hovers.
  const check = {
    slot: 'button',
    tag: 'button',
    attrs: { 'data-variant': 'secondary', 'aria-pressed': 'true' },
    pseudos: ['after'],
  };
  assert.equal(value('aero7', check, 'opacity'), '1', '选中时 L2 应由 opacity 1 显现');
  assert.match(value('aero7', check, 'border-image-source'), /--ln-aero7-button-check-image/, 'L2 应恒定 check 贴图');
  assert.equal(value('aero7', { ...check, pseudos: ['after', 'hover'] }, 'opacity'), '1', '选中悬停时 L2 仍应保持 opacity 1');

  // 4. Selected + hover: L3 - the `button-motion-layer` host, a different slot driven by
  //    the ancestral button's `aria-pressed` + `:hover` - lights up with checkhover.
  const checkHover = {
    slot: 'button-motion-layer',
    tag: 'span',
    attrs: {},
    pseudos: [],
    ancestors: [{ slot: 'button', attrs: { 'data-variant': 'secondary', 'aria-pressed': 'true' }, pseudos: ['hover'] }],
  };
  assert.equal(value('aero7', checkHover, 'opacity'), '1', '选中悬停时 L3 应由 opacity 1 显现');
  assert.match(value('aero7', checkHover, 'border-image-source'), /--ln-aero7-button-checkhover-image/, 'L3 应恒定 checkhover 贴图');
  assert.equal(
    value('aero7', { ...checkHover, ancestors: [{ slot: 'button', attrs: { 'data-variant': 'secondary' }, pseudos: [] }] }, 'opacity'),
    '0',
    '未选中未悬停时 L3 应回到 opacity 0',
  );

  // 5. Disabled: L4 (host `::before`) wins with the disabled artwork...
  const disabledHost = {
    slot: 'button-motion-layer',
    tag: 'span',
    attrs: {},
    pseudos: ['before'],
    ancestors: [{ slot: 'button', attrs: { 'data-variant': 'secondary' }, pseudos: ['disabled'] }],
  };
  assert.equal(value('aero7', disabledHost, 'opacity'), '1', '禁用时 L4 应由 opacity 1 显现');
  assert.match(value('aero7', disabledHost, 'border-image-source'), /--ln-aero7-button-disabled-image/, 'L4 应恒定 disabled 贴图');

  // ...and disabled outranks hover/check: L1 and L3 fall back to 0.
  assert.equal(
    value('aero7', button({ 'data-variant': 'secondary' }, { pseudos: ['before', 'disabled'] }), 'opacity'),
    '0',
    '禁用时 L1 应回到 opacity 0',
  );
  assert.equal(
    value('aero7', { ...checkHover, ancestors: [{ slot: 'button', attrs: { 'data-variant': 'secondary' }, pseudos: ['disabled'] }] }, 'opacity'),
    '0',
    '禁用时 L3 应回到 opacity 0',
  );
  // L2 is the one layer the cascade model cannot rule out by state: it reads `:not()`
  // as always true, so a disabled+`aria-pressed` button still resolves L2 to 1. The
  // guard is therefore checked against the source, together with L2's base `opacity: 0`
  // from point 3's resting query - which together prove disabled pins L2 to 0.
  const aero7 = source('ui/themes/aero7/widgets.css');
  assert.match(
    aero7,
    /\[aria-pressed="true"\]:not\(:disabled\)::after\s*,\s*\[data-slot="button"\]:where\(:not\([^)]*\)\)\[data-state="on"\]:not\(:disabled\)::after\s*\{\s*opacity:\s*1;/,
    'L2 的选中驱动必须带 :not(:disabled) 保护，禁用时不得显现',
  );
  assert.equal(
    value('aero7', { slot: 'button', tag: 'button', attrs: { 'data-variant': 'secondary' }, pseudos: ['after'] }, 'opacity'),
    '0',
    'L2 基态应为 opacity 0，禁用时保持隐藏',
  );

  // 6. `aria-pressed="true"` and Radix Toggle's `data-state="on"` are the same signal:
  //    each resolves to the same layer with the same opacity and the same texture.
  const byPressed = { slot: 'button', tag: 'button', attrs: { 'data-variant': 'secondary', 'aria-pressed': 'true' }, pseudos: ['after'] };
  const byState = { slot: 'button', tag: 'button', attrs: { 'data-variant': 'secondary', 'data-state': 'on' }, pseudos: ['after'] };
  assert.equal(value('aero7', byPressed, 'opacity'), value('aero7', byState, 'opacity'), 'L2 的 opacity 两来源不一致');
  assert.equal(
    value('aero7', byPressed, 'border-image-source'),
    value('aero7', byState, 'border-image-source'),
    'L2 的贴图两来源不一致',
  );
  const l3ByState = {
    ...checkHover,
    ancestors: [{ slot: 'button', attrs: { 'data-variant': 'secondary', 'data-state': 'on' }, pseudos: ['hover'] }],
  };
  assert.equal(value('aero7', checkHover, 'opacity'), value('aero7', l3ByState, 'opacity'), 'L3 的 opacity 两来源不一致');
  assert.equal(
    value('aero7', checkHover, 'border-image-source'),
    value('aero7', l3ByState, 'border-image-source'),
    'L3 的贴图两来源不一致',
  );
});
