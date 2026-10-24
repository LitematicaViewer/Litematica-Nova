import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";
import ts from "typescript";

const catalog = JSON.parse(readFileSync(new URL("../../pack-in/materials/minecraft-26.3.json", import.meta.url), "utf8"));
const iconManifest = JSON.parse(readFileSync(new URL("../../pack-in/materials/item-icons-26.3.json", import.meta.url), "utf8"));
const chinese = JSON.parse(readFileSync(new URL("../../pack-in/lang/zh_cn.json", import.meta.url), "utf8"));
function load(file, mocks) {
  const source = readFileSync(new URL(file, import.meta.url), "utf8");
  const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } });
  const exports = {};
  vm.runInNewContext(outputText, { exports, require: id => {
    if (!(id in mocks)) throw new Error(`unexpected dependency ${id}`);
    return mocks[id];
  }, crypto:{randomUUID:()=>"test-stats-operation"}, performance, console, Map, Number });
  return exports;
}

test("catalog pins actual 26.3 registries, aliases, Chinese names and non-64 stacks", () => {
  assert.equal(catalog.minecraft_version, "26.3");
  assert.equal(Object.keys(catalog.blocks).length, 1286);
  assert.equal(Object.keys(catalog.items).length, 1658);
  assert.equal(catalog.blocks["minecraft:redstone_wire"].item_id, "minecraft:redstone");
  assert.equal(catalog.blocks["minecraft:oak_wall_sign"].item_id, "minecraft:oak_sign");
  assert.equal(chinese[catalog.items["minecraft:redstone"].translation_key], "红石粉");
  assert.equal(catalog.items["minecraft:oak_sign"].stack_size, 16);
  assert.equal(catalog.items["minecraft:conduit"].stack_size, 64);
});

test("statistics service accepts Rust views unchanged and sends export settings instead of assembling files", async () => {
  const requests=[], exports=[], recipeRequests=[];
  const view={totalNonAirBlocks:8,materialCount:2,density:.5};
  const service=load("../src/services/statsService.ts", {
    "../platform/dialogs": { saveDialog: async () => "materials.csv" },
    "../platform/applicationOperations": {
      getProjectStats: async input => {requests.push(input);return {source_revision:"source-A",statisticsView:view};},
      exportMaterialList: async input => {exports.push(input);return {output:input.outputPath};},
      queryMaterialRecipes: async (input, operationId) => { recipeRequests.push({input,operationId}); return {sourceRevision:"source-A",summary:{totalMaterials:3,materialsWithRecipes:1,materialsWithoutRecipes:2,unresolvedMaterials:1,excludedMaterials:0}}; },
      prepareMaterialRecipes:async()=>({status:"available",has_recipes:true}),
    },
    "./filePathIdentity": {windowsPathKey:value=>value.toLowerCase()},
    "./i18n": { t:key=>key, getUiLocale:()=>"zh-CN",getI18nRevision:()=>1, getMinecraftTranslations:()=>({"item.minecraft.potion":"药水"}),initI18n:async()=>{} },
  });
  const result=await service.loadStructureStats("projection.litematic",true);
  assert.equal(result,view);assert.equal(requests[0].includeContainerItems,true);
  assert.deepEqual(Object.keys(requests[0].languageMap),["item.minecraft.potion"]);
  assert.throws(()=>service.mapStructureStats({material_items:[]}),/displayDataMissing/);
  await service.exportMaterialListWithOptions("projection.litematic",{scope:{kind:"all",value:null},includeContainerItems:true,multiplier:2,minecraftVersion:"26.3",terminalRawMaterials:false,format:"csv"});
  assert.equal(exports.length,1);assert.equal(exports[0].multiplier,2);assert.equal(exports[0].outputPath,"materials.csv");
  const recipes=await service.fetchMaterialRecipeCache("projection.litematic",true,"recipes-batch-operation");
  assert.equal(recipeRequests.length,1);assert.equal(recipeRequests[0].operationId,"recipes-batch-operation");
  assert.equal(recipeRequests[0].input.sourcePath,"projection.litematic");assert.equal(recipeRequests[0].input.includeContainerItems,true);
  assert.equal(recipes.summary.materialsWithRecipes,1);assert.equal(recipes.summary.materialsWithoutRecipes,2);assert.equal(recipes.summary.unresolvedMaterials,1);
  assert.equal(await service.fetchMaterialRecipeCache(undefined,false,"global-cache-operation"),null);
  const page=readFileSync(new URL("../ui/windows/main/pages/statistics/StatisticsPage.tsx",import.meta.url),"utf8");
  assert.match(page,/fetchMaterialRecipeCache\(currentFile, includeContainerItems, operationId\)/);
  assert.match(page,/result.summary.materialsWithoutRecipes/);assert.match(page,/recipeOperation.current !== operationId/);
  const source=readFileSync(new URL("../src/services/statsService.ts",import.meta.url),"utf8");
  assert.doesNotMatch(source,/formatMaterialUnits|csvEscape|writeTextFileAbsolute|filterMaterialItems|mapMaterial\(/);
});

test("icons query the active resource and retain stable identities without unrelated substitutes", async () => {
  const requests = [];
  const icons = load("../src/services/blockIconResolver.ts", {
    "../platform/events": {listenEvent:async()=>()=>{}},
    "../platform/files": { readGameDataIcon: async id => { requests.push(id); return id === "minecraft:redstone" ? "real-item" : null; } },
    "./gameResources": {getActiveIconSearchRoots:async()=>[],readIconFromRoot:async()=>null,getActiveResourceFingerprint:async()=>"active"},
    "./enumeratorService": {loadEnumeratorCollections:async()=>[]},
  });
  assert.equal(await icons.getBlockIconDataUrl("minecraft:redstone"), "real-item");
  assert.deepEqual(requests, ["minecraft:redstone"]);
  assert.equal(await icons.getBlockIconDataUrl("minecraft:stone_slab"), null);
  assert.ok(requests.every(id => !id.includes("planks")));
  assert.equal(await icons.getBlockIconDataUrl("other:stone"), null);
  assert.equal(requests.at(-1),"other:stone");
});
