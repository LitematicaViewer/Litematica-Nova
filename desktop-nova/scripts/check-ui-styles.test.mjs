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
