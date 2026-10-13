import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

function service(api, dialog = async () => null) {
  const exports = {};
  const source = readFileSync(new URL("../src/services/statsService.ts", import.meta.url), "utf8");
  const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  vm.runInNewContext(code, { exports, performance, console: { info() {} }, require(id) {
    if (id === "../platform/applicationOperations") return api;
    if (id === "../platform/dialogs") return { saveDialog: dialog };
    if (id === "./backend") return {};
    if (id === "./i18n") return { translateBlockId: id => id, translateBuildingType: id => id };
    throw Error(id);
  }});
  return exports;
}
function deferred() { let resolve; let reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }

test("same file and container parameter share an inflight analysis, completed requests revalidate in Host", async () => {
  const gate = deferred(); let calls = 0;
  const s = service({ getProjectStats: () => { calls++; return gate.promise; } });
  const first = s.loadStructureStats("real.litematic", false);
  const second = s.loadStructureStats("real.litematic", false);
  assert.equal(calls, 1); gate.resolve({}); await Promise.all([first, second]);
  await s.loadStructureStats("real.litematic", false); assert.equal(calls, 2);
});
test("failed inflight requests clear their slot; container parameters remain independent", async () => {
  const gate = deferred(); let calls = 0;
  const s = service({ getProjectStats: () => { calls++; return calls <= 2 ? gate.promise : Promise.resolve({}); } });
  const a = s.loadStructureStats("a", false); const b = s.loadStructureStats("a", true);
  assert.equal(calls, 2); gate.reject(Error("parse failed"));
  await assert.rejects(a, /parse failed/); await assert.rejects(b, /parse failed/);
  await s.loadStructureStats("a", false); assert.equal(calls, 3);
});
test("material scope inflight keys preserve file, region/layer and containers", async () => {
  const gate = deferred(); let calls = 0;
  const s = service({ getProjectMaterials: () => { calls++; return gate.promise; } });
  const scope = { kind: "region", value: "main" };
  const a = s.loadMaterialsScope("a", scope, false); const b = s.loadMaterialsScope("a", scope, false);
  const c = s.loadMaterialsScope("a", { kind: "layer", value: "1" }, false);
  assert.equal(calls, 2); gate.resolve({ material_items: [] }); await Promise.all([a, b, c]);
});
test("all export options reach a real product command, cancel writes nothing", async () => {
  const calls = []; let save = null;
  const s = service({ exportMaterialList: async input => { calls.push(input); return { output: input.outputPath }; } }, async () => save);
  const options = { scope: { kind: "layer", value: "3" }, includeContainerItems: true, multiplier: 4, minecraftVersion: "26.3", terminalRawMaterials: true, format: "html" };
  assert.equal(await s.exportMaterialListWithOptions("a.litematic", options), false); assert.equal(calls.length, 0);
  save = "C:\\exports\\materials.html";
  assert.equal(await s.exportMaterialListWithOptions("a.litematic", options), true);
  assert.equal(calls[0].format, "html"); assert.equal(calls[0].multiplier, 4);
  assert.equal(calls[0].scope.kind, "layer"); assert.equal(calls[0].includeContainerItems, true);
  assert.equal(calls[0].terminalRawMaterials, true); assert.equal(calls[0].outputPath, save);
});

test("preparing recipes is explicit and never claims success for a missing cache", async () => {
  let calls = 0; let status = "missing";
  const s = service({ prepareMaterialRecipes: async version => { calls++; assert.equal(version, "26.3"); return { status, has_recipes: status === "available" }; } });
  assert.equal(calls, 0);
  await assert.rejects(s.fetchMaterialRecipeCache(), /尚未就绪/);
  status = "available"; await s.fetchMaterialRecipeCache(); assert.equal(calls, 2);
});
