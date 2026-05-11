#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import ts from "typescript";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(scriptDir, "..");
const srcDir = path.join(rootDir, "src");
const uiDir = path.join(rootDir, "ui");
const legacyUiDir = path.join(srcDir, "ui");

const SOURCE_EXTENSIONS = new Set([".ts", ".tsx", ".js", ".jsx"]);
const TEXT_EXTENSIONS = new Set([...SOURCE_EXTENSIONS, ".css", ".scss", ".json"]);
const PAGE_FILE_RE = /(^|[/\\])pages[/\\].+\.[tj]sx?$|(^|[/\\])routes[/\\].*Page\.[tj]sx?$|Page\.[tj]sx?$/;
const API_KEY_RE = /\b(api[-_\s]*key|apikey|secret|token|ai[_-]?(save|clear|get|test|chat)|openai|gemini|provider)\b/i;

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

if (violations.length > 0) {
  console.error(`Layer boundary check failed with ${violations.length} violation(s):`);
  for (const violation of violations) {
    console.error(`${violation.file}:${violation.line} [${violation.rule}] ${violation.message}`);
  }
  process.exit(1);
}

console.log("Layer boundary check passed.");
