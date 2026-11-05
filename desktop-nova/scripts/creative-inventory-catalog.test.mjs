import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
import test from "node:test";
import vm from "node:vm";
import { inflateSync } from "node:zlib";
import { bundledCreativeIdentity } from "./bundled-creative-identity.mjs";

const read = path => readFileSync(new URL(path, import.meta.url), "utf8");
const bundled = JSON.parse(read("../src/services/creativeInventoryBundled.json"));
const fixtureContentId = "f".repeat(64);
const source = stripTypeScriptTypes(read("../src/services/creativeInventoryCatalog.ts"))
  .replace(/^import .*;$/gm, "").replace(/^export /gm, "");
function service(overrides = {}) {
  let id = "vanilla", count = 0;
  const result = vm.runInNewContext(source + "\n;({prepareCreativeInventoryCatalog,filterCreativeItems,bundledCreativeGroups})", {
    bundled, __NOVA_CREATIVE_CATALOG_BINDING__: "nova-catalog-v1:" + fixtureContentId,
    getGameDataContext: async () => ({content_id:id, game_version:"1.21"}),
    readCreativeInventoryCatalog: async ({contentId, version}) => ({contentId, version, categories:[], items:[], count:++count}),
    t: key => key, ...overrides,
  });
  return {...result, setId: value => { id = value; }, calls: () => count};
}

test("prewarm and exact scene share one query; different identity, version, locale, collection or resources do not", async () => {
  const svc = service(), load = svc.prepareCreativeInventoryCatalog;
  const [a, b] = await Promise.all([load({}, "zh-CN"), load({contentId:"vanilla", version:"1.21"}, "zh-CN")]);
  assert.equal(a, b);
  assert.equal(svc.calls(), 1);
  await load({version:"1.20"}, "zh-CN"); await load({}, "en-US");
  await load({extraCollections:[{id:"extra", name:"Mine", values:["minecraft:stone"]}]}, "zh-CN");
  await load({resourceIdentity:"pack-B"}, "zh-CN");
  svc.setId("modded");
  await assert.rejects(load({contentId:"vanilla"}, "zh-CN"), /creativeResourceChanged/);
  assert.equal((await load({}, "zh-CN")).contentId, "modded");
  assert.equal(svc.calls(), 6);
});

test("cold categories match the bundled identity and known versions only", () => {
  const groups = service().bundledCreativeGroups;
  assert.equal(groups(fixtureContentId, "26.3").length, 19);
  assert.equal(groups(fixtureContentId, "1.8").length, 12);
  assert.ok(!groups(fixtureContentId, "1.8").includes("barrel"));
  assert.equal(groups("modded", "26.3").length, 0);
  assert.equal(groups(fixtureContentId, "future").length, 0);
  assert.equal(service({__NOVA_CREATIVE_CATALOG_BINDING__:null}).bundledCreativeGroups(fixtureContentId, "26.3").length, 0);
  assert.equal(bundledCreativeIdentity(new URL("./missing-pack-fixture", import.meta.url).pathname), null);
});

test("mismatched native result is not cached and the same identity can retry", async () => {
  let first = true;
  const svc = service({readCreativeInventoryCatalog: async ({version,contentId}) => {
    const actual = first ? "wrong" : contentId; first = false;
    return {version, contentId:actual, categories:[], items:[]};
  }});
  await assert.rejects(svc.prepareCreativeInventoryCatalog({}, "zh-CN"), /creativeResourceChanged/);
  assert.equal((await svc.prepareCreativeInventoryCatalog({}, "zh-CN")).contentId, "vanilla");
});

test("a new session reuses the persisted native snapshot only for its exact identity", async () => {
  const records = new Map();
  const indexedDB = {open() {
    const request = {result:{transaction() {
      const tx = {objectStore() { return {
        get(slot) { queueMicrotask(() => tx.oncomplete()); return {result:records.get(slot)}; },
        put(value, slot) { records.set(slot, structuredClone(value)); queueMicrotask(() => tx.oncomplete()); return {}; },
      }; }};
      return tx;
    }}};
    queueMicrotask(() => request.onsuccess());
    return request;
  }};
  const first = service({indexedDB});
  await first.prepareCreativeInventoryCatalog({}, "zh-CN");
  assert.equal(records.size, 1);
  const next = service({indexedDB});
  await next.prepareCreativeInventoryCatalog({}, "zh-CN");
  assert.equal(next.calls(), 0);
  next.setId("modded");
  await next.prepareCreativeInventoryCatalog({}, "zh-CN");
  assert.equal(next.calls(), 1);
  assert.equal(records.size, 1);
});

test("full group and search include the last native typed state, not only a 45- or 256-item page", () => {
  const items = Array.from({length:302}, (_, i) => ({id:`sample:b${i}`, name:`方块${i}`, sourceName:"TestMod",
    itemId:`sample:b${i}`, blockState:`sample:b${i}[axis=x]`, state:{id:`sample:b${i}`,properties:{axis:"x"}}}));
  const data = { items, categories:[{id:"all", indices:items.map((_, i) => i)}, {id:"extra", indices:[301]}] };
  const filter = service().filterCreativeItems;
  assert.equal(filter(data,"all","").length, 302);
  assert.equal(filter(data,"all","方块301")[0], items[301]);
  assert.equal(filter(data,"extra","AXIS=X")[0].state, items[301].state);
  assert.equal(filter(data,"all","testmod").length, 302);
  assert.equal(filter(data,"extra","sample:b0").length, 0);
});

test("every category in the shipped exact-version registries, plus the Mod overlay, has Chinese and English labels", () => {
  const db = inflateSync(readFileSync(new URL("../../data/blockdb/blocks.dat.z", import.meta.url)));
  const trailer = db.lastIndexOf("NOVAST04");
  assert.notEqual(trailer, -1);
  const registries = JSON.parse(inflateSync(db.subarray(trailer + 12))).registries;
  const labels = read("../src/services/locales/editor.ts");
  const [zh,en] = labels.split("export const enUS");
  const ids = new Set(["mod"]);
  for (const registry of Object.values(registries)) for (const block of Object.values(registry.blocks)) if (block.category) ids.add(block.category);
  for (const id of ids) {
    assert.match(zh, new RegExp(`"flake\\.category\\.${id}": "[^"]*[\\u3400-\\u9fff]`));
    assert.match(en, new RegExp(`"flake\\.category\\.${id}": "[A-Za-z ]+"`));
  }
});
