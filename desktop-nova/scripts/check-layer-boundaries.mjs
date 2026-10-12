#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import ts from "typescript";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const defaultRootDir = path.resolve(scriptDir, "..");
const BASELINE_SCHEMA = "lba.layer-violations-baseline.v1";

function parseArguments(argv) {
  const options = {
    mode: "strict",
    rootDir: defaultRootDir,
    baselinePath: path.join(scriptDir, "layer-violations-baseline.json"),
    writeBaseline: false,
  };

  for (const argument of argv) {
    if (argument === "--write-baseline") {
      options.writeBaseline = true;
    } else if (argument.startsWith("--mode=")) {
      options.mode = argument.slice("--mode=".length);
    } else if (argument.startsWith("--root-dir=")) {
      options.rootDir = path.resolve(argument.slice("--root-dir=".length));
    } else if (argument.startsWith("--baseline=")) {
      options.baselinePath = path.resolve(argument.slice("--baseline=".length));
    } else {
      throw new Error(`Unknown layer check argument: ${argument}`);
    }
  }

  if (options.mode !== "strict" && options.mode !== "baseline") {
    throw new Error(`Invalid layer check mode ${JSON.stringify(options.mode)}; expected strict or baseline.`);
  }
  return options;
}

const options = parseArguments(process.argv.slice(2));
const rootDir = options.rootDir;
const srcDir = path.join(rootDir, "src");
const uiDir = path.join(rootDir, "ui");
const legacyUiDir = path.join(srcDir, "ui");

const SOURCE_EXTENSIONS = new Set([".ts", ".tsx", ".js", ".jsx"]);
const TEXT_EXTENSIONS = new Set([...SOURCE_EXTENSIONS, ".css", ".scss", ".json"]);
const PAGE_FILE_RE = /(^|[/\\])pages[/\\].+\.[tj]sx?$|(^|[/\\])routes[/\\].*Page\.[tj]sx?$|Page\.[tj]sx?$/;
const API_KEY_RE = /\b(api[-_\s]*key|apikey|secret|token|ai[_-]?(save|clear|get|test|chat)|openai|gemini|provider)\b/i;
const ALLOWED_INLINE_STYLE_RE = /style=\{\{[^}]*\b(cursor|left|top|width)\s*:/;

const violations = [];

function toPosix(filePath) {
  return filePath.split(path.sep).join("/");
}

function relativePath(filePath) {
  return toPosix(path.relative(rootDir, filePath));
}

function addViolation(filePath, line, rule, message) {
  violations.push({
    file: relativePath(filePath),
    line,
    rule,
    message
  });
}

function compareViolations(left, right) {
  return (
    left.file.localeCompare(right.file) ||
    left.line - right.line ||
    left.rule.localeCompare(right.rule) ||
    left.message.localeCompare(right.message)
  );
}

function violationKey(violation) {
  return JSON.stringify([violation.file, violation.line, violation.rule, violation.message]);
}

function validateBaselineViolation(value, index) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`Layer baseline violation ${index} must be an object.`);
  }
  const exactKeys = ["file", "line", "rule", "message"];
  const keys = Object.keys(value).sort();
  if (JSON.stringify(keys) !== JSON.stringify([...exactKeys].sort())) {
    throw new Error(`Layer baseline violation ${index} must contain exactly file, line, rule, and message.`);
  }
  if (
    typeof value.file !== "string" ||
    value.file.length === 0 ||
    /[*?[\]{}!]/.test(value.file) ||
    value.file.includes("\\") ||
    path.isAbsolute(value.file) ||
    value.file.split("/").includes("..")
  ) {
    throw new Error(`Layer baseline violation ${index} has a non-exact file path.`);
  }
  if (!Number.isSafeInteger(value.line) || value.line < 1) {
    throw new Error(`Layer baseline violation ${index} has an invalid line.`);
  }
  if (typeof value.rule !== "string" || value.rule.length === 0 || /[*?[\]{}!]/.test(value.rule)) {
    throw new Error(`Layer baseline violation ${index} has a non-exact rule.`);
  }
  if (typeof value.message !== "string" || value.message.length === 0) {
    throw new Error(`Layer baseline violation ${index} has an invalid message.`);
  }
  return value;
}

function readBaseline(filePath) {
  const parsed = JSON.parse(fs.readFileSync(filePath, "utf8"));
  if (
    !parsed ||
    typeof parsed !== "object" ||
    parsed.schema !== BASELINE_SCHEMA ||
    !Array.isArray(parsed.violations)
  ) {
    throw new Error(`Layer baseline must use schema ${BASELINE_SCHEMA}.`);
  }
  const entries = parsed.violations.map(validateBaselineViolation).sort(compareViolations);
  const keys = entries.map(violationKey);
  if (new Set(keys).size !== keys.length) {
    throw new Error("Layer baseline contains duplicate exact violations.");
  }
  return entries;
}

function writeBaseline(filePath, entries) {
  const parent = path.dirname(filePath);
  fs.mkdirSync(parent, { recursive: true });
  fs.writeFileSync(
    filePath,
    `${JSON.stringify({ schema: BASELINE_SCHEMA, violations: entries }, null, 2)}\n`,
    "utf8",
  );
}

function printViolations(title, entries) {
  console.error(title);
  for (const violation of entries) {
    console.error(`${violation.file}:${violation.line} [${violation.rule}] ${violation.message}`);
  }
}

function walk(dir) {
  if (!fs.existsSync(dir)) {
    return [];
  }

  const files = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === "dist") {
      continue;
    }

    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...walk(fullPath));
    } else if (TEXT_EXTENSIONS.has(path.extname(entry.name))) {
      files.push(fullPath);
    }
  }
  return files;
}

function lineNumberAt(text, index) {
  let line = 1;
  for (let i = 0; i < index; i += 1) {
    if (text.charCodeAt(i) === 10) {
      line += 1;
    }
  }
  return line;
}

function isUnder(filePath, dirPath) {
  const relative = path.relative(dirPath, filePath);
  return relative !== "" && !relative.startsWith("..") && !path.isAbsolute(relative);
}

function resolveImport(fromFile, specifier) {
  if (specifier.startsWith(".")) {
    return path.resolve(path.dirname(fromFile), specifier);
  }

  if (specifier.startsWith("@/")) {
    return path.resolve(srcDir, specifier.slice(2));
  }

  if (specifier === "src" || specifier.startsWith("src/")) {
    return path.resolve(rootDir, specifier);
  }

  return null;
}

function layerDirs(layerName) {
  if (layerName === "ui") {
    return [uiDir, legacyUiDir];
  }
  return [path.join(srcDir, layerName)];
}

function importTargetsLayer(fromFile, specifier, layerName) {
  const target = resolveImport(fromFile, specifier);
  if (!target) {
    return false;
  }
  return layerDirs(layerName).some((layerDir) => target === layerDir || isUnder(target, layerDir));
}

function collectImportSpecifiers(sourceFile) {
  const imports = [];

  function visit(node) {
    if (ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier)) {
      imports.push({ specifier: node.moduleSpecifier.text, pos: node.moduleSpecifier.getStart(sourceFile) });
    }

    if (
      ts.isCallExpression(node) &&
      node.expression.kind === ts.SyntaxKind.ImportKeyword &&
      node.arguments.length === 1 &&
      ts.isStringLiteral(node.arguments[0])
    ) {
      imports.push({ specifier: node.arguments[0].text, pos: node.arguments[0].getStart(sourceFile) });
    }

    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return imports;
}

function collectLocalStorageUses(sourceFile) {
  const uses = [];

  function visit(node) {
    if (ts.isIdentifier(node) && node.text === "localStorage") {
      uses.push(node.getStart(sourceFile));
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return uses;
}

function checkTextRules(filePath, text) {
  const rel = relativePath(filePath);
  const lines = text.split(/\r?\n/);
  const inUi = isUnder(filePath, uiDir) || isUnder(filePath, legacyUiDir);
  const isPage = PAGE_FILE_RE.test(rel);

  lines.forEach((line, index) => {
    const lineNumber = index + 1;

    if (inUi && line.includes("@tauri-apps/api")) {
      addViolation(filePath, lineNumber, "ui-no-tauri-api", "ui must not import @tauri-apps/api.");
    }

    if (inUi && /\binvoke\s*\(/.test(line)) {
      addViolation(filePath, lineNumber, "ui-no-direct-invoke", "ui must call platform services instead of invoke directly.");
    }

    if (inUi && /litematica_(core|native_viewer)\.exe/.test(line)) {
      addViolation(filePath, lineNumber, "ui-no-backend-binary", "ui must not reference backend executable names directly.");
    }

    if (isPage && /\binvoke\s*\(/.test(line)) {
      addViolation(filePath, lineNumber, "pages-no-direct-invoke", "Page files must not invoke Tauri commands directly.");
    }

    if (inUi && line.includes("style={{") && !ALLOWED_INLINE_STYLE_RE.test(line)) {
      addViolation(filePath, lineNumber, "ui-no-fixed-inline-style", "Fixed UI styles must live in CSS classes; only cursor, tooltip position, and progress width inline styles are allowed.");
    }

    if (inUi && line.includes("dangerouslySetInnerHTML")) {
      addViolation(filePath, lineNumber, "ui-no-dangerous-html", "UI icons/content should be rendered as components instead of dangerouslySetInnerHTML.");
    }
  });
}

function checkSourceRules(filePath, text) {
  const sourceFile = ts.createSourceFile(filePath, text, ts.ScriptTarget.Latest, true, filePath.endsWith(".tsx") || filePath.endsWith(".jsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const inBusiness = isUnder(filePath, path.join(srcDir, "business"));
  const inPlatform = isUnder(filePath, path.join(srcDir, "platform"));

  for (const item of collectImportSpecifiers(sourceFile)) {
    const line = lineNumberAt(text, item.pos);

    if (inBusiness && importTargetsLayer(filePath, item.specifier, "ui")) {
      addViolation(filePath, line, "business-no-ui-import", "src/business must not import ui.");
    }

    if (inPlatform && importTargetsLayer(filePath, item.specifier, "ui")) {
      addViolation(filePath, line, "platform-no-ui-import", "src/platform must not import ui.");
    }

    if (inPlatform && importTargetsLayer(filePath, item.specifier, "business")) {
      addViolation(filePath, line, "platform-no-business-import", "src/platform must not import src/business.");
    }
  }

  if (API_KEY_RE.test(text)) {
    for (const pos of collectLocalStorageUses(sourceFile)) {
      addViolation(filePath, lineNumberAt(text, pos), "api-key-no-local-storage", "API key related code must not use localStorage.");
    }
  }
}

for (const filePath of [...walk(srcDir), ...walk(uiDir)]) {
  const text = fs.readFileSync(filePath, "utf8");
  checkTextRules(filePath, text);

  if (SOURCE_EXTENSIONS.has(path.extname(filePath))) {
    checkSourceRules(filePath, text);
  }
}

violations.sort(compareViolations);

if (options.writeBaseline) {
  writeBaseline(options.baselinePath, violations);
  console.log(`Wrote ${violations.length} exact layer violation(s) to ${options.baselinePath}.`);
  process.exit(0);
}

if (options.mode === "strict") {
  if (violations.length === 0) {
    console.log("Layer boundary strict check passed with zero violations.");
    process.exit(0);
  }
  printViolations(`Layer boundary strict check failed with ${violations.length} violation(s):`, violations);
  process.exit(1);
}

const baseline = readBaseline(options.baselinePath);
const baselineKeys = new Set(baseline.map(violationKey));
const currentKeys = new Set(violations.map(violationKey));
const newViolations = violations.filter((violation) => !baselineKeys.has(violationKey(violation)));
const resolvedCount = baseline.filter((violation) => !currentKeys.has(violationKey(violation))).length;

if (newViolations.length > 0) {
  printViolations(
    `Layer boundary baseline check failed with ${newViolations.length} new violation(s); ${violations.length - newViolations.length} exact baseline violation(s) remain:`,
    newViolations,
  );
  process.exit(1);
}

console.log(
  `Layer boundary baseline check passed: ${violations.length} known violation(s), 0 new, ${resolvedCount} resolved.`,
);
