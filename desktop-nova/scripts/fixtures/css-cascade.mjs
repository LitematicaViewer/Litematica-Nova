import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import postcss from 'postcss';

// Shared cascade model for the source/CSS contracts in scripts/*.test.mjs.
//
// These tests assert which declaration *wins* for a control, not merely which
// declarations exist: a theme rule that states the right value can still lose to a
// more specific shared rule, and that is exactly how the Minecraft radio and checkbox
// defects (docs/UI_ARCHITECTURE.md V1.7) and the missing button selected state were
// introduced while every existing check stayed green.
//
// Known limits, unchanged by design:
//   * `var()` values are not resolved, so a token indirection is compared by name.
//   * `:not(...)` is treated as a negative condition that the control under test never
//     satisfies; only its `[data-theme="..."]` part is honoured, because a rule pinned
//     to another theme can never apply.
//   * A rule must name the control's slot in its rightmost compound (or be a plain
//     element rule), otherwise it is assumed to target a descendant.

export const LAYER_ORDER = ['theme', 'vendor', 'tokens', 'base', 'components', 'utilities'];

// Theme rules live in `components.theme`, above the parent layer's shared/page
// defaults, so "a theme declaration takes effect" is structural rather than a
// specificity contest. Rules directly in `components` rank below every sublayer.
const SUBLAYER_ORDER = ['components.shared', 'components.page', 'components.theme'];

/** Numeric rank used for cascade comparison; sublayers sit inside their parent. */
export function layerRank(layer) {
  if (layer === null || layer === undefined) return LAYER_ORDER.length * 10;
  const sublayer = SUBLAYER_ORDER.indexOf(layer);
  if (sublayer >= 0) return LAYER_ORDER.indexOf('components') * 10 + 1 + sublayer;
  const index = LAYER_ORDER.indexOf(layer);
  return index >= 0 ? index * 10 : LAYER_ORDER.length * 10;
}

/** Splits a selector argument list on commas that are not nested. */
function splitArguments(text) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (character === '(' || character === '[') depth += 1;
    else if (character === ')' || character === ']') depth -= 1;
    else if (character === ',' && depth === 0) {
      parts.push(text.slice(start, index));
      start = index + 1;
    }
  }
  parts.push(text.slice(start));
  return parts;
}

/**
 * Expands `:is()`/`:where()` into selector alternatives. Both are supported everywhere
 * the app runs, and expanding them is what keeps this model honest: the alternative form
 * would have to be re-implemented here, and an approximation is how a rule such as
 * `:is([aria-pressed="true"], [data-state="on"])` gets counted as matching a resting
 * button as well.
 */
export function expandSelector(selector) {
  let parts = [selector];
  for (let guard = 0; guard < 16; guard += 1) {
    let expanded = false;
    const next = [];
    for (const part of parts) {
      const match = /:(?:is|where)\(([^()]*)\)/.exec(part);
      if (!match) {
        next.push(part);
        continue;
      }
      expanded = true;
      for (const argument of splitArguments(match[1])) {
        next.push(part.slice(0, match.index) + argument.trim() + part.slice(match.index + match[0].length));
      }
    }
    parts = next;
    if (!expanded) break;
  }
  return parts;
}

export function specificity(selector) {
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

export function layerOf(rule) {
  for (let parent = rule.parent; parent; parent = parent.parent) {
    if (parent.type === 'atrule' && parent.name === 'layer' && parent.params && parent.nodes) return parent.params.trim();
  }
  return null;
}

/**
 * Parses stylesheets in load order and flattens them into one ranked rule list.
 * `files` must be given in the order the app loads them: shared layer, page CSS, theme.
 */
export function collect(files, root) {
  const rules = [];
  let order = 0;
  for (const path of files) {
    const css = postcss.parse(readFileSync(new URL(path, root), 'utf8'), { from: path });
    css.walkRules((rule) => {
      for (const selector of rule.selectors) {
        for (const expanded of expandSelector(selector)) {
          rules.push({
            path,
            selector: expanded,
            decls: rule.nodes.filter((node) => node.type === 'decl'),
            layer: layerOf(rule),
            order: order++,
          });
        }
      }
    });
  }
  return rules;
}

/** The theme a selector is pinned to, or `undefined` when it never mentions one. */
export function themeOf(selector) {
  const match = /\[data-theme(?:="([^"]*)")?\]/.exec(selector);
  if (!match) return undefined;
  return match[1] ?? null;
}

/** `:not([data-theme="x"])` disqualifies the rule for theme `x`. */
function excludedThemes(selector) {
  const excluded = new Set();
  for (const match of selector.matchAll(/:not\(\s*\[data-theme="([^"]*)"\]\s*\)/g)) {
    excluded.add(match[1]);
  }
  return excluded;
}

function appliesToTheme(selector, themeId) {
  if (excludedThemes(selector).has(themeId)) return false;
  const theme = themeOf(selector);
  return theme === undefined || theme === null || theme === themeId;
}

/**
 * Does one selector compound satisfy a `{ slot, attrs, pseudos }` description?
 *
 * Mirrors the rightmost-compound test inside `winner`, so an ancestor compound can be
 * checked with the same rules: a constrained attribute the description does not carry
 * disqualifies, and the compound may only name states the description claims hold.
 * `:is()`/`:where()`/`:not()` are stripped exactly as `winner` does (see the file header
 * for why `:not()` is treated as always-true).
 */
function compoundMatches(compound, spec) {
  const bare = compound.replace(/:(?:not|is|where)\([^)]*\)/g, ' ');
  const slots = [...bare.matchAll(/\[data-slot="([^"]+)"\]/g)].map((match) => match[1]);
  if (spec.slot !== undefined && !slots.includes(spec.slot)) return false;
  const attrs = [...bare.matchAll(/\[([\w-]+)(?:="([^"]*)")?\]/g)]
    .filter(([, name]) => name !== 'data-slot')
    .map(([, name, value]) => [name, value]);
  if (attrs.some(([name, value]) => {
    const declared = (spec.attrs ?? {})[name];
    if (declared === undefined) return true;
    return value !== undefined && declared !== value;
  })) return false;
  const states = [...bare.matchAll(/:([a-z-]+)/g)].map((match) => match[1]);
  if (states.some((state) => !(spec.pseudos ?? []).includes(state))) return false;
  return true;
}

/**
 * The declaration that wins `property` for a control, ranked by layer, specificity then
 * order.
 *
 * `control` is `{ slot, tag, pseudos, attrs, classes }`. `pseudos` lists the state being
 * asked about (e.g. `['hover']`); a rule carrying any other state cannot apply. `attrs`
 * lists the attributes the control carries: a rule may only constrain attributes that
 * are present with the same value, while a constraint the control does not carry at all
 * disqualifies the rule.
 *
 * `control.ancestors` is an optional escape hatch for rules whose *rightmost* compound is
 * a different element than the state that drives it - the Aero7 motion host
 * `[data-slot="button-motion-layer"]` is lit by its ancestral button's
 * `[aria-pressed]`/`:hover`/`:disabled`. When present, every listed `{ slot, attrs,
 * pseudos }` must be satisfied by one of the compounds left of the tail. It is opt-in and
 * leaves the default rightmost-compound behaviour untouched, so existing contracts that
 * never set it are unaffected.
 */
export function winner(rules, control, property, themeId = 'minecraft') {
  const marker = `[data-slot="${control.slot}"]`;
  const wanted = control.pseudos ?? [];
  let best = null;
  for (const rule of rules) {
    // Only the rightmost compound may introduce the slot, or the rule targets a child.
    const tail = rule.selector.split(/\s+|>/).pop();
    const bare = tail.replace(/:(?:not|is|where)\([^)]*\)/g, ' ');
    const slots = [...bare.matchAll(/\[data-slot="([^"]+)"\]/g)].map((match) => match[1]);
    if (slots.some((slot) => slot !== control.slot)) continue;
    const named = bare.includes(marker);
    const element = new RegExp(`(^|[^\\w-])${control.tag}(?![-\\w])`).test(bare);
    if (!named && !element) continue;
    // A class in the rightmost compound means the rule targets a differently-classed
    // element; `button.flake-page__quickbar-slot` can never match a dropdown trigger.
    const classes = [...bare.matchAll(/\.([\w-]+)/g)].map((match) => match[1]);
    if (classes.some((name) => !(control.classes ?? []).includes(name))) continue;
    // Likewise for any attribute beyond the slot itself: `button[aria-pressed="true"]`
    // does not apply to a control that never carries that attribute. A presence-only
    // constraint (`[data-state]`) just requires the control to carry the attribute.
    const attrs = [...bare.matchAll(/\[([\w-]+)(?:="([^"]*)")?\]/g)]
      .filter(([, name]) => name !== 'data-slot')
      .map(([, name, value]) => [name, value]);
    if (attrs.some(([name, value]) => {
      const declared = (control.attrs ?? {})[name];
      if (declared === undefined) return true;
      return value !== undefined && declared !== value;
    })) continue;
    const states = [...bare.matchAll(/:([a-z-]+)/g)].map((match) => match[1]);
    if (states.some((state) => !wanted.includes(state))) continue;
    // Opt-in: the driving state may live on an ancestor compound. Only then is the full
    // selector split; the default path above stays byte-for-byte the same.
    if (control.ancestors) {
      const compounds = rule.selector.split(/\s+|>/).filter(Boolean);
      const heads = compounds.slice(0, -1);
      const satisfied = control.ancestors.every((ancestor) =>
        heads.some((compound) => compoundMatches(compound, ancestor)),
      );
      if (!satisfied) continue;
    }
    // A selector pinned to another theme can never apply here. `[data-theme]` without a
    // value means "any theme" and must stay eligible, or every shared-layer rule is
    // filtered out and the assertions pass against the theme's own rules alone.
    if (!appliesToTheme(rule.selector, themeId)) continue;
    const declaration = rule.decls.find((node) => node.prop === property);
    if (!declaration) continue;
    const rank = {
      layer: layerRank(rule.layer),
      specificity: specificity(rule.selector),
      order: rule.order,
    };
    const better =
      best === null ||
      rank.layer > best.rank.layer ||
      (rank.layer === best.rank.layer && rank.specificity > best.rank.specificity) ||
      (rank.layer === best.rank.layer && rank.specificity === best.rank.specificity && rank.order > best.rank.order);
    if (better) best = { decl: declaration, rank, selector: rule.selector, path: rule.path };
  }
  assert.ok(best, `no rule sets ${property} for [data-slot="${control.slot}"]`);
  return best;
}
