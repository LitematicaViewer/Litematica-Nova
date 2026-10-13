import assert from "node:assert/strict";
import test from "node:test";
import { harness, text, nextTurn, deferred } from "./fixtures/beta-page-harness.mjs";

const generateSummary = { regions_count: 1, total_volume: 64, estimated_non_air_blocks: 16, material_summary: [], palette_entries_per_region: [], warnings: [] };
const generateFacade = {
  translateBlockId: (id) => id, getAllBlocks: () => [], getBlockProperties: () => ({}), getDefaultProperties: () => ({}),
  createDefaultOperation: (type) => ({ id: type, type, block: { name: "minecraft:stone", properties: {} }, from: [0, 0, 0], to: [1, 1, 1] }),
  buildProjectionPlan: (form) => form, validatePlanClientSide: () => [], loadDatabases: async () => {}, loadGenerationTemplates: async () => [],
  getDefaultProjectionOutputPath: async () => "C:/fixtures/generated.litematic",
  loadLibrary: async () => ({}), addOrUpdateRecord: async () => {}, formatGeneratePathLog: () => "path checked",
};

test("生成：必须预览，编辑后失效；双击预览只调用一次", async () => {
  let previews = 0; const gate = deferred();
  const h = harness("generate/GeneratePage", "GeneratePage", { ...generateFacade,
    dryRunGenerate: async () => { previews++; await gate.promise; return { summary: generateSummary, diagnostics: "ok" }; },
  });
  const props = { currentFile: "", setCurrentFile() {}, setRoute() {} };
  h.render(props); assert.equal(h.button("生成投影文件").props.disabled, true);
  const preview = h.button("预览生成结果").props.onClick;
  const first = preview(); await preview(); assert.equal(previews, 1);
  gate.resolve(); await first; h.render(props);
  assert.equal(h.button("生成投影文件").props.disabled, false);
  h.component("MetadataGroup").props.updateMetadata("name", "新名字"); h.render(props);
  assert.equal(h.button("生成投影文件").props.disabled, true);
  assert.ok(h.component("TemplateLibrary"));
  assert.equal(h.nodes().find((node) => node.type === "pre" && node.props.className === "generate-json-preview"), undefined);
});

test("生成：路径检查等待期间锁定提交，失败后可以重试", async () => {
  let pathChecks = 0; let writes = 0; const gate = deferred();
  const h = harness("generate/GeneratePage", "GeneratePage", { ...generateFacade,
    dryRunGenerate: async () => ({ summary: generateSummary, diagnostics: "ok" }),
    validateOutputPath: async () => { pathChecks++; await gate.promise; throw new Error("测试路径错误"); },
    applyGenerate: async () => { writes++; },
  });
  const props = { currentFile: "", setCurrentFile() {}, setRoute() {} };
  h.render(props); await h.button("预览生成结果").props.onClick(); h.render(props);
  const apply = h.button("生成投影文件").props.onClick;
  const first = apply(); await apply(); assert.equal(pathChecks, 1);
  gate.resolve(); await first; h.render(props);
  assert.equal(writes, 0); assert.equal(h.button("生成投影文件").props.disabled, false);
  assert.ok(h.nodes().some((node) => node.type === "textarea" && node.props.value.includes("测试路径错误")));
});


test("生成：模板读取失败明确提示，重试成功后恢复列表", async () => {
  let attempts = 0;
  const h = harness("generate/GeneratePage", "GeneratePage", { ...generateFacade,
    loadGenerationTemplates: async () => {
      attempts++;
      if (attempts === 1) throw new Error("模板索引读取失败");
      return [{ id: "platform", name_zh: "石砖平台", description_zh: "平台", tags: [], plan: {} }];
    },
  });
  const props = { currentFile: "", setCurrentFile() {}, setRoute() {} };
  h.render(props);
  assert.equal(h.component("TemplateLibrary").props.loading, true);
  await nextTurn(); h.render(props);
  const failed = h.nodes().filter((node) => node.props?.role === "alert");
  assert.ok(failed.some((node) => text(node).includes("模板加载失败")));
  assert.ok(!h.nodes().some((node) => text(node) === "没有匹配模板"));
  h.button("重新加载模板").props.onClick(); h.render(props);
  await nextTurn(); h.render(props);
  assert.equal(attempts, 2);
  assert.equal(h.component("TemplateLibrary").props.loadError, "");
  assert.equal(h.component("TemplateLibrary").props.templates.length, 1);
  assert.ok(h.button("使用此模板"));
});
