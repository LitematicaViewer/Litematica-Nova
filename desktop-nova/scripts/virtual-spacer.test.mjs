import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import postcss from 'postcss';

// Source contracts only: no browser, DOM emulation, or visual acceptance.
//
// A virtual-list spacer that fails to take its height makes the scroll container's
// total height follow the number of *rendered* rows instead of the full list. The
// container then clamps scrollTop, the scroll listener feeds the clamped value back
// into state, and the view ping-pongs between two offsets. That regression shipped
// twice because the contract was only grepped as text: the rule kept saying
// `height: var(...)` while losing the cascade to `.material-list-table td`'s 40px.
// The checks below therefore resolve the cascade (importance -> layer -> specificity
// -> order) over the stylesheets each window really loads, and pin the other halves
// of a working virtual list: data-keyed zebra stripes and `rowHeight` constants that
// match the height the browser lays out.
const root = new URL('../', import.meta.url);
const source = (file) => readFileSync(new URL(file, root), 'utf8');
const parse = (file) => postcss.parse(source(file), { from: file });

const SPACER_VARIABLE = '--material-list-virtual-spacer-height';
const LAYER_ORDER = ['theme', 'vendor', 'tokens', 'base', 'components', 'utilities'];
const compact = (value) => value.replace(/\s+/g, '');
const px = (value) => Number(/(\d+(?:\.\d+)?)px/.exec(value ?? '')?.[1]);

/** a = ids, b = classes/attributes/pseudo-classes, c = types. */
function specificity(selector) {
  const cleaned = selector
    // `:where()` always contributes zero, whatever it wraps.
    .replace(/:where\([^()]*\)/g, '')
    .replace(/:not\(([^()]*)\)/g, ' $1 ')
    .replace(/:is\(([^()]*)\)/g, ' $1 ')
    .replace(/::[a-z-]+(\([^()]*\))?/g, '');
  const ids = (cleaned.match(/#[\w-]+/g) ?? []).length;
  const classes = (cleaned.match(/\.[\w-]+/g) ?? []).length;
  const attributes = (cleaned.match(/\[[^\]]+\]/g) ?? []).length;
  const pseudo = (cleaned.match(/:(?!:)[\w-]+(\([^()]*\))?/g) ?? []).length;
  const types = (cleaned.match(/(?:^|[\s>+~,(])([a-zA-Z][\w-]*)/g) ?? []).length;
  return { a: ids, b: classes + attributes + pseudo, c: types };
}

const specificityKey = ({ a, b, c }) => (a << 20) + (b << 10) + c;

function layerOf(rule) {
  const layers = [];
  for (let node = rule.parent; node && node.type !== 'root'; node = node.parent) {
    if (node.type === 'atrule' && node.name === 'layer' && node.params) layers.unshift(node.params);
  }
  return layers.length ? layers.join('.') : null;
}

/** Follows relative `@import`s so the list mirrors the window's real load order. */
function loadChain(entry, seen = new Set(), out = []) {
  if (seen.has(entry)) return out;
  seen.add(entry);
  parse(entry).walkAtRules('import', (atRule) => {
    const relative = /^["'](\.{1,2}\/[^"']+)["']/.exec(atRule.params.trim())?.[1];
    if (!relative) return;
    loadChain(path.posix.normalize(path.posix.join(path.posix.dirname(entry), relative)), seen, out);
  });
  out.push(entry);
  return out;
}

/** Flattens the given stylesheets into ordered single-selector declarations. */
function declarationsOf(sheets) {
  const entries = [];
  let order = 0;
  for (const sheet of sheets) {
    parse(sheet).walkRules((rule) => {
      order += 1;
      for (const decl of rule.nodes.filter((node) => node.type === 'decl')) {
        for (const selector of rule.selectors) {
          entries.push({
            sheet,
            selector: selector.trim(),
            layer: layerOf(rule),
            property: decl.prop.toLowerCase(),
            value: decl.value,
            important: Boolean(decl.important),
            specificity: specificity(selector),
            order,
          });
        }
      }
    });
  }
  return entries;
}

const strength = (entry) => {
  const index = entry.layer === null ? LAYER_ORDER.length : LAYER_ORDER.indexOf(entry.layer);
  const rank = index === -1 ? LAYER_ORDER.length : index;
  return {
    important: entry.important ? 1 : 0,
    // Important declarations reverse the layer order; unlayered important is the weakest.
    layer: entry.important ? -rank : rank,
    specificity: specificityKey(entry.specificity),
    order: entry.order,
  };
};

const beats = (left, right) => {
  const a = strength(left);
  const b = strength(right);
  if (a.important !== b.important) return a.important > b.important;
  if (a.layer !== b.layer) return a.layer > b.layer;
  if (a.specificity !== b.specificity) return a.specificity > b.specificity;
  return a.order > b.order;
};

/** Last compound of a selector: which element does this rule style? */
function subjectOf(selector) {
  return (
    selector
      .replace(/:(?:not|has|is|where)\([^()]*(?:\([^()]*\)[^()]*)*\)/g, '')
      .split(/\s*[>+~]\s*|\s+/)
      .filter(Boolean)
      .pop() ?? ''
  ).trim();
}

function subjectTargets(selector, element) {
  const subject = subjectOf(selector);
  if (!subject) return false;
  if (subject === '*') return true;
  if (element.classes.some((name) => subject.includes(`.${name}`))) return true;
  return new RegExp(`(^|[\\s.#[:(])${element.tag}(?![\\w-])`).test(subject);
}

/** A selector that excludes the spacer on purpose cannot clamp it. */
const excludesSpacer = (selector) =>
  /:not\(:has\(\.material-list-virtual-spacer-cell\)\)/.test(selector) ||
  /:not\(\.material-list-virtual-spacer-cell\)/.test(selector) ||
  /:not\(\.material-virtual-spacer\)/.test(selector);

const THEME_SHEETS = ['ui/themes/modern.css', 'ui/themes/minecraft/widgets.css', 'ui/themes/8bitcn/theme.css'];
const STATISTICS = 'ui/windows/main/pages/statistics/materials.css';

const SURFACES = [
  {
    label: '材料明细表（统计页对话框 / material_list 窗口）',
    stylesheet: STATISTICS,
    spacerSelector: '.material-list-table tbody tr > .material-list-virtual-spacer-cell',
    element: { tag: 'td', classes: ['material-list-virtual-spacer-cell'] },
    loads: ['ui/styles/base.css', STATISTICS],
  },
  {
    label: '枚举器左 / 右值表',
    stylesheet: 'ui/styles/enumerator.css',
    spacerSelector: '.enumerator-window .material-list-virtual-spacer-cell',
    element: { tag: 'td', classes: ['material-list-virtual-spacer-cell'] },
    loads: ['ui/styles/base.css', 'ui/styles/enumerator.css'],
  },
  {
    label: '材料汇总列表 <ul>',
    stylesheet: STATISTICS,
    spacerSelector: '.material-summary-list > .material-virtual-spacer',
    element: { tag: 'li', classes: ['material-virtual-spacer'] },
    loads: ['ui/styles/base.css', STATISTICS],
  },
];

test('虚拟列表占位元素的高度由样式表声明，组件只提供自定义属性值', () => {
  const component = source('ui/components/VirtualSpacerCell.tsx');
  const property = /VIRTUAL_SPACER_HEIGHT_PROPERTY\s*=\s*"(--[a-z0-9-]+)"/.exec(component)?.[1];
  assert.equal(property, SPACER_VARIABLE, 'VirtualSpacerCell.tsx must define the spacer custom property name');

  // The value travels through the custom property; markup never declares a style
  // property (`ui-no-fixed-inline-style`).
  assert.match(component, /style\.setProperty\(\s*VIRTUAL_SPACER_HEIGHT_PROPERTY/, 'the spacer must set its custom property imperatively');
  assert.doesNotMatch(component, /style=\{\{/, 'the spacer must not declare a style property in markup');
});

test('占位高度必须在每个表实际加载的样式表中赢得层叠', () => {
  for (const surface of SURFACES) {
    const sheets = [...loadChain(surface.loads[0]), ...surface.loads.slice(1), ...THEME_SHEETS];
    const heights = declarationsOf(sheets).filter((entry) => entry.property === 'height');

    const spacers = heights.filter((entry) => compact(entry.selector) === compact(surface.spacerSelector));
    assert.equal(spacers.length, 1, `${surface.stylesheet} must declare ${surface.spacerSelector} exactly once`);
    const spacer = spacers[0];
    assert.equal(
      compact(spacer.value),
      compact(`var(${SPACER_VARIABLE}, 0px)`),
      `${surface.spacerSelector} must declare height: var(${SPACER_VARIABLE}, 0px)`,
    );

    const competitors = heights.filter(
      (entry) =>
        compact(entry.selector) !== compact(surface.spacerSelector) &&
        !excludesSpacer(entry.selector) &&
        subjectTargets(entry.selector, surface.element),
    );

    for (const competitor of competitors) {
      assert.ok(
        beats(spacer, competitor),
        `${surface.label}: ${surface.spacerSelector} loses the cascade to ` +
          `${competitor.sheet} \`${competitor.selector} { height: ${competitor.value}${competitor.important ? ' !important' : ''} }\`` +
          ` (spacer spec (${spacer.specificity.a},${spacer.specificity.b},${spacer.specificity.c}), layer ${spacer.layer}, important ${spacer.important})`,
      );
    }
  }
});

test('虚拟化行列的斑马纹跟随数据行序，不跟随 DOM 位置', () => {
  const page = source('ui/windows/main/pages/statistics/StatisticsPage.tsx');
  assert.equal(
    (page.match(/rowParityClass\(virtual\.startIndex \+ index\)/g) ?? []).length,
    2,
    'both virtualized branches (summary <li> and detail <tr>) must key the parity on the absolute data index',
  );
  assert.match(page, /material-list-row-odd/);
  assert.match(page, /material-list-row-even/);

  const enumerator = source('ui/windows/enumerator/EnumeratorWindow.tsx');
  assert.ok(/(leftStartIndex|rightStartIndex) \+ index\) % 2/.test(enumerator), 'enumerator rows keep a data-keyed parity');

  // The stripes are keyed on the parity class and must outrank the positional fallback...
  const backgrounds = declarationsOf([...loadChain('ui/styles/base.css'), STATISTICS]).filter(
    (entry) => entry.property === 'background' || entry.property === 'background-color',
  );
  const positional = backgrounds.find(
    (entry) => /:nth-child\(odd\)/.test(entry.selector) && entry.selector.includes('.material-list-table'),
  );
  assert.ok(positional, 'a positional stripe is still needed for the non-virtualized collection table');

  const oddStripe = backgrounds.find((entry) => entry.selector === '.material-list-table .material-list-row-odd');
  const evenReset = backgrounds.find((entry) => entry.selector === '.material-list-table .material-list-row-even');
  assert.ok(oddStripe, 'subwindow.css must stripe `.material-list-table .material-list-row-odd`');
  assert.ok(evenReset, 'subwindow.css must reset `.material-list-table .material-list-row-even`');
  assert.ok(beats(oddStripe, positional), 'the data-keyed stripe must outrank the positional one');
  assert.ok(beats(evenReset, positional), 'even-parity rows must not inherit the positional stripe');

  // ...and the enumerator scope must outrank that same fallback.
  const enumeratorCss = declarationsOf(['ui/styles/enumerator.css']);
  const enumeratorStripe = enumeratorCss.find(
    (entry) => entry.property === 'background' && entry.selector.includes('.material-list-row-odd'),
  );
  assert.ok(enumeratorStripe, 'enumerator.css must stripe its virtualized rows by data parity');
  assert.ok(beats(enumeratorStripe, positional), 'the enumerator stripe must outrank the positional one');

  // The summary list is virtualized in both branches, so it may not stripe by DOM position at all.
  const summaryStripe = backgrounds.find(
    (entry) => entry.selector.includes('.material-summary-list') && entry.selector.includes('.material-list-row-odd'),
  );
  assert.ok(summaryStripe, 'materials.css must stripe the summary list through `.material-list-row-odd`');
  assert.equal(
    backgrounds.filter((entry) => entry.selector.includes('.material-summary-list') && /:nth-child/.test(entry.selector)).length,
    0,
    'the summary list must not stripe by DOM position',
  );
});

test('rowHeight 常量与样式表实际行高一致', () => {
  const iconHeight = px(
    declarationsOf(['ui/styles/base.css']).find(
      (entry) => entry.property === 'height' && entry.selector.includes('.block-icon-fallback'),
    )?.value,
  );
  assert.equal(iconHeight, 32, 'the block icon is the row-height baseline');

  const paddingOf = (sheet, selector) =>
    px(
      declarationsOf([sheet]).find(
        (entry) => entry.property === 'padding' && compact(entry.selector) === compact(selector),
      )?.value,
    );
  const listPadding = paddingOf('ui/shell/widget/subwindow.css', '.material-list-table td');
  const detailPadding = paddingOf(STATISTICS, '.beta-material-table td');
  const summaryHeight = declarationsOf([STATISTICS])
    .filter((entry) => entry.property === 'max-height' && entry.selector.includes('.material-summary-list'))
    .map((entry) => px(entry.value))
    .find((value) => Number.isFinite(value));
  assert.equal(listPadding, 6);
  assert.equal(detailPadding, 9);

  // 32px icon + 2x vertical padding + the 1px collapsed border between rows.
  const detailRow = iconHeight + 2 * detailPadding + 1;
  const enumeratorRow = iconHeight + 2 * listPadding + 1;
  assert.equal(detailRow, 51);
  assert.equal(enumeratorRow, 45);
  assert.equal(summaryHeight, 48);

  const statisticsCall = /useVirtualWindow\(materials\s*,\s*scrollRef\s*,\s*breakdown\s*\?\s*(\d+)\s*:\s*(\d+)\s*,\s*(\d+)\s*\)/.exec(
    source('ui/windows/main/pages/statistics/StatisticsPage.tsx'),
  );
  assert.ok(statisticsCall, 'MaterialTable must pass the row height per branch');
  assert.deepEqual(
    statisticsCall.slice(1).map(Number),
    [detailRow, summaryHeight, 8],
    `MaterialTable must pass ${detailRow} for detail rows and ${summaryHeight} for summary rows`,
  );

  const enumeratorCalls = [
    ...source('ui/windows/enumerator/EnumeratorWindow.tsx').matchAll(
      /useVirtualWindow\(\s*(?:filteredLeftRows|filteredRightRows)\s*,\s*(?:leftValueTableRef|rightValueTableRef)\s*,\s*(\d+)\s*,\s*(\d+)\s*\)/g,
    ),
  ];
  assert.equal(enumeratorCalls.length, 2, 'both enumerator tables are virtualized');
  for (const call of enumeratorCalls) {
    assert.deepEqual(call.slice(1).map(Number), [enumeratorRow, 10], `enumerator rows are laid out ${enumeratorRow}px tall`);
  }
});

test('虚拟滚动容器退出浏览器滚动锚定并固定滚动条沟槽', () => {
  const scrollers = [
    [STATISTICS, '.material-table-scroll'],
    ['ui/styles/enumerator.css', '.enumerator-value-table-wrap'],
  ];
  for (const [sheet, selector] of scrollers) {
    const entries = declarationsOf([sheet]).filter((entry) => entry.selector === selector);
    assert.ok(
      entries.some((entry) => entry.property === 'overflow-anchor' && entry.value === 'none'),
      `${sheet} ${selector} must set overflow-anchor: none`,
    );
    assert.ok(
      entries.some((entry) => entry.property === 'scrollbar-gutter'),
      `${sheet} ${selector} must set scrollbar-gutter`,
    );
  }
});
