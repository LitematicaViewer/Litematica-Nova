import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
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

/** The declaration that wins `property` for a Minecraft control, ranked by layer, specificity, then order. */
function winner(rules, slot, property) {
  const marker = `[data-slot="${slot}"]`;
  let best = null;
  for (const rule of rules) {
    // Only the rightmost compound may introduce the slot, or the rule targets a child.
    const tail = rule.selector.split(/\s+|>/).pop();
    if (!tail.includes(marker)) continue;
    // A selector pinned to another theme can never apply here. Without this filter a
    // rule such as 8bitcn's `border-radius: 0` silently satisfies a Minecraft question.
    const theme = themeOf(rule.selector);
    if (theme !== undefined && theme !== "minecraft") continue;
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
  assert.ok(best, `no rule sets ${property} for [data-slot="${slot}"]`);
  return best;
}

const rules = collect(['ui/styles/controls.css', 'ui/themes/minecraft/appearance.css']);

test('the cascade filter rejects rules pinned to another theme', () => {
  // Guards the guard: without this the radio assertion passes on 8bitcn's rule.
  assert.equal(themeOf('html[data-theme="8bitcn"] [data-slot="radio-group-item"]'), '8bitcn');
  assert.equal(themeOf('html[data-theme="minecraft"] button[data-slot="checkbox"]'), 'minecraft');
  assert.equal(themeOf('html[data-theme] [data-slot="checkbox"]'), null);
  assert.equal(themeOf('[data-slot="checkbox"]'), undefined);
});

test('Minecraft 单选框贴图不被共享层的圆形裁切', () => {
  const rounded = winner(rules, 'radio-group-item', 'border-radius');
  assert.equal(rounded.decl.value, '0', `[data-choice-layout="control"] is clipped by ${rounded.selector} in ${rounded.path}`);
});

test('Minecraft 复选框贴图中心不被共享层的 --primary 填充', () => {
  const fill = winner(rules, 'checkbox', 'background-color');
  assert.equal(fill.decl.value, 'transparent', `the checked fill leaks through from ${fill.selector} in ${fill.path}`);
});

test('共享层只声明填充色，不重置贴图', () => {
  // A `background` shorthand here would reset `background-image`, which is how the
  // theme ended up out-specifying the image while still losing the colour.
  for (const rule of rules.filter((entry) => entry.path === 'ui/styles/controls.css')) {
    if (!rule.selector.includes('[data-slot="checkbox"]')) continue;
    assert.ok(!rule.decls.some((node) => node.prop === 'background'), `${rule.selector} resets background-image`);
  }
});
