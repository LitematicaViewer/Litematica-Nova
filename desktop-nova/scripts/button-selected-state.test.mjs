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

test('Aero7：选中换到 check 贴图，常态保持 normal，禁用优先于悬停', () => {
  // Aero7 paints every action-button state from a nine-slice texture, so the winning
  // declaration is `border-image-source` rather than a `background` fill.
  const pressedTexture = declared('aero7', pressed(), 'border-image-source');
  assert.match(pressedTexture.decl.value, /--ln-aero7-button-check-image/, '选中态没有换到 check 贴图');
  const restingTexture = declared('aero7', resting(), 'border-image-source');
  assert.match(restingTexture.decl.value, /--ln-aero7-button-normal-image/, '常态贴图被选中规则覆盖');
  const selectedHover = declared('aero7', { ...pressed(), pseudos: ['hover'] }, 'border-image-source');
  assert.match(selectedHover.decl.value, /--ln-aero7-button-checkhover-image/, '选中悬停没有换到 checkhover 贴图');
  const disabled = declared('aero7', { ...resting(), pseudos: ['disabled'] }, 'border-image-source');
  assert.match(disabled.decl.value, /--ln-aero7-button-disabled-image/, '禁用没有换到 disabled 贴图');
  // `aria-pressed` and Radix Toggle's `data-state="on"` must resolve to the same texture.
  assert.equal(
    value('aero7', pressed('secondary', 'data-state'), 'border-image-source'),
    value('aero7', pressed('secondary', 'aria-pressed'), 'border-image-source'),
  );
});
