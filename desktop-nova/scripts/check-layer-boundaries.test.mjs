import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const checkerPath = path.join(scriptDir, "check-layer-boundaries.mjs");
const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "lba-layer-check-"));
const pageDir = path.join(fixtureRoot, "ui", "pages");
const pagePath = path.join(pageDir, "FixturePage.tsx");
const baselinePath = path.join(fixtureRoot, "layer-baseline.json");

fs.mkdirSync(pageDir, { recursive: true });

after(() => {
  fs.rmSync(fixtureRoot, { recursive: true, force: true });
});

function runChecker(...arguments_) {
  return spawnSync(
    process.execPath,
    [
      checkerPath,
      `--root-dir=${fixtureRoot}`,
      `--baseline=${baselinePath}`,
      ...arguments_,
    ],
    {
      encoding: "utf8",
    },
  );
}

function outputOf(result) {
  return `${result.stdout}\n${result.stderr}`;
}

test("baseline mode allows only the exact recorded violations", () => {
  fs.writeFileSync(
    pagePath,
    'export const FixturePage = () => <div style={{ color: "red" }}>fixture</div>;\n',
    "utf8",
  );

  const writeResult = runChecker("--write-baseline");
  assert.equal(writeResult.status, 0, outputOf(writeResult));

  const baselineResult = runChecker("--mode=baseline");
  assert.equal(baselineResult.status, 0, outputOf(baselineResult));
  assert.match(baselineResult.stdout, /1 known violation\(s\), 0 new, 0 resolved/);

  const strictResult = runChecker("--mode=strict");
  assert.equal(strictResult.status, 1, outputOf(strictResult));
  assert.match(strictResult.stderr, /strict check failed with 1 violation/);

  fs.writeFileSync(
    pagePath,
    '\nexport const FixturePage = () => <div style={{ color: "red" }}>fixture</div>;\n',
    "utf8",
  );
  const shiftedViolationResult = runChecker("--mode=baseline");
  assert.equal(shiftedViolationResult.status, 1, outputOf(shiftedViolationResult));
  assert.match(shiftedViolationResult.stderr, /failed with 1 new violation/);

  fs.writeFileSync(
    pagePath,
    'export const FixturePage = () => <div style={{ color: "red" }}>fixture</div>;\n',
    "utf8",
  );
  fs.appendFileSync(
    pagePath,
    'export const UnsafeHtml = () => <div dangerouslySetInnerHTML={{ __html: "fixture" }} />;\n',
    "utf8",
  );
  const newViolationResult = runChecker("--mode=baseline");
  assert.equal(newViolationResult.status, 1, outputOf(newViolationResult));
  assert.match(newViolationResult.stderr, /failed with 1 new violation/);

  fs.writeFileSync(pagePath, "export const FixturePage = () => <div>fixture</div>;\n", "utf8");
  const resolvedResult = runChecker("--mode=baseline");
  assert.equal(resolvedResult.status, 0, outputOf(resolvedResult));
  assert.match(resolvedResult.stdout, /0 known violation\(s\), 0 new, 1 resolved/);
});

test("baseline entries reject wildcard paths and rules", () => {
  fs.writeFileSync(
    baselinePath,
    `${JSON.stringify(
      {
        schema: "lba.layer-violations-baseline.v1",
        violations: [
          {
            file: "ui/**/*.tsx",
            line: 1,
            rule: "ui-*",
            message: "wildcards must not be accepted",
          },
        ],
      },
      null,
      2,
    )}\n`,
    "utf8",
  );

  const result = runChecker("--mode=baseline");
  assert.equal(result.status, 1, outputOf(result));
  assert.match(outputOf(result), /non-exact file path/);

  fs.writeFileSync(
    baselinePath,
    `${JSON.stringify(
      {
        schema: "lba.layer-violations-baseline.v1",
        violations: [
          {
            file: "ui/pages/FixturePage.tsx",
            line: 1,
            rule: "ui-*",
            message: "wildcard rules must not be accepted",
          },
        ],
      },
      null,
      2,
    )}\n`,
    "utf8",
  );
  const wildcardRuleResult = runChecker("--mode=baseline");
  assert.equal(wildcardRuleResult.status, 1, outputOf(wildcardRuleResult));
  assert.match(outputOf(wildcardRuleResult), /non-exact rule/);
});
