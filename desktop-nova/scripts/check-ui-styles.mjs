#!/usr/bin/env node

// UI style guardrail for the theme migration described in docs/UI_ARCHITECTURE.md.
//
// The baseline records one count per (rule, file) rather than exact file+line
// records. Phase 1 moves every CSS rule into a cascade layer and Phase 3 rewrites
// the component class lists, so line-exact records would flip en masse between
// "resolved" and "new" on every commit. A per-file count expresses the property
// that actually matters across the migration: no file may gain findings of a rule,
// and every count must reach zero. Run `--mode=strict` to list exact locations.
//
// Run modes mirror scripts/check-layer-boundaries.mjs.

import fs from "node:fs";
import path from "node:path";
import ts from "typescript";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const defaultRootDir = path.resolve(scriptDir, "..");
const BASELINE_SCHEMA = "lba.ui-style-violations-baseline.v1";
const LAYER_ORDER = ["theme", "tokens", "base", "components", "utilities"];
const SKIPPED_DIRECTORIES = new Set(["node_modules", "dist", ".vite", "target", ".tmp"]);

// Elements that already publish a Nova data-slot contract. Styling them by element
// type inside a cascade layer bypasses the slot contract themes hook into.
const SLOT_BACKED_ELEMENTS = new Set([
  "button",
  "input",
  "select",
  "textarea",
  "table",
  "thead",
  "tbody",
  "tfoot",
  "tr",
  "th",
  "td",
]);

// Tailwind appearance utilities. Layout and structure utilities are deliberately
// absent: components and pages are still allowed to own spacing, flow and sizing.
// Appearance belongs to the theme files.
const APPEARANCE_UTILITY_PATTERNS = [
  /^bg-/,
  /^text-(?!(?:left|center|right|justify|start|end|wrap|nowrap|ellipsis|clip|balance|pretty)$)/,
  /^border$/,
  /^border-(?!(?:collapse|spacing)$)/,
  /^rounded(?:-|$)/,
  /^shadow(?:-|$)/,
  /^ring(?:-|$)/,
  // `outline-none` and `outline-hidden` are focus-ring resets that components keep
  // next to their accessibility hooks, so only width and colour variants count.
  /^outline$/,
  /^outline-(?!(?:none|hidden)$)/,
  /^font-(?:thin|extralight|light|normal|medium|semibold|bold|extrabold|black|sans|serif|mono)$/,
  /^opacity-/,
  /^divide-(?!(?:x|y)$)/,
  /^fill-/,
  /^stroke-/,
  /^(?:from|via|to)-/,
  /^decoration-/,
  /^placeholder-/,
  /^caret-/,
  /^accent-/,
  /^drop-shadow-/,
  /^leading-/,
  /^tracking-/,
];

const LEGACY_BUTTON_VARIANT_RE = /^btn-[a-z0-9-]+$/;

function parseArguments(argv) {
  const options = {
    mode: "baseline",
    rootDir: defaultRootDir,
    baselinePath: path.join(scriptDir, "ui-styles-baseline.json"),
    writeBaseline: false,
    printBaseline: false,
    report: false,
  };

  for (const argument of argv) {
    if (argument === "--write-baseline") {
      options.writeBaseline = true;
    } else if (argument === "--print-baseline") {
      options.printBaseline = true;
    } else if (argument === "--report") {
      options.report = true;
    } else if (argument.startsWith("--mode=")) {
      options.mode = argument.slice("--mode=".length);
    } else if (argument.startsWith("--root-dir=")) {
      options.rootDir = path.resolve(argument.slice("--root-dir=".length));
    } else if (argument.startsWith("--baseline=")) {
      options.baselinePath = path.resolve(argument.slice("--baseline=".length));
    } else {
      throw new Error(`Unknown UI style check argument: ${argument}`);
    }
  }

  if (options.mode !== "strict" && options.mode !== "baseline") {
    throw new Error(`Invalid UI style check mode ${JSON.stringify(options.mode)}; expected strict or baseline.`);
  }
  return options;
}

// Findings are reported as (rule, file) counts. `violations` keeps the exact
// locations for strict mode and the report; `unlayeredRuleCounts` holds the one
// rule that is cheaper to count directly than to enumerate.
const UNLAYERED_RULE = "css-unlayered-rule";

// Scanning state for the current collectFindings() call. The detectors share it
// through addViolation so each one stays a small, focused function.
let activeReport = null;

function toPosix(filePath) {
  return filePath.split(path.sep).join("/");
}

function relativePath(filePath) {
  return toPosix(path.relative(activeReport.rootDir, filePath));
}

function addViolation(filePath, line, rule, message) {
  const violation = { file: relativePath(filePath), line, rule, message };
  const key = JSON.stringify([violation.file, violation.line, violation.rule, violation.message]);
  if (activeReport.violationKeys.has(key)) {
    return;
  }
  activeReport.violationKeys.add(key);
  activeReport.violations.push(violation);
}

function compareViolations(left, right) {
  return (
    left.file.localeCompare(right.file) ||
    left.line - right.line ||
    left.rule.localeCompare(right.rule) ||
    left.message.localeCompare(right.message)
  );
}

function walk(dir, extensions) {
  if (!fs.existsSync(dir)) {
    return [];
  }

  const files = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isSymbolicLink()) {
      continue;
    }
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!SKIPPED_DIRECTORIES.has(entry.name.toLowerCase())) {
        files.push(...walk(fullPath, extensions));
      }
    } else if (extensions.has(path.extname(entry.name).toLowerCase())) {
      files.push(fullPath);
    }
  }
  return files;
}

function lineNumberAt(text, index) {
  let line = 1;
  for (let cursor = 0; cursor < index; cursor += 1) {
    if (text.charCodeAt(cursor) === 10) {
      line += 1;
    }
  }
  return line;
}

// ---------------------------------------------------------------------------
// CSS scanning
// ---------------------------------------------------------------------------

function blankRange(characters, source, start, end) {
  for (let index = start; index < end; index += 1) {
    if (source[index] !== "\r" && source[index] !== "\n") {
      characters[index] = " ";
    }
  }
}

// Comments and string bodies are blanked so that braces and semicolons inside
// them never confuse the block scanner. Offsets and line breaks are preserved,
// so reported line numbers still point at the original text. Attribute selector
// values are blanked too, which is harmless: the guardrails only need to know
// that `[data-slot` is present, never what it equals.
function maskCssText(source) {
  const characters = source.split("");
  let index = 0;

  while (index < source.length) {
    if (source[index] === "/" && source[index + 1] === "*") {
      const end = source.indexOf("*/", index + 2);
      const stop = end < 0 ? source.length : end + 2;
      blankRange(characters, source, index, stop);
      index = stop;
      continue;
    }

    if (source[index] === '"' || source[index] === "'") {
      const quote = source[index];
      let cursor = index + 1;
      while (cursor < source.length) {
        if (source[cursor] === "\\") {
          cursor += 2;
        } else if (source[cursor] === quote) {
          cursor += 1;
          break;
        } else {
          cursor += 1;
        }
      }
      blankRange(characters, source, index, cursor);
      index = cursor;
      continue;
    }

    index += 1;
  }

  return characters.join("");
}

// Walks a stylesheet once and returns every style rule together with the cascade
// layer it lives in. Nested at-rules (@media, @supports) are transparent for
// layer purposes; at-rules with non-selector bodies (@keyframes, @font-face,
// @theme) are opaque and never produce rules.
function scanCss(source) {
  const masked = maskCssText(source);
  const rules = [];
  const reducedMotionSpans = [];
  const stack = [];
  let buffer = "";
  let bufferStart = -1;
  let index = 0;

  const layerPath = () => {
    for (let depth = stack.length - 1; depth >= 0; depth -= 1) {
      if (stack[depth].kind === "layer") {
        return stack[depth].path;
      }
    }
    return null;
  };

  while (index < masked.length) {
    const character = masked[index];

    if (character === "{") {
      const prelude = buffer.trim();
      const start = bufferStart < 0 ? index : bufferStart;
      const insideOpaque = stack.some((entry) => entry.kind === "opaque");

      if (/^@layer\b/.test(prelude)) {
        const name = prelude.replace(/^@layer\b/, "").trim();
        const parent = layerPath();
        stack.push({
          kind: "layer",
          name,
          path: parent ? `${parent}.${name}` : name,
          start: index,
        });
      } else if (/^@(?:media|supports|container|scope|document|layer)\b/.test(prelude)) {
        stack.push({ kind: "at", start: index, reducedMotion: /prefers-reduced-motion/.test(prelude) });
      } else if (/^@/.test(prelude)) {
        stack.push({ kind: "opaque", start: index });
      } else if (insideOpaque) {
        stack.push({ kind: "opaque", start: index });
      } else if (prelude.length > 0) {
        rules.push({ selector: prelude, layer: layerPath(), line: lineNumberAt(masked, start), bodyStart: index });
        stack.push({ kind: "rule", start: index });
      } else {
        stack.push({ kind: "rule", start: index, selector: "" });
      }

      buffer = "";
      bufferStart = -1;
      index += 1;
      continue;
    }

    if (character === "}") {
      const entry = stack.pop();
      if (entry && entry.kind === "at" && entry.reducedMotion) {
        reducedMotionSpans.push({ start: entry.start, end: index });
      }
      buffer = "";
      bufferStart = -1;
      index += 1;
      continue;
    }

    if (character === ";" && stack.length === 0) {
      const statement = buffer.trim();
      if (/^@layer\b/.test(statement)) {
        const names = statement
          .replace(/^@layer\b/, "")
          .split(",")
          .map((name) => name.trim())
          .filter(Boolean);
        if (names.length > 0) {
          activeReport.layerOrderDeclarations.push(names);
        }
      }
      buffer = "";
      bufferStart = -1;
      index += 1;
      continue;
    }

    if (bufferStart < 0 && !/\s/.test(character)) {
      bufferStart = index;
    }
    buffer += character;
    index += 1;
  }

  return { rules, reducedMotionSpans, masked };
}

// A selector part is "element styled" when its subject compound (the last one,
// which is the element actually matched) begins with a bare element name.
function subjectElementOf(selectorPart) {
  const compounds = selectorPart
    .trim()
    .split(/[\s>+~]+/)
    .filter(Boolean);
  const subject = compounds[compounds.length - 1];
  if (!subject) {
    return null;
  }
  const match = /^([a-zA-Z][a-zA-Z0-9-]*)/.exec(subject);
  if (!match) {
    return null;
  }
  return match[1].toLowerCase();
}

function checkCssFile(filePath, source) {
  const { rules, reducedMotionSpans, masked } = scanCss(source);
  const relative = relativePath(filePath);
  let unlayered = 0;

  for (const rule of rules) {
    if (rule.layer === null) {
      unlayered += 1;
    } else if (rule.layer === "components" && rule.selector.length > 0) {
      for (const part of rule.selector.split(",")) {
        const element = subjectElementOf(part);
        if (element && SLOT_BACKED_ELEMENTS.has(element)) {
          addViolation(
            filePath,
            rule.line,
            "css-components-bare-element-selector",
            `@layer components styles <${element}> by element type; target [data-slot="..."] instead: ${part.trim()}`,
          );
        }
      }
    }

    if (/\.btn-[a-z0-9-]+/.test(rule.selector)) {
      for (const className of rule.selector.match(/\.btn-[a-z0-9-]+/g) ?? []) {
        addViolation(
          filePath,
          rule.line,
          "css-legacy-button-variant",
          `Legacy button variant class ${className} must be replaced by a size/variant prop.`,
        );
      }
    }
  }

  activeReport.unlayeredRuleCounts.set(relative, unlayered);

  for (const match of masked.matchAll(/!\s*important/gi)) {
    // `@media (prefers-reduced-motion: reduce)` is the one place where !important
    // is the standard way to win against animation declared elsewhere.
    if (reducedMotionSpans.some((span) => match.index > span.start && match.index < span.end)) {
      continue;
    }
    addViolation(
      filePath,
      lineNumberAt(masked, match.index),
      "css-important",
      "!important is not allowed in project CSS; fix the cascade layer instead.",
    );
  }
}

// ---------------------------------------------------------------------------
// TypeScript / TSX scanning
// ---------------------------------------------------------------------------

function collectClassTexts(node, sourceFile, out) {
  if (!node) {
    return;
  }

  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
    out.push({ text: node.text, pos: node.getStart(sourceFile) });
    return;
  }

  if (ts.isJsxExpression(node)) {
    collectClassTexts(node.expression, sourceFile, out);
    return;
  }

  if (ts.isTemplateExpression(node)) {
    out.push({ text: node.head.text, pos: node.head.getStart(sourceFile) });
    for (const span of node.templateSpans) {
      collectClassTexts(span.literal, sourceFile, out);
    }
    return;
  }

  if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.PlusToken) {
    collectClassTexts(node.left, sourceFile, out);
    collectClassTexts(node.right, sourceFile, out);
    return;
  }

  if (ts.isConditionalExpression(node)) {
    collectClassTexts(node.whenTrue, sourceFile, out);
    collectClassTexts(node.whenFalse, sourceFile, out);
    return;
  }

  if (ts.isParenthesizedExpression(node)) {
    collectClassTexts(node.expression, sourceFile, out);
    return;
  }

  if (ts.isArrayLiteralExpression(node)) {
    for (const element of node.elements) {
      collectClassTexts(element, sourceFile, out);
    }
    return;
  }

  if (ts.isCallExpression(node)) {
    // Covers cn(...), clsx(...), classNames(...) and template tag helpers.
    for (const argument of node.arguments) {
      collectClassTexts(argument, sourceFile, out);
    }
  }
}

// Strips Tailwind variant prefixes so `dark:bg-input/30` is judged as `bg-input/30`
// and `data-[size=default]:h-9` is judged as `h-9` (structural, allowed).
function stripVariants(token) {
  let remaining = token;
  for (let guard = 0; guard < 8; guard += 1) {
    const match = /^[^:\s]+:/.exec(remaining);
    if (!match) {
      return remaining;
    }
    remaining = remaining.slice(match[0].length);
  }
  return remaining;
}

function isAppearanceUtility(token) {
  const base = stripVariants(token);
  if (base.length === 0) {
    return false;
  }
  return APPEARANCE_UTILITY_PATTERNS.some((pattern) => pattern.test(base));
}

function checkSourceFile(filePath, source) {
  const sourceFile = ts.createSourceFile(
    filePath,
    source,
    ts.ScriptTarget.Latest,
    true,
    filePath.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );

  const classTexts = [];

  function visit(node) {
    if (
      ts.isJsxAttribute(node) &&
      node.name.getText(sourceFile) === "className" &&
      node.initializer
    ) {
      collectClassTexts(node.initializer, sourceFile, classTexts);
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);

  for (const entry of classTexts) {
    const line = lineNumberAt(source, entry.pos);
    for (const token of entry.text.split(/\s+/)) {
      if (token.length === 0) {
        continue;
      }
      if (isAppearanceUtility(token)) {
        addViolation(
          filePath,
          line,
          "tsx-appearance-utility-class",
          `Appearance utility class "${token}" belongs in a theme component layer, not in component markup.`,
        );
      }
      if (LEGACY_BUTTON_VARIANT_RE.test(stripVariants(token))) {
        addViolation(
          filePath,
          line,
          "tsx-legacy-button-variant",
          `Legacy button variant class "${token}" must be replaced by a size/variant prop.`,
        );
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Scan entry point
// ---------------------------------------------------------------------------

const CSS_EXTENSIONS = new Set([".css"]);
const SOURCE_EXTENSIONS = new Set([".ts", ".tsx"]);

/**
 * Scans every stylesheet and TypeScript/TSX module under `<rootDir>/ui` and
 * returns the findings for one repository state. Safe to call repeatedly.
 */
export function collectFindings(rootDir) {
  const previousReport = activeReport;
  activeReport = {
    rootDir,
    violations: [],
    violationKeys: new Set(),
    unlayeredRuleCounts: new Map(),
    layerOrderDeclarations: [],
    definedTokens: new Set(),
    cssFileCount: 0,
    sourceFileCount: 0,
  };

  try {
    const report = activeReport;
    const uiDir = path.join(rootDir, "ui");
    const cssFiles = walk(uiDir, CSS_EXTENSIONS);
    const sourceFiles = walk(uiDir, SOURCE_EXTENSIONS);

    // A definition anywhere in the frontend counts, whichever file shape holds it.
    for (const filePath of [...cssFiles, ...sourceFiles]) {
      const text = fs.readFileSync(filePath, "utf8");
      for (const match of text.matchAll(/(--ln-[a-z0-9-]+)\s*:/g)) {
        report.definedTokens.add(match[1]);
      }
    }

    for (const filePath of cssFiles) {
      report.cssFileCount += 1;
      const source = fs.readFileSync(filePath, "utf8");
      checkCssFile(filePath, source);

      for (const match of source.matchAll(/var\(\s*(--ln-[a-z0-9-]+)/g)) {
        if (!report.definedTokens.has(match[1])) {
          addViolation(
            filePath,
            lineNumberAt(source, match.index),
            "token-undefined-reference",
            `${match[1]} is referenced but never defined; it silently falls back.`,
          );
        }
      }
    }

    for (const filePath of sourceFiles) {
      report.sourceFileCount += 1;
      checkSourceFile(filePath, fs.readFileSync(filePath, "utf8"));
    }

    report.violations.sort(compareViolations);
    return report;
  } finally {
    activeReport = previousReport;
  }
}

/** Collapses a report into (rule, file) counts, the baseline's unit of comparison. */
export function buildCounts(report) {
  const counts = new Map();
  const bump = (rule, file, amount) => {
    const key = JSON.stringify([rule, file]);
    counts.set(key, (counts.get(key) ?? 0) + amount);
  };

  for (const violation of report.violations) {
    bump(violation.rule, violation.file, 1);
  }
  for (const [file, count] of report.unlayeredRuleCounts) {
    if (count > 0) {
      bump(UNLAYERED_RULE, file, count);
    }
  }
  return counts;
}

/** Renders counts as the ordered, serializable baseline document body. */
export function countsToJson(counts) {
  const byRule = {};
  for (const [key, count] of [...counts.entries()].sort(([left], [right]) => left.localeCompare(right))) {
    const [rule, file] = JSON.parse(key);
    byRule[rule] ??= {};
    byRule[rule][file] = count;
  }
  const ordered = {};
  for (const rule of Object.keys(byRule).sort()) {
    ordered[rule] = Object.fromEntries(
      Object.entries(byRule[rule]).sort(([left], [right]) => left.localeCompare(right)),
    );
  }
  return ordered;
}

export function baselineDocument(counts) {
  return { schema: BASELINE_SCHEMA, counts: countsToJson(counts) };
}

/** Validates a parsed baseline document and returns its counts keyed like buildCounts. */
export function baselineCountsFromDocument(parsed) {
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("UI style baseline must be a JSON object.");
  }
  if (parsed.schema !== BASELINE_SCHEMA) {
    throw new Error(`UI style baseline must use schema ${BASELINE_SCHEMA}.`);
  }
  if (!parsed.counts || typeof parsed.counts !== "object" || Array.isArray(parsed.counts)) {
    throw new Error("UI style baseline must contain a counts object.");
  }

  const counts = new Map();
  for (const [rule, files] of Object.entries(parsed.counts)) {
    if (typeof rule !== "string" || rule.length === 0 || /[*?[\]{}!]/.test(rule)) {
      throw new Error(`UI style baseline has a non-exact rule: ${JSON.stringify(rule)}`);
    }
    if (!files || typeof files !== "object" || Array.isArray(files)) {
      throw new Error(`UI style baseline rule ${rule} must map files to counts.`);
    }
    for (const [file, count] of Object.entries(files)) {
      if (!isExactRelativePath(file)) {
        throw new Error(`UI style baseline has a non-exact path under ${rule}: ${JSON.stringify(file)}`);
      }
      if (!Number.isSafeInteger(count) || count < 0) {
        throw new Error(`UI style baseline has an invalid count for ${rule} / ${file}.`);
      }
      counts.set(JSON.stringify([rule, file]), count);
    }
  }
  return counts;
}

// ---------------------------------------------------------------------------
// Baseline handling
// ---------------------------------------------------------------------------

function isExactRelativePath(value) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    !/[*?[\]{}!]/.test(value) &&
    !value.includes("\\") &&
    !path.isAbsolute(value) &&
    !value.split("/").includes("..")
  );
}

function readBaseline(filePath) {
  return baselineCountsFromDocument(JSON.parse(fs.readFileSync(filePath, "utf8")));
}

function writeBaseline(filePath, counts) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, `${JSON.stringify(baselineDocument(counts), null, 2)}\n`, "utf8");
}

function printViolations(title, entries) {
  console.error(title);
  for (const violation of entries) {
    console.error(`${violation.file}:${violation.line} [${violation.rule}] ${violation.message}`);
  }
}

function printReport(report) {
  const counts = buildCounts(report);
  const byRule = new Map();
  for (const key of counts.keys()) {
    const [rule] = JSON.parse(key);
    byRule.set(rule, (byRule.get(rule) ?? 0) + counts.get(key));
  }

  const unlayeredFiles = [...report.unlayeredRuleCounts.entries()]
    .filter(([, count]) => count > 0)
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]));
  const totalUnlayered = unlayeredFiles.reduce((sum, [, count]) => sum + count, 0);

  console.log("UI style report");
  console.log(`  scanned: ${report.cssFileCount} CSS file(s), ${report.sourceFileCount} TypeScript file(s)`);
  console.log(`  defined --ln-* tokens: ${report.definedTokens.size}`);
  console.log(`  unlayered CSS rules: ${totalUnlayered} across ${unlayeredFiles.length} file(s)`);
  for (const [file, count] of unlayeredFiles) {
    console.log(`    ${String(count).padStart(5)}  ${file}`);
  }

  const declaredOrder = report.layerOrderDeclarations.find(
    (names) => JSON.stringify(names) === JSON.stringify(LAYER_ORDER),
  );
  console.log(
    declaredOrder
      ? `  cascade layer order: declared as ${LAYER_ORDER.join(", ")}`
      : `  cascade layer order: NOT declared; expected @layer ${LAYER_ORDER.join(", ")}; (Phase 1)`,
  );

  console.log(`  findings: ${report.violations.length}`);
  for (const [rule, count] of [...byRule.entries()].sort(([left], [right]) => left.localeCompare(right))) {
    console.log(`    ${String(count).padStart(5)}  ${rule}`);
  }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/**
 * Runs one mode against a repository state and returns a structured outcome.
 * `main` only formats this, so tests can assert on results without a subprocess.
 */
export function evaluate(options) {
  const report = collectFindings(options.rootDir);
  const currentCounts = buildCounts(report);
  const unlayeredTotal = [...report.unlayeredRuleCounts.values()].reduce((sum, count) => sum + count, 0);
  const outcome = { report, currentCounts, unlayeredTotal };

  if (options.mode === "strict") {
    return { ...outcome, mode: "strict", ok: report.violations.length === 0 && unlayeredTotal === 0 };
  }

  let baselineCounts;
  try {
    baselineCounts = readBaseline(options.baselinePath);
  } catch (error) {
    return { ...outcome, mode: "baseline", ok: false, baselineError: error.message };
  }

  const regressions = [];
  let resolvedCount = 0;
  for (const [key, count] of currentCounts) {
    const allowed = baselineCounts.get(key) ?? 0;
    if (count > allowed) {
      const [rule, file] = JSON.parse(key);
      regressions.push({ rule, file, count, allowed });
    }
  }
  for (const [key, allowed] of baselineCounts) {
    const count = currentCounts.get(key) ?? 0;
    if (count < allowed) {
      resolvedCount += allowed - count;
    }
  }
  regressions.sort(
    (left, right) => left.rule.localeCompare(right.rule) || left.file.localeCompare(right.file),
  );

  return { ...outcome, mode: "baseline", ok: regressions.length === 0, baselineCounts, regressions, resolvedCount };
}

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

function main(argv) {
  const options = parseArguments(argv);

  if (options.printBaseline || options.writeBaseline || options.report) {
    const report = collectFindings(options.rootDir);
    const currentCounts = buildCounts(report);

    if (options.printBaseline) {
      process.stdout.write(`${JSON.stringify(baselineDocument(currentCounts), null, 2)}\n`);
      return 0;
    }
    if (options.writeBaseline) {
      writeBaseline(options.baselinePath, currentCounts);
      console.log(`Wrote ${currentCounts.size} (rule, file) count(s) to ${options.baselinePath}.`);
    }
    printReport(report);
    return 0;
  }

  const outcome = evaluate(options);

  if (outcome.mode === "strict") {
    if (outcome.ok) {
      console.log("UI style strict check passed with zero findings.");
      return 0;
    }
    if (outcome.report.violations.length > 0) {
      printViolations(
        `UI style strict check failed with ${outcome.report.violations.length} violation(s):`,
        outcome.report.violations,
      );
    }
    if (outcome.unlayeredTotal > 0) {
      const fileCount = [...outcome.report.unlayeredRuleCounts.values()].filter((count) => count > 0).length;
      console.error(`Unlayered CSS rules remain: ${outcome.unlayeredTotal} across ${fileCount} file(s).`);
    }
    return 1;
  }

  if (outcome.baselineError) {
    console.error(`UI style baseline check failed: ${outcome.baselineError}`);
    return 1;
  }

  if (!outcome.ok) {
    console.error(`UI style baseline check failed with ${outcome.regressions.length} regression(s):`);
    for (const regression of outcome.regressions) {
      console.error(
        `  [${regression.rule}] ${regression.file}: ${regression.count}, baseline allows ${regression.allowed}`,
      );
    }
    console.error('Run "npm run check:ui-styles:report" for the current totals.');
    return 1;
  }

  const currentTotal = [...outcome.currentCounts.values()].reduce((sum, count) => sum + count, 0);
  const baselineTotal = [...outcome.baselineCounts.values()].reduce((sum, count) => sum + count, 0);
  console.log(
    `UI style baseline check passed: ${currentTotal} known finding(s) (baseline ${baselineTotal}), 0 regressions, ${outcome.resolvedCount} resolved.`,
  );
  return 0;
}

const isMainModule =
  typeof process.argv[1] === "string" && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isMainModule) {
  try {
    process.exit(main(process.argv.slice(2)));
  } catch (error) {
    console.error(`UI style check failed: ${error.message}`);
    process.exit(1);
  }
}
