import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import postcss from 'postcss';

// Source contracts only: no browser, DOM emulation, or visual acceptance.
//
// A virtual-list spacer that fails to take its height makes the scroll
// container's total height follow the number of *rendered* rows instead of the
// full list. The container then clamps scrollTop, the scroll listener feeds the
// clamped value back into state, and the view oscillates between two offsets.
// That regression already shipped once, so the two halves of the contract - the
// custom property the component publishes and the `height` rule the stylesheet
// owns - are pinned here, including the property name that joins them.
const root = new URL('../', import.meta.url);
const source = (path) => readFileSync(new URL(path, root), 'utf8');
const parse = (path) => postcss.parse(source(path), { from: path });

function declarationsOf(css, selector) {
  const declarations = [];
  css.walkRules((entry) => {
    if (entry.selectors.includes(selector)) {
      declarations.push(...entry.nodes.filter((node) => node.type === 'decl'));
    }
  });
  assert.ok(declarations.length, `missing CSS contract: ${selector}`);
  return Object.fromEntries(declarations.map(({ prop, value }) => [prop, value]));
}

const compact = (value) => value.replace(/\s+/g, '');

test('虚拟列表占位元素的高度由样式表声明，组件只提供自定义属性值', () => {
  const component = source('ui/components/VirtualSpacerCell.tsx');
  const property = /VIRTUAL_SPACER_HEIGHT_PROPERTY\s*=\s*"(--[a-z0-9-]+)"/.exec(component)?.[1];
  assert.ok(property, 'VirtualSpacerCell.tsx must define the spacer custom property name');

  // The component must publish the value through the custom property, never by
  // declaring a style property in markup (`ui-no-fixed-inline-style`).
  assert.match(component, /style\.setProperty\(\s*VIRTUAL_SPACER_HEIGHT_PROPERTY/, 'the spacer must set its custom property imperatively');
  assert.doesNotMatch(component, /style=\{\{/, 'the spacer must not declare a style property in markup');

  const expected = compact(`var(${property}, 0px)`);
  const surfaces = [
    // Statistics summary list (the virtualized <ul> branch).
    ['ui/windows/main/pages/statistics/materials.css', '.material-summary-list > .material-virtual-spacer'],
    // Statistics detail table.
    ['ui/windows/main/pages/statistics/materials.css', '.material-list-virtual-spacer-cell'],
    // Enumerator window tables.
    ['ui/styles/enumerator.css', '.enumerator-window .material-list-virtual-spacer-cell'],
  ];

  for (const [file, selector] of surfaces) {
    const declarations = declarationsOf(parse(file), selector);
    assert.equal(
      compact(declarations.height ?? ''),
      expected,
      `${file} ${selector} must declare height: var(${property}, 0px) so the spacer keeps the full scroll height`,
    );
  }
});

test('统计页两个占位元素都由组件渲染，不内联高度', () => {
  const page = source('ui/windows/main/pages/statistics/StatisticsPage.tsx');
  assert.match(page, /import\s*\{[^}]*VirtualSpacerItem[^}]*\}\s*from\s*"[^"]*VirtualSpacerCell"/);
  assert.equal(
    (page.match(/<VirtualSpacerItem\b/g) ?? []).length,
    2,
    'both summary-list spacers must render VirtualSpacerItem',
  );
  assert.doesNotMatch(page, /style=\{\{\s*height/, 'the statistics page must not inline spacer heights');
});

test('枚举器窗口的占位单元格同样不含内联样式', () => {
  const enumerator = source('ui/windows/enumerator/EnumeratorWindow.tsx');
  assert.equal((enumerator.match(/<VirtualSpacerCell\b/g) ?? []).length, 4);
  assert.doesNotMatch(enumerator, /style=\{\{\s*height/, 'the enumerator window must not inline spacer heights');
});
