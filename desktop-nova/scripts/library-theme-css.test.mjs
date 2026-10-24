import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import postcss from 'postcss';

// Source/CSS contracts only: no browser, DOM emulation, or visual acceptance.
const root = new URL('../', import.meta.url);
const source = (path) => readFileSync(new URL(path, root), 'utf8');
const parse = (path) => postcss.parse(source(path), { from: path });
const library = parse('ui/windows/main/pages/library/library.css');
const minecraft = parse('ui/themes/minecraft/appearance.css');
const controls = parse('ui/styles/controls.css');
function rule(css, selector) {
  const declarations = [];
  css.walkRules((entry) => {
    if (entry.selectors.includes(selector)) declarations.push(...entry.nodes.filter((node) => node.type === 'decl'));
  });
  assert.ok(declarations.length, `missing CSS contract: ${selector}`);
  return Object.fromEntries(declarations.map(({ prop, value }) => [prop, value]));
}
function palette(theme, mode) {
  const css = theme === 'minecraft' ? minecraft : parse(`ui/themes/${theme}/theme.css`);
  const base = theme === 'minecraft' ? 'html[data-theme="minecraft"]' : ':root';
  const overrides = theme === 'minecraft' ? 'light' : 'dark';
  return { ...rule(css, base), ...(mode === overrides ? rule(css, `${base}[data-color-scheme="${mode}"]`) : {}) };
}
function luminance(hex) {
  assert.match(hex, /^#[\da-f]{6}$/i);
  const channels = hex.slice(1).match(/../g).map((value) => parseInt(value, 16) / 255)
    .map((value) => value <= .04045 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4);
  return .2126 * channels[0] + .7152 * channels[1] + .0722 * channels[2];
}
function contrast(a, b) {
  const values = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (values[0] + .05) / (values[1] + .05);
}

test('各主题明暗材料库文本与交互面板色值保持可读；不以系统媒体查询覆盖显式偏好', () => {
  for (const theme of ['shadcn', '8bitcn', 'minecraft', 'metro10', 'aero7']) for (const mode of ['light', 'dark']) {
    const tokens = palette(theme, mode);
    for (const [text, surface] of [['foreground', 'card'], ['muted-foreground', 'card'], ['muted-foreground', 'muted'], ['foreground', 'accent'], ['primary-foreground', 'primary']]) {
      const ratio = contrast(tokens[`--${text}`], tokens[`--${surface}`]);
      assert.ok(ratio >= 4.5, `${theme}/${mode} ${text}/${surface}: ${ratio.toFixed(2)}`);
    }
    assert.doesNotMatch(source(`ui/themes/${theme}/${theme === 'minecraft' ? 'appearance.css' : 'theme.css'}`), /@media\s*\(prefers-color-scheme/);
  }
});

test('卡片与分类不继承纹理按钮或 8bit 位移阴影，状态独立且焦点覆盖整卡', () => {
  // D-phase moved the "action button" identity onto `themeSlot` (`data-slot="library-tile"`),
  // so theme button textures never reach the tile host; `library.css` only pins layout here.
  const tile = rule(library, 'html[data-theme] .library-page .library-tile-open');
  assert.equal(tile.transform, 'none');
  assert.equal(tile.height, 'auto');
  assert.equal(tile.gap, '0');
  assert.equal(tile.padding, '0');
  assert.equal(rule(library, '.library-tile')['position'], 'relative');
  const category = rule(library, 'html[data-theme] .library-page .library-category');
  assert.equal(category.transform, 'none');
  const categoryRow = rule(library, '.library-page .library-category');
  assert.equal(categoryRow.height, 'auto');
  assert.equal(categoryRow.gap, '8px');
});

test('Minecraft 三栏与工具栏使用当前明暗令牌；预览和滚动条保留真实资源及像素边缘', () => {
  const theme = 'html[data-theme="minecraft"] ';
  for (const selector of ['.library-toolbar', '.library-categories', '.library-inspector', '.library-collection-toolbar', '.library-collection-footer', '.library-tile-image', '.library-detail-preview']) {
    assert.match(rule(minecraft, theme + selector).background, /var\(/, selector);
  }
  const tiles = rule(minecraft, theme + '.library-tile');
  assert.match(tiles['box-shadow'], /inset.*var\(/);
  const page = rule(minecraft, theme + '.library-page');
  assert.equal(page['--ln-scroll-radius'], '0');
  assert.match(page['--ln-scroll-track'], /var\(/);
  assert.match(rule(controls, '[data-slot="scroll-area-thumb"]')['background'], /var\(--ln-scroll-thumb\)/);
  assert.match(rule(controls, '[data-slot="scroll-area-thumb"]:active')['background'], /var\(--ln-scroll-active\)/);
  assert.match(rule(library, '.library-tile-image img')['object-fit'], /contain/);
  assert.doesNotMatch(source('ui/windows/main/pages/library/library.css'), /filter:\s*(?:invert|grayscale|sepia)/);
  assert.match(source('ui/themes/minecraft/widgets.css'), /button_normal_min\.png/);
  assert.match(source('ui/themes/minecraft/widgets.css'), /unifont-16\.0\.04\.ttf/);
});

test('三栏仍共享响应式结构，窄窗只保留外层纵向滚动；主题规则不重新固定列宽', () => {
  const breakpoints = [];
  library.walkAtRules('container', (entry) => breakpoints.push(entry.params));
  assert.ok(breakpoints.includes('library-width (max-width: 68rem)'));
  assert.ok(breakpoints.includes('library-width (max-width: 44rem)'));
  let narrow;
  library.walkAtRules('container', (entry) => { if (entry.params === 'library-width (max-width: 44rem)') narrow = entry; });
  assert.equal(rule(narrow, '.library-workspace').display, 'block');
  assert.equal(rule(narrow, '.library-grid-scroll').height, 'auto');
  assert.equal(rule(narrow, '.library-inspector-scroll').overflow, 'visible');
  minecraft.walkRules((entry) => {
    if (!entry.selector.includes('.library-')) return;
    entry.walkDecls((decl) => assert.notEqual(decl.prop, 'grid-template-columns', 'theme must not fork the responsive layout'));
  });
});
