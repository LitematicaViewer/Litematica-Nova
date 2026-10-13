import assert from "node:assert/strict";
import test from "node:test";
import { harness, text, nextTurn, deferred } from "./fixtures/beta-page-harness.mjs";

const metadata = {
  metadata: { name: "原名称", author: "原作者", total_blocks: 10, total_volume: 20 },
  regions: [{ name: "main", position: { x: 0, y: 0, z: 0 }, size: { x: 2, y: 5, z: 2 } }],
};
const propertiesFacade = {
  subscribeApplicationRender: () => () => {}, getApplicationRenderState: () => ({}),
  findRenderOperationForSource: () => null, isApplicationRenderPending: () => false, isApplicationViewerPrepared: () => false,
  analyzeProjectionFile: async () => metadata,
  loadLibrary: async () => ({}), addOrUpdateRecordFromAnalysis: async () => {}, projectionRecordAnalysis: (value) => value,
};

test("属性：撤销只恢复未保存内容，重复保存只提交一次", async () => {
  let writes = 0; const gate = deferred();
  const h = harness("properties/PropertiesPage", "PropertiesPage", { ...propertiesFacade,
    saveProjectionMetadataPatch: async () => { writes++; await gate.promise; return "ok"; },
  });
  const props = { currentFile: "C:\\fixtures\\sample.litematic", setCurrentFile() {}, setRoute() {} };
  h.render(props); await nextTurn(); h.render(props);
  h.component("TextRow").props.onChange("修改后"); h.render(props);
  assert.equal(h.button("保存修改").props.disabled, false);
  h.button("撤销未保存修改").props.onClick(); h.render(props);
  assert.equal(h.component("TextRow").props.value, "原名称");
  assert.equal(writes, 0);
  h.component("TextRow").props.onChange("新名称"); h.render(props);
  const save = h.button("保存修改").props.onClick;
  const first = save(); await save();
  assert.equal(writes, 1);
  h.render(props);
  assert.equal(h.nodes().find((node) => node.type === "fieldset").props.disabled, true);
  gate.resolve(); await first; h.render(props);
  assert.equal(h.button("保存修改").props.disabled, true);
});

test("属性：取消另存不写入，文件切换后不回填旧属性", async () => {
  let writes = 0; const gate = deferred();
  const h = harness("properties/PropertiesPage", "PropertiesPage", { ...propertiesFacade,
    analyzeProjectionFile: async (path) => path === "first.litematic" ? gate.promise : metadata,
    selectLitematicSavePath: async () => null,
    saveProjectionMetadataPatch: async () => { writes++; },
  });
  const props = { currentFile: "first.litematic", setCurrentFile() {}, setRoute() {} };
  h.render(props); props.currentFile = "second.litematic"; h.render(props); await nextTurn(); h.render(props);
  gate.resolve({ ...metadata, metadata: { name: "过期内容" } }); await nextTurn(); h.render(props);
  assert.equal(h.component("TextRow").props.value, "原名称");
  await h.button("另存副本…").props.onClick(); assert.equal(writes, 0);
});


test("属性：渲染分析迟到不清空同一文件的未保存编辑", async () => {
  let operation = null;
  const h = harness("properties/PropertiesPage", "PropertiesPage", { ...propertiesFacade,
    findRenderOperationForSource: () => operation,
  });
  const props = { currentFile: "sample.litematic", setCurrentFile() {}, setRoute() {} };
  h.render(props); await nextTurn(); h.render(props);
  h.component("TextRow").props.onChange("还未保存的名称"); h.render(props);
  operation = { operationId: "render-later", state: "viewer_prepared", analysis: { ...metadata, structure_stats: { total_non_air_blocks: 10 } }, progress: {}, presentation: { mode: "hidden" } };
  h.render(props); await nextTurn(); h.render(props);
  assert.equal(h.component("TextRow").props.value, "还未保存的名称");
  assert.equal(h.button("保存修改").props.disabled, false);
});

test("属性：快速渲染只请求一次独立 fast，Full 始终是禁用占位", async () => {
  const requests = []; const gate = deferred(); let operation = null;
  const h = harness("properties/PropertiesPage", "PropertiesPage", { ...propertiesFacade,
    getQuickPreviewState: () => operation,
    startQuickPreview: async (sourcePath) => { requests.push(sourcePath); return gate.promise; },
    startApplicationRender: () => { throw new Error("不得进入 Full/Realtime"); },
  });
  const props = { currentFile: "sample.litematic", setCurrentFile() {}, setRoute() {} };
  h.render(props); await nextTurn(); h.render(props);
  const click = h.button("快速渲染").props.onClick; click(); click();
  assert.deepEqual(requests, [props.currentFile]); assert.equal(h.button("完整模式").props.disabled, true);
  assert.ok(h.nodes().some((node) => text(node.props?.children).includes("完整模式未包含在公开版")));
  gate.resolve({ previewId: "quick-running" }); await nextTurn();
  operation = { previewId: "quick-running", sourcePath: props.currentFile, state: "starting", renderMode: "fast_experimental", presentation: { mode: "hidden" }, error: null };
  h.render(props); assert.equal(h.button("正在加载…").props.disabled, true); assert.equal(requests.length, 1);
});

test("渲染页：同一快速入口，完整模式无可触发操作", async () => {
  const requests = []; let presentations = 0; let operation = null;
  const h = harness("render/RenderPage", "RenderPage", {
    getQuickPreviewState: () => operation,
    startQuickPreview: async (path) => { requests.push(path); return { previewId: "quick-one" }; },
    _viewport: { setPresentation: async () => { presentations++; } },
  });
  const props = { currentFile: "sample.litematic" };
  h.render(props); h.button("快速渲染").props.onClick(); await nextTurn(); h.render(props);
  assert.deepEqual(requests, [props.currentFile]);
  const full = h.button("完整模式"); assert.equal(full.props.disabled, true); assert.equal(full.props.onClick, undefined);
  assert.equal(h.button("构建 / 恢复"), undefined);
  operation = { previewId: "quick-one", sourcePath: props.currentFile, state: "ready", renderMode: "fast_experimental", presentation: { mode: "embedded", visible: true }, error: null };
  h.render(props); assert.equal(h.button("独立窗口"), undefined); assert.equal(presentations, 0);
});
