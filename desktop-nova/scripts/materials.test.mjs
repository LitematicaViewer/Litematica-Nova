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
  }, console, Map, Number });
  return exports;
}
let statsCalls = 0;
let csv = "";
const service = load("../src/services/statsService.ts", {
  "../platform/dialogs": { saveDialog: async () => "materials.csv" },
  "../platform/applicationOperations": { getProjectStats: async () => { statsCalls++; return {}; }, getProjectMaterials: async () => ({material_items:[]}) },
  "./backend": { writeTextFileAbsolute: async (_path, text) => { csv = text; } },
  "./i18n": { translateBlockId: id => id, translateBuildingType: id => id },
});
function material(id, count) {
  const item = catalog.items[id];
  return service.mapStructureStats({ material_items: [{ block_id: id, display_name: chinese[item.translation_key], total_count: count, block_count: count, stack_size: item.stack_size, can_fit_container: item.can_fit_container, material_status: "item" }] }).materials[0];
}
test("catalog pins actual 26.3 registries, aliases, Chinese names and non-64 stacks", () => {
  assert.equal(catalog.minecraft_version, "26.3");
  assert.equal(Object.keys(catalog.blocks).length, 1286);
  assert.equal(Object.keys(catalog.items).length, 1658);
  assert.equal(catalog.blocks["minecraft:redstone_wire"].item_id, "minecraft:redstone");
  assert.equal(catalog.blocks["minecraft:oak_wall_sign"].item_id, "minecraft:oak_sign");
  assert.equal(material("minecraft:redstone", 4).name, "红石粉");
  assert.equal(material("minecraft:oak_sign", 17).stackSize, 16);
  assert.equal(material("minecraft:conduit", 65).stackSize, 64);
});
test("quantity display uses item capacity and switching to individuals is purely local", () => {
  const item = material("minecraft:oak_sign", 16 * (729 + 27 + 1) + 3);
  assert.equal(service.formatMaterialQuantity(item, item.totalCount), "1箱盒 1盒 1组 3个");
  const before = statsCalls;
  assert.equal(service.formatMaterialQuantity(item, item.totalCount, true), "12,115 个");
  assert.equal(statsCalls, before);
  assert.equal(service.formatMaterialUnits(0), "0个");
  assert.equal(service.formatMaterialUnits(NaN), "0个");
});
test("non-nestable boxes and component-customized stacks do not claim shulker packing", () => {
  const box = material("minecraft:shulker_box", 28);
  assert.equal(service.formatMaterialQuantity(box, 28), "28 个");
  const item = { ...material("minecraft:stone", 64), hasCustomStackSize: true };
  assert.equal(service.formatMaterialQuantity(item, 64), "64 个");
  const fluid = { ...item, materialStatus: "no_item" };
  assert.equal(service.formatMaterialQuantity(fluid, 64), "64 方块");
});
test("CSV and table quantity semantics agree", async () => {
  const sign = material("minecraft:oak_sign", 17);
  await service.exportMaterials("sign.litematic", [sign], 2);
  assert.ok(csv.includes("橡木告示牌,34,2组 2个"));
});
test("old renderer summaries are not falsely treated as item requirements", () => {
  assert.equal(service.hasInventoryMaterialSemantics({ material_items: [] }), false);
  assert.equal(service.hasInventoryMaterialSemantics({ material_semantics: "minecraft-26.3-inventory-v1" }), false);
  assert.equal(service.hasInventoryMaterialSemantics({ material_semantics: "minecraft-26.3-inventory-v2" }), true);
});
test("missing derived summaries stay unavailable while explicit zero stays zero", () => {
  const absent = service.mapStructureStats({});
  assert.equal(absent.redstoneRatio, null);
  assert.equal(absent.fluidRatio, null);
  assert.equal(absent.buildingType, "未提供");
  const available = service.mapStructureStats({ derived: { building: { redstone_ratio: 0, fluid_ratio: .125, building_type: "redstone" } } });
  assert.equal(available.redstoneRatio, 0);
  assert.equal(available.fluidRatio, .125);
  assert.equal(available.buildingType, "redstone");
});
test("icons resolve actual aliases, prefer same-item icon, and never replace slab with planks", async () => {
  const requests = [];
  const icons = load("../src/services/blockIconResolver.ts", {
    "../platform/files": { readImageBase64: async path => { requests.push(path); if (path === "pack-in/item-icons-26.3/redstone.png") return "real-item"; throw Error("missing"); } },
    "../../../pack-in/materials/minecraft-26.3.json": catalog,
    "../../../pack-in/materials/item-icons-26.3.json": iconManifest,
  });
  assert.equal(await icons.getBlockIconDataUrl("minecraft:redstone_wire"), "real-item");
  assert.deepEqual(requests, ["pack-in/item-icons-26.3/redstone.png"]);
  assert.equal(await icons.getBlockIconDataUrl("minecraft:stone_slab"), null);
  assert.ok(requests.every(path => !path.includes("planks")));
  assert.equal(Array.from(icons.iconCandidates("minecraft:water")).length, 0);
  assert.equal(Array.from(icons.iconCandidates("minecraft:made_up")).length, 0);
  assert.equal(Array.from(icons.iconCandidates("other:stone")).length, 0);
});

test("AIR-mapped technical blocks are diagnostics, never materials or exported items", async () => {
  const piston = material("minecraft:piston", 2);
  const rows = [
    {block_id: "minecraft:piston", material_status: "item", block_count: 2, total_count: 2},
    {block_id: "minecraft:piston_head", material_status: "no_item", block_count: 2, total_count: 2},
    {block_id: "minecraft:moving_piston", material_status: "no_item", block_count: 1, total_count: 1},
    {block_id: "other:unverified", material_status: "unknown", block_count: 3, total_count: 3},
  ];
  const data = service.mapStructureStats({structure_stats: {total_non_air_blocks: 8}, material_items: rows});
  assert.equal(data.totalNonAirBlocks, 8);
  assert.deepEqual(Array.from(data.materials, row => row.id), ["minecraft:piston"]);
  assert.deepEqual(Array.from(data.nonItemBlocks, row => [row.id, row.count]), [["minecraft:piston_head", 2], ["minecraft:moving_piston", 1], ["other:unverified", 3]]);
  const fresh = service.mapStructureStats({material_items: rows.slice(0, 1), non_item_blocks: [{block_id:"minecraft:piston_head", display_name:"活塞头", count:2, reason:"no_item"}]});
  assert.equal(fresh.nonItemBlocks[0].name, "活塞头");
  assert.equal(fresh.nonItemBlocks[0].count, 2);
  await service.exportMaterials("piston.litematic", [piston, {...piston, id: "minecraft:piston_head", name:"活塞头", materialStatus:"no_item"}], 1);
  assert.ok(!csv.includes("活塞头"));
  assert.ok(csv.includes("活塞,2"));
});
