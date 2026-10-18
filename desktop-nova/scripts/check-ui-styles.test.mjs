import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  baselineCountsFromDocument,
  baselineDocument,
  collectFindings,
  evaluate,
} from "./check-ui-styles.mjs";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.resolve(scriptDir, "..");
const repositoryBaselinePath = path.join(scriptDir, "ui-styles-baseline.json");

const fixtureRoots = [];

after(() => {
  for (const root of fixtureRoots) {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

// The checker scans <root-dir>/ui, so every fixture is a throwaway root with a ui/ tree.
function makeFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lba-ui-style-check-"));
  fixtureRoots.push(root);
  const uiDir = path.join(root, "ui");
  fs.mkdirSync(uiDir, { recursive: true });

  const fixture = {
    root,
    uiDir,
    baselinePath: path.join(root, "ui-styles-baseline.json"),
    write(relativePath, contents) {
      const target = path.join(uiDir, relativePath);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, contents, "utf8");
      return target;
    },
    writeBaselineFromCurrentState() {
      const counts = new Map();
      const report = collectFindings(root);
      for (const violation of report.violations) {
        const key = JSON.stringify([violation.rule, violation.file]);
        counts.set(key, (counts.get(key) ?? 0) + 1);
      }
      for (const [file, count] of report.unlayeredRuleCounts) {
        if (count > 0) {
          const key = JSON.stringify(["css-unlayered-rule", file]);
          counts.set(key, (counts.get(key) ?? 0) + count);
        }
      }
      fs.writeFileSync(fixture.baselinePath, `${JSON.stringify(baselineDocument(counts), null, 2)}\n`, "utf8");
      return counts;
    },
    strict() {
      return evaluate({ mode: "strict", rootDir: root, baselinePath: fixture.baselinePath });
    },
    baseline() {
      return evaluate({ mode: "baseline", rootDir: root, baselinePath: fixture.baselinePath });
    },
  };
  return fixture;
}

function messagesFor(outcome, rule) {
  return outcome.report.violations.filter((violation) => violation.rule === rule);
}

test("baseline mode allows only the recorded counts", () => {
  const fixture = makeFixture();
  fixture.write("styles/fixture.css", ".alpha {\n    color: red;\n}\n\n.beta {\n    color: blue;\n}\n");

  fixture.writeBaselineFromCurrentState();

  const passing = fixture.baseline();
  assert.equal(passing.ok, true, JSON.stringify(passing.regressions));
  assert.equal(passing.resolvedCount, 0);
  assert.equal(passing.unlayeredTotal, 2);

  const strict = fixture.strict();
  assert.equal(strict.ok, false);
  assert.equal(strict.unlayeredTotal, 2);

  fixture.write(
    "styles/fixture.css",
    ".alpha {\n    color: red;\n}\n\n.beta {\n    color: blue;\n}\n\n.gamma {\n    color: green;\n}\n",
  );
  const regression = fixture.baseline();
  assert.equal(regression.ok, false);
  assert.deepEqual(regression.regressions, [
    { rule: "css-unlayered-rule", file: "ui/styles/fixture.css", count: 3, allowed: 2 },
  ]);

  fixture.write("styles/fixture.css", ".alpha {\n    color: red;\n}\n");
  const resolved = fixture.baseline();
  assert.equal(resolved.ok, true);
  assert.equal(resolved.resolvedCount, 1);
});

test("a finding of a rule absent from the baseline is a regression", () => {
  const fixture = makeFixture();
  fixture.write("styles/fixture.css", ".alpha {\n    color: red;\n}\n");
  fixture.writeBaselineFromCurrentState();

  // The unlayered count is unchanged, but a new rule now fires in the same file.
  fixture.write("styles/fixture.css", ".alpha {\n    color: red;\n    background: blue !important;\n}\n");
  const outcome = fixture.baseline();
  assert.equal(outcome.ok, false);
  assert.deepEqual(outcome.regressions, [
    { rule: "css-important", file: "ui/styles/fixture.css", count: 1, allowed: 0 },
  ]);
});

test("each detector reports its own rule", () => {
  const fixture = makeFixture();
  fixture.write(
    "styles/controls.css",
    [
      ".loose {",
      "    color: red;",
      "}",
      "",
      "@layer components {",
      "    button {",
      "        padding: 1px;",
      "    }",
      '    [data-slot="button"] {',
      "        padding: 2px;",
      "    }",
      "}",
      "",
      ".forcing {",
      "    color: red !important;",
      "}",
      "",
      ".token {",
      "    border: 1px solid var(--ln-does-not-exist);",
      "}",
      "",
      ".btn-xs {",
      "    font-size: 11px;",
      "}",
      "",
    ].join("\n"),
  );
  fixture.write("styles/widget.tsx", 'export const W = () => <div className="bg-primary text-sm" />;\n');

  const outcome = fixture.strict();
  assert.equal(outcome.ok, false);

  const bareElements = messagesFor(outcome, "css-components-bare-element-selector");
  assert.equal(bareElements.length, 1, JSON.stringify(bareElements));
  assert.equal(bareElements[0].file, "ui/styles/controls.css");
  assert.match(bareElements[0].message, /styles <button> by element type/);

  assert.equal(messagesFor(outcome, "css-important").length, 1);
  assert.equal(messagesFor(outcome, "token-undefined-reference").length, 1);
  assert.match(messagesFor(outcome, "token-undefined-reference")[0].message, /--ln-does-not-exist/);
  assert.equal(messagesFor(outcome, "css-legacy-button-variant").length, 1);
  assert.equal(messagesFor(outcome, "tsx-appearance-utility-class").length, 2);
  // `.loose`, `.forcing`, `.token` and `.btn-xs`; the @layer components body is layered.
  assert.equal(outcome.unlayeredTotal, 4);
});

test("layout, accessibility resets, and reduced-motion escape hatches are allowed", () => {
  const fixture = makeFixture();
  fixture.write(
    "styles/allowed.css",
    [
      "@media (prefers-reduced-motion: reduce) {",
      "    .motion {",
      "        transition: none !important;",
      "    }",
      "}",
      "",
      "@layer components {",
      '    [data-slot="button"][data-variant="ghost"] .glyph {',
      "        opacity: 0.5;",
      "    }",
      "}",
      "",
    ].join("\n"),
  );
  // `text-left`, `border-collapse`, `outline-none` and `table` are structural or
  // accessibility resets, so the only unlayered style rule is `.motion`; the
  // @media block itself is an at-rule and the @layer components body is layered.
  fixture.write(
    "styles/allowed.tsx",
    'export const A = () => <div className="flex items-center gap-2 text-left border-collapse outline-none table" />;\n',
  );

  const outcome = fixture.strict();
  assert.equal(messagesFor(outcome, "css-important").length, 0);
  assert.equal(messagesFor(outcome, "tsx-appearance-utility-class").length, 0);
  assert.equal(messagesFor(outcome, "css-components-bare-element-selector").length, 0);
  assert.equal(outcome.unlayeredTotal, 1);
});

test("appearance detection strips variants instead of matching raw text", () => {
  const fixture = makeFixture();
  fixture.write(
    "styles/variants.tsx",
    [
      "export const V = () => (",
      '  <div className={"dark:bg-input/30 hover:text-accent-foreground data-[size=default]:h-9 md:text-sm"} />',
      ");",
      "",
    ].join("\n"),
  );

  const outcome = fixture.strict();
  const reported = messagesFor(outcome, "tsx-appearance-utility-class").map((violation) => violation.message);
  assert.equal(reported.length, 3, JSON.stringify(reported));
  assert.ok(reported.some((message) => message.includes('"dark:bg-input/30"')));
  assert.ok(reported.some((message) => message.includes('"hover:text-accent-foreground"')));
  assert.ok(reported.some((message) => message.includes('"md:text-sm"')));
  // `data-[size=default]:h-9` strips to the structural height utility and stays allowed.
  assert.ok(!reported.some((message) => message.includes("h-9")));
});

test("system colours are rejected except inside forced-colors blocks", () => {
  const fixture = makeFixture();
  fixture.write(
    "styles/colours.css",
    [
      ".themed {",
      "    border: 1px solid var(--border);",
      "}",
      "",
      ".leaky {",
      "    border: 1px solid ThreeDShadow;",
      "    color: GrayText;",
      "}",
      "",
      "@media (forced-colors: active) {",
      "    .forced {",
      "        background: Canvas;",
      "        color: ButtonText;",
      "    }",
      "}",
      "",
    ].join("\n"),
  );

  const outcome = fixture.strict();
  const reported = messagesFor(outcome, "css-system-colour");
  assert.equal(reported.length, 2, JSON.stringify(reported.map((v) => v.message)));
  assert.ok(reported.some((v) => v.message.includes("ThreeDShadow")));
  assert.ok(reported.some((v) => v.message.includes("GrayText")));
  // The forced-colours block exists to use the OS palette.
  assert.ok(!reported.some((v) => v.message.includes("Canvas") || v.message.includes("ButtonText")));
});

test("vendor CSS may not be imported into a layer at or above components", () => {
  const fixture = makeFixture();
  fixture.write("styles/base.css", "@layer theme, vendor, components, utilities;\n\n.x { color: red; }\n");
  fixture.write("themes/modern.css", '@import "tailwindcss/utilities.css" layer(utilities);\n');

  const bad = fixture.strict();
  const reported = messagesFor(bad, "css-vendor-layer-above-components");
  assert.equal(reported.length, 1, JSON.stringify(reported.map((v) => v.message)));
  assert.match(reported[0].message, /ranks at or above "components"/);

  // `vendor` and `theme` both rank below `components`, so they are accepted.
  fixture.write(
    "themes/modern.css",
    '@import "tailwindcss/theme.css" layer(theme);\n@import "tailwindcss/utilities.css" layer(vendor);\n',
  );
  assert.equal(messagesFor(fixture.strict(), "css-vendor-layer-above-components").length, 0);
});

test("runtime-set and Radix-provided properties are exempt from the undefined check", () => {
  const fixture = makeFixture();
  fixture.write(
    "styles/tokens.css",
    [
      ":root {",
      "    --ln-known: 1px;",
      "}",
      "",
      ".runtime {",
      "    top: var(--flake-tooltip-top, 0px);",
      "    left: var(--radix-select-trigger-width);",
      "}",
      "",
      ".dead {",
      "    color: var(--ln-nobody-defines-this);",
      "}",
      "",
    ].join("\n"),
  );
  // A property set from TypeScript is legitimately absent from CSS.
  fixture.write(
    "styles/panel.tsx",
    'export const P = () => <div ref={(e) => e?.style.setProperty("--flake-tooltip-top", "1px")} />;\n',
  );

  const reported = messagesFor(fixture.strict(), "token-undefined-reference");
  assert.equal(reported.length, 1, JSON.stringify(reported.map((v) => v.message)));
  assert.match(reported[0].message, /--ln-nobody-defines-this/);
});

test("hardcoded colours are flagged outside the palette and theme swatches", () => {
  const fixture = makeFixture();
  fixture.write(
    "styles/leaky.css",
    [
      ".error {",
      "    border: 1px solid #ff6666;",
      "}",
      "",
      ".tokenised {",
      "    border: 1px solid var(--destructive);",
      "    background: color-mix(in srgb, var(--destructive) 10%, transparent);",
      "}",
      "",
      ":root {",
      "    --palette: #ff6666;",
      "}",
      "",
    ].join("\n"),
  );
  // The theme files *are* the palette, so a literal there is the source of truth.
  fixture.write("themes/demo/theme.css", ".a {\n    color: #ff6666;\n}\n");
  // The settings swatches preview other themes and must stay literal.
  fixture.write("windows/main/pages/settings/settings.css", ".swatch {\n    background: #e4e4e7;\n}\n");

  const reported = messagesFor(fixture.strict(), "css-hardcoded-colour");
  assert.equal(reported.length, 1, JSON.stringify(reported.map((v) => v.message)));
  assert.match(reported[0].file, /styles\/leaky\.css/);
  assert.match(reported[0].message, /#ff6666/);
});

test("raw native form controls are rejected in favour of the shared components", () => {
  const fixture = makeFixture();
  fixture.write(
    "windows/page.tsx",
    [
      "export const P = () => (",
      "    <div>",
      '        <select value="a" onChange={() => {}}><option>a</option></select>',
      '        <input type="checkbox" checked />',
      '        <input type={"range"} />',
      '        <input type="file" />',
      '        <button onClick={() => {}}>go</button>',
      '        <Button onClick={() => {}}>go</Button>',
      "    </div>",
      ");",
      "",
    ].join("\n"),
  );

  const controls = messagesFor(fixture.strict(), "tsx-native-control");
  // select + checkbox; the range is behind an expression and the file input is legitimate.
  assert.equal(controls.length, 2, JSON.stringify(controls.map((v) => v.message)));
  assert.ok(controls.some((v) => v.message.includes("<select>")));
  assert.ok(controls.some((v) => v.message.includes('type="checkbox"')));

  // The lowercase element is flagged; the component is not.
  const raw = messagesFor(fixture.strict(), "tsx-raw-button");
  assert.equal(raw.length, 1, JSON.stringify(raw.map((v) => v.message)));
  assert.match(raw[0].message, /use ui\/button\.tsx/);
});

test("buttons may not set their own font size", () => {
  const fixture = makeFixture();
  fixture.write(
    "styles/buttons.css",
    [
      ".toolbar button { font-size: 11px; }",
      "button.compact, .btn-xs { font-size: 13px; }",
      /* A descendant is not the button: this must not be reported. */
      ".toolbar button span { font-size: 11px; }",
      /* `font: inherit` is the mechanism that keeps one size, so it is allowed. */
      "button { font: inherit; }",
      "",
    ].join("\n"),
  );

  // A selector list is reported per offending selector, so the two bad rules yield three.
  const reported = messagesFor(fixture.strict(), "css-button-font-size");
  assert.equal(reported.length, 3, JSON.stringify(reported.map((v) => v.message)));
  assert.ok(reported.some((v) => v.message.includes(".toolbar button")));
  assert.ok(reported.some((v) => v.message.includes("button.compact")));
  assert.ok(reported.some((v) => v.message.includes(".btn-xs")));
  assert.ok(!reported.some((v) => v.message.includes("span")));
});

test("selectors cannot require a page root while targeting portaled dialog content", () => {
  const fixture = makeFixture();
  fixture.write(
    "styles/portal.css",
    [
      /* Never matches: dialog content is portaled out of the page subtree. */
      ".flake-page.flake-page--edit-mode .flake-page__state-dialog-grid button.key { position: relative; }",
      ".nova-page .dialog-surface button.key { padding: 0; }",
      /* Anchored inside the dialog, so it works. */
      ".flake-page__state-dialog button.key { position: relative; }",
      /* A page rule that does not reach into the dialog is fine. */
      ".flake-page button.toolbar { padding: 0; }",
      "",
    ].join("\n"),
  );

  const reported = messagesFor(fixture.strict(), "css-portal-scope");
  assert.equal(reported.length, 2, JSON.stringify(reported.map((v) => v.message)));
  assert.ok(reported.every((v) => v.message.includes("portaled dialog content")));
});

test("a redundant btn class on <Button> is reported", () => {
  const fixture = makeFixture();
  fixture.write(
    "windows/page.tsx",
    [
      "export const P = () => (",
      "    <div>",
      '        <Button className="btn">ok</Button>',
      '        <Button className="btn panel-button">ok</Button>',
      '        <Button className="panel-button">ok</Button>',
      /* A class that merely contains "btn" is a different class and must survive. */
      '        <Button className="replace-entry-remove-btn">ok</Button>',
      "    </div>",
      ");",
      "",
    ].join("\n"),
  );

  const reported = messagesFor(fixture.strict(), "tsx-redundant-btn-class");
  assert.equal(reported.length, 2, JSON.stringify(reported.map((v) => v.message)));
  assert.ok(reported.every((v) => v.message.includes("already carries `btn`")));
});
test("a var() written inside a comment is documentation, not a reference", () => {
  const fixture = makeFixture();
  fixture.write("styles/tokens.css", ":root { --ln-known: red; }\n");
  fixture.write(
    "styles/comments.css",
    [
      "/* The indirection resolves var(--ln-not-defined-anywhere, <fallback>). */",
      ".a { color: var(--ln-known); }",
      "",
    ].join("\n"),
  );

  // Comments must be masked before scanning for references, or prose about a token
  // gets reported as a missing token.
  assert.equal(messagesFor(fixture.strict(), "token-undefined-reference").length, 0);
});
test("url() references must resolve to real files", () => {
  const fixture = makeFixture();
  fs.mkdirSync(path.join(fixture.root, "ui", "styles"), { recursive: true });
  fs.writeFileSync(path.join(fixture.root, "ui", "styles", "asset.png"), "x");
  fixture.write(
    "styles/assets.css",
    [
      ".ok { background: url(\"./asset.png\"); }",
      ".also-ok { background: url(\"data:image/png;base64,AAAA\"); }",
      ".svg-ref { filter: url(\"#blur\"); }",
      ".broken { background: url(\"./missing.png\"); }",
      "",
    ].join("\n"),
  );

  const reported = messagesFor(fixture.strict(), "css-unresolved-asset");
  assert.equal(reported.length, 1, JSON.stringify(reported.map((v) => v.message)));
  assert.match(reported[0].message, /missing\.png/);
});
test("class tokens fused by a rewrite are reported", () => {
  const fixture = makeFixture();
  // The known-class set comes from project CSS, so both halves must exist there.
  fixture.write("styles/note-keys.css", ".note-key-button { color: red; }\n.is-sharp { opacity: 1; }\n");
  fixture.write("styles/keys.tsx", 'export const K = () => <button className="note-key-buttonis-sharp" />;\n');

  const reported = messagesFor(fixture.strict(), "tsx-glued-class-token");
  assert.equal(reported.length, 1, JSON.stringify(reported.map((v) => v.message)));
  assert.match(reported[0].message, /note-key-buttonis-sharp/);
});
test("baseline documents are validated before use", () => {
  const fixture = makeFixture();
  fixture.write("styles/fixture.css", ".alpha {\n    color: red;\n}\n");

  const invalid = (document, pattern) => {
    fs.writeFileSync(fixture.baselinePath, `${JSON.stringify(document, null, 2)}\n`, "utf8");
    const outcome = fixture.baseline();
    assert.equal(outcome.ok, false);
    assert.match(outcome.baselineError, pattern);
  };

  invalid({ schema: "wrong.schema", counts: {} }, /must use schema lba\.ui-style-violations-baseline\.v1/);
  invalid({ schema: "lba.ui-style-violations-baseline.v1" }, /must contain a counts object/);
  invalid(
    { schema: "lba.ui-style-violations-baseline.v1", counts: { "css-important": { "ui/**/*.css": 1 } } },
    /non-exact path/,
  );
  invalid(
    { schema: "lba.ui-style-violations-baseline.v1", counts: { "css-important": { "ui/a.css": -1 } } },
    /invalid count/,
  );
  invalid(
    { schema: "lba.ui-style-violations-baseline.v1", counts: { "css-*": { "ui/a.css": 1 } } },
    /non-exact rule/,
  );
  assert.throws(() => baselineCountsFromDocument(null), /must be a JSON object/);
});

test("collectFindings is repeatable and does not leak state between calls", () => {
  const first = makeFixture();
  first.write("styles/a.css", ".alpha {\n    color: red;\n}\n");
  const firstReport = collectFindings(first.root);
  assert.equal(firstReport.unlayeredRuleCounts.get("ui/styles/a.css"), 1);
  assert.equal(firstReport.definedTokens.size, 0);

  const second = makeFixture();
  second.write("styles/b.css", ":root {\n    --ln-known: 1px;\n}\n\n.beta {\n    color: var(--ln-known);\n}\n");
  const secondReport = collectFindings(second.root);
  assert.equal(secondReport.unlayeredRuleCounts.get("ui/styles/b.css"), 2);
  assert.equal(secondReport.violations.length, 0);
  // The first fixture's findings must not appear in the second report.
  assert.equal(secondReport.unlayeredRuleCounts.has("ui/styles/a.css"), false);
  assert.equal(collectFindings(first.root).unlayeredRuleCounts.get("ui/styles/a.css"), 1);
});

test("rules must live in the ownership sublayer for their folder", () => {
  const fixture = makeFixture();
  fixture.write(
    "windows/main/pages/home/home.css",
    [
      "@layer theme, vendor, tokens, base, components, utilities;",
      "@layer components.shared, components.page, components.theme;",
      "@layer components.shared {",
      "    .page-layout { display: flex; }",
      "}",
      "",
    ].join("\n"),
  );
  const outcome = fixture.strict();
  const assigned = messagesFor(outcome, "css-sublayer-assignment");
  assert.equal(assigned.length, 1, JSON.stringify(assigned));
  assert.match(assigned[0].message, /components\.page/);
});

test("a file that uses component sublayers must declare their order", () => {
  const fixture = makeFixture();
  fixture.write(
    "themes/probe/theme.css",
    [
      "@layer theme, vendor, tokens, base, components, utilities;",
      "@layer components.theme {",
      '    [data-slot="button"] { border-radius: 0; }',
      "}",
      "",
    ].join("\n"),
  );
  const outcome = fixture.strict();
  const order = messagesFor(outcome, "css-sublayer-order");
  assert.equal(order.length, 1, JSON.stringify(order));
  assert.match(order[0].message, /components\.shared, components\.page, components\.theme/);
});

test("page text sizes must come from the theme type scale", () => {
  const fixture = makeFixture();
  fixture.write(
    "windows/main/pages/home/home.css",
    [
      "@layer theme, vendor, tokens, base, components, utilities;",
      "@layer components.shared, components.page, components.theme;",
      "@layer components.page {",
      "    .caption { font-size: .86rem; }",
      "    .ok { font-size: var(--nova-text-sm); }",
      "    .display { font-size: clamp(1.8rem, 3vw, 2.7rem); }",
      "}",
      "",
    ].join("\n"),
  );
  const outcome = fixture.strict();
  const reported = messagesFor(outcome, "css-page-font-size");
  assert.equal(reported.length, 1, JSON.stringify(reported));
  assert.match(reported[0].message, /\.86rem/);
});

test("theme entries must define the text scale tokens", () => {
  const fixture = makeFixture();
  fixture.write("themes/plain/theme.css", ["@layer tokens {", ":root { --background: #fff; }", "}", ""].join("\n"));
  const outcome = fixture.strict();
  const reported = messagesFor(outcome, "css-theme-type-token");
  assert.equal(reported.length, 4, JSON.stringify(reported));
  assert.deepEqual(reported.map((entry) => entry.message.match(/--nova-text-[a-z]+/)[0]).sort(), ["--nova-text-base", "--nova-text-lg", "--nova-text-sm", "--nova-text-xs"]);
});

test("small text that uses the type scale must also expose the family token", () => {
  const fixture = makeFixture();
  fixture.write(
    "styles/controls.css",
    [
      "@layer theme, vendor, tokens, base, components, utilities;",
      "@layer components.shared, components.page, components.theme;",
      "@layer components.shared {",
      "    .bad { font-size: var(--nova-text-sm); }",
      "    .good { font-size: var(--nova-text-sm); font-family: var(--nova-font-sm, inherit); }",
      "}",
      "",
    ].join("\n"),
  );
  const outcome = fixture.strict();
  const reported = messagesFor(outcome, "css-text-scale-family");
  assert.equal(reported.length, 1, JSON.stringify(reported));
  assert.match(reported[0].message, /--nova-font-sm/);
});

test("the repository baseline is in sync with the working tree", () => {
  assert.ok(fs.existsSync(repositoryBaselinePath), "ui-styles-baseline.json must exist");

  const report = collectFindings(repositoryRoot);
  const counts = new Map();
  for (const violation of report.violations) {
    const key = JSON.stringify([violation.rule, violation.file]);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  for (const [file, count] of report.unlayeredRuleCounts) {
    if (count > 0) {
      const key = JSON.stringify(["css-unlayered-rule", file]);
      counts.set(key, (counts.get(key) ?? 0) + count);
    }
  }

  const onDisk = JSON.parse(fs.readFileSync(repositoryBaselinePath, "utf8"));
  assert.deepEqual(
    onDisk,
    baselineDocument(counts),
    'ui-styles-baseline.json is stale; run "npm run check:ui-styles:update-baseline" after an intended change.',
  );
});
