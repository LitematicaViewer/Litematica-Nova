import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import test from "node:test";
import ts from "typescript";

const repoRoot = new URL("../../", import.meta.url);
const indexPath = "data/generation-templates/index.json";
function service(readWorkspaceFile) {
  const source = readFileSync(new URL("../src/services/generationTemplates.ts", import.meta.url), "utf8");
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS } }).outputText;
  const context = { exports: {}, require(id) {
    if (id === "./backend") return { readWorkspaceFile };
    if (id === "./generateService") return {};
    throw new Error("Unexpected dependency: " + id);
  } };
  vm.runInNewContext(compiled, context);
  return context.exports;
}

test("模板目录：读取实际带 BOM 的索引与全部 11 份模板", async () => {
  const reads = [];
  const loader = service(async (path) => {
    reads.push(path);
    return readFile(new URL(path, repoRoot), "utf8");
  });
  const templates = await loader.loadGenerationTemplates();
  assert.equal(templates.length, 11);
  assert.equal(new Set(templates.map((item) => item.id)).size, 11);
  assert.ok(templates.some((item) => item.id === "stone-brick-platform" && item.name_zh === "石砖平台"));
  assert.ok(templates.every((item) => item.plan.regions.length && Array.isArray(item.tags)));
  assert.equal(reads.length, 12);
  assert.equal(reads[0], indexPath);
});

test("模板解析：索引与模板分别兼容 BOM，正文字符保持不变", async () => {
  const template = { id: "bom", name_zh: "模板\uFEFF正文", tags: [], plan: { regions: [] } };
  for (const indexBom of ["", "\uFEFF"]) {
    for (const templateBom of ["", "\uFEFF"]) {
      const loader = service(async (path) => path === indexPath
        ? indexBom + '["bom.json"]'
        : templateBom + JSON.stringify(template));
      const loaded = await loader.loadGenerationTemplates();
      assert.equal(loaded[0].name_zh, template.name_zh);
    }
  }
});

test("模板读取：缺失或无效 JSON 保持失败，不返回空列表", async () => {
  const missing = service(async () => { throw new Error("模板资源缺失"); });
  await assert.rejects(missing.loadGenerationTemplates(), /模板资源缺失/);
  const malformed = service(async (path) => path === indexPath ? '["bad.json"]' : "{invalid}");
  await assert.rejects(malformed.loadGenerationTemplates(), { name: "SyntaxError" });
});
