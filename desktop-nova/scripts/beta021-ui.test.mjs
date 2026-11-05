import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { harness, nextTurn, text } from "./fixtures/beta-page-harness.mjs";

function service(name, dependencies = {}) {
  const result = ts.transpileModule(readFileSync(new URL(`../src/services/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true },
  });
  const exports = {};
  vm.runInNewContext(result.outputText, { exports, require(id) {
    if (id in dependencies) return dependencies[id];
    throw new Error(`Unexpected service dependency ${id}`);
  } });
  return exports;
}
const paths = service("filePathIdentity");

test("调试开关仅显式true开启，旧UI测试配置不构成新发行版默认开启", async () => {
  const writes = [];
  const config = service("userConfig", { "./backend": { saveUserConfig: async input => { writes.push(input); return { config: input }; } }, "./renderMode": {} });
  for (const value of [undefined, null, false, "true", 1]) assert.equal(config.normalizeDebugMode(value), false);
  assert.equal(config.normalizeDebugMode(true), true);
  await config.saveDebugModeConfig(true); await config.saveDebugModeConfig(false);
  assert.deepEqual(JSON.parse(JSON.stringify(writes)), [{ debug_mode: true }, { debug_mode: false }]);
});

test("展示路径和错误内路径处理扩展UNC/盘符，身份去重且不改变原IO输入", () => {
  const local = String.raw`\\?\C:\中文 文件\x.litematic`;
  const unc = String.raw`\\?\UNC\server\share\中文 文件\x.litematic`;
  assert.equal(paths.displayFilePath(local), String.raw`C:\中文 文件\x.litematic`);
  assert.equal(paths.displayFilePath(unc), String.raw`\\server\share\中文 文件\x.litematic`);
  assert.equal(paths.windowsPathKey(unc), paths.windowsPathKey(String.raw`\\SERVER\share\中文 文件\x.litematic`));
  assert.equal(paths.displayPathText(`读取失败：${unc}\n输出：${local}`), String.raw`读取失败：\\server\share\中文 文件\x.litematic` + "\n" + String.raw`输出：C:\中文 文件\x.litematic`);
  assert.equal(paths.displayFilePath("/Case/file"), "/Case/file");
  assert.notEqual(paths.windowsPathKey("/Case/file"), paths.windowsPathKey("/case/file"));
  assert.equal(local.charAt(2), "?"); assert.equal(unc.charAt(2), "?");
});

test("创造物品选择器和两个场景快捷栏使用 Rust 资源身份投影；选择值不改写", () => {
  const picker = readFileSync(new URL("../ui/components/BlockPickerDialog.tsx", import.meta.url), "utf8");
  const flake = readFileSync(new URL("../ui/windows/main/pages/flake/FlakePage.tsx", import.meta.url), "utf8");
  const menu = readFileSync(new URL("../ui/windows/main/pages/flake/creativeInventoryDialog.tsx", import.meta.url), "utf8");
  for (const source of [picker, flake, menu]) {
    assert.match(source, /useCreativeIdentities\(/);
    assert.doesNotMatch(source, /minecraft-26\.3\.json|creativeItemIdentity/);
    assert.match(source, /lookupMode="item_first"/);
  }
  assert.match(picker, /onPickBlock\(blockId\); onClose\(\)/);
});

test("枚举器生产多页面资源指向真实独立组件，不引用主壳或关闭恢复", () => {
  const config = readFileSync(new URL("../vite.config.ts", import.meta.url), "utf8");
  assert.match(config, /enumerator:\s*resolve\(__dirname,\s*"enumerator.html"\)/);
  const html = readFileSync(new URL("../enumerator.html", import.meta.url), "utf8");
  const entry = html.match(/src="([^"]+main.tsx)"/)?.[1];
  assert.equal(entry, "/ui/windows/enumerator/main.tsx");
  assert.ok(existsSync(new URL(`..${entry}`, import.meta.url)));
  const bootstrap = readFileSync(new URL(`..${entry}`, import.meta.url), "utf8");
  assert.match(bootstrap, /<EnumeratorWindow\s*\/>/);
  assert.doesNotMatch(bootstrap, /AppShell|useApplicationCloseRecovery/);
});

test("标准/完整设为缩略图使用当前模式、持久写回链和更新事件，不触发Viewer重启", async () => {
  for (const mode of ["standard", "full"]) {
    const captures = [], cache = [], events = [], launches = [], presents = [];
    const source = String.raw`\\?\C:\投影测试\同一文件.litematic`;
    const operation = { operationId: `current-${mode}`, source: { path: source }, displayMode: mode, buildMode: "realtime", state: "viewer_prepared", presentation: { mode: "embedded", visible: true } };
    const facade = { findRenderOperationForSource: () => operation, isApplicationViewerPrepared: () => true, isApplicationPresentationVisible: () => true,
      updateApplicationRenderPresentation: async (...args) => presents.push(args), startApplicationRender: async (...args) => launches.push(args),
      renderPreviewImage: async (...args) => { captures.push(args); return { preview_path: "derived.png", data_url: "data:embedded" }; },
      readProjectionPreviewImage: async () => ({ data_url: "data:embedded" }),
      loadLibrary: async () => ({ records: [] }), setRecordPreview: async (...args) => cache.push(args),
      _emitEvent: async (...args) => events.push(args) };
    const h = harness("render/RenderPage", "RenderPage", facade); const props = { currentFile: source };
    h.render(props); h.button(mode === "standard" ? "标准模式" : "完整模式").props.onClick(); h.render(props); await nextTurn(); h.render(props);
    presents.length = 0;
    assert.equal(h.button("设为缩略图").props.disabled, false);
    h.button("设为缩略图").props.onClick(); await nextTurn(); h.render(props);
    assert.equal(captures.length, 1); assert.equal(captures[0][0], source); assert.equal(captures[0][1], mode); assert.equal(captures[0][3], operation.operationId);
    assert.equal(cache[0][1], source);
    assert.deepEqual(JSON.parse(JSON.stringify(events)), [["projection-library-state-changed", { reason: "embedded-preview", path: source }]]);
    assert.equal(launches.length, 0); assert.equal(presents.length, 0);
    assert.match(text(h.render(props)), /缩略图已保存/);
  }
});

test("主页与选项只显示主壳提供的真实构建版本，库不读取旧外部PNG作为权威源", () => {
  for (const file of ["home/HomePage", "settings/SettingsPage"]) {
    const ui = readFileSync(new URL(`../ui/windows/main/pages/${file}.tsx`, import.meta.url), "utf8");
    assert.match(ui, /buildVersion/); assert.doesNotMatch(ui, /Beta 0\.1|三维预览仍为实验|settings-help-text/);
  }
  const library = readFileSync(new URL("../ui/windows/main/pages/library/LibraryPage.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(library, /readImageBase64|previewDataUrls|查看内嵌图|查看渲染图/);
  assert.match(library, /readProjectionPreviewImage\(record.path\)/);
});

test("子窗启动与选择器和投影库错误只经路径展示层，不改变错误或IO来源", () => {
  for (const file of ["windows/asset_manager/main.tsx", "windows/enumerator/main.tsx", "windows/material_list/main.tsx"]) {
    const ui = readFileSync(new URL(`../ui/${file}`, import.meta.url), "utf8");
    assert.match(ui, /displayPathText\(String\(error\)\)/);
    assert.doesNotMatch(ui, />\{String\(error\)\}</);
  }
  for (const file of ["components/BlockPickerDialog.tsx", "windows/main/pages/flake/creativeInventoryDialog.tsx", "windows/main/pages/library/categoryIcon.tsx"]) {
    const ui = readFileSync(new URL(`../ui/${file}`, import.meta.url), "utf8");
    assert.match(ui, /displayPathText\(error\)/);
    assert.doesNotMatch(ui, />\{error\}</);
  }
  const library = readFileSync(new URL("../ui/windows/main/pages/library/LibraryPage.tsx", import.meta.url), "utf8");
  assert.match(library, /displayPathText\(previewError\)/);
  const original = String.raw`读取失败：\\?\UNC\主机\共享\中文 文件.litematic`;
  assert.equal(paths.displayPathText(original), String.raw`读取失败：\\主机\共享\中文 文件.litematic`);
  assert.ok(original.includes("?"));
});

test("洞穴藤蔓头身复用原版glow_berries创造身份而材料AIR保持不变", () => {
  const catalog = JSON.parse(readFileSync(new URL("../../pack-in/materials/minecraft-26.3.json", import.meta.url), "utf8"));
  for (const id of ["minecraft:cave_vines", "minecraft:cave_vines_plant"]) {
    assert.equal(catalog.blocks[id].creative_item_id, "minecraft:glow_berries");
  }
  assert.equal(catalog.blocks["minecraft:cave_vines_plant"].item_id, "minecraft:air");
  assert.equal(catalog.blocks["minecraft:cave_vines"].item_id, "minecraft:glow_berries");
  const generator = readFileSync(new URL("../../tools/mc26-oracle/src/main/java/net/litematicaba/mc26oracle/MaterialCatalogMain.java", import.meta.url), "utf8");
  assert.match(generator, /classes\.contains\("CaveVinesPlantBlock"\)/);
  assert.match(generator, /"GLOW_BERRIES"/);
  assert.match(generator, /row\.put\("creative_item_id"/);
});

test("Fast截图IPC传原路径与精确previewId，后端拒绝缺失或其他当前场景ID", async () => {
  const calls = [], exports = {};
  const source = readFileSync(new URL("../src/platform/previewImage.ts", import.meta.url), "utf8");
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  vm.runInNewContext(js, { exports, require(id) {
    assert.equal(id, "./tauri");
    return { invoke: async (command, input) => { calls.push({ command, input }); return { preview_path: "derived.png", data_url: "data:embedded" }; } };
  } });
  const rawPath = String.raw`\\?\UNC\host\share\中文 同一文件.litematic`;
  await exports.renderPreviewImage(rawPath, "fast_experimental", undefined, "exact-fast-session-generation");
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [{ command: "render_preview_image", input: {
    filePath: rawPath, displayMode: "fast_experimental", cacheInput: null, operationId: "exact-fast-session-generation",
  } }]);
  const backend = readFileSync(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8");
  const handler = backend.split("async fn render_preview_image(")[1].split("async fn generate_preview_image(")[0];
  const fast = handler.split('if display_mode == "fast_experimental" {')[1].split("let mode = match")[0];
  assert.match(fast, /let preview_id = operation_id\.ok_or/);
  assert.match(fast, /preview_id == view\.preview_id/);
  assert.doesNotMatch(fast, /is_none_or/);
  assert.match(handler, /facade\.capture_frame\(&OperationId\(id\), &source, mode\)/);
  assert.match(fast, /persist_rendered_preview\([^;]+"fast_experimental"\)/s);
  assert.match(handler, /persist_rendered_preview\([^;]+&display_mode\)/s);
  const persist = backend.split("fn persist_rendered_preview(")[1].split("#[tauri::command]")[0];
  assert.match(persist, /"preview_image_mode": mode/);
  const generated = backend.split("async fn generate_preview_image(")[1].split("async fn application_export_material_list(")[0];
  assert.match(generated, /persist_rendered_preview\([^;]+&preview_mode\)/s);
  assert.doesNotMatch(generated, /if let Some\(preview\) = operations\.read_embedded_preview/);
});

test("枚举表达式只在集合变更时绑定一次，后续键入仅送快照与文本", async () => {
  const binds = [], queries = [];
  const api = service("enumeratorService", {
    "../platform/files": {}, "./gameResources": {}, "./backend": {}, "./i18n": {t:(key, params) => key + ":" + (params?.token || "")},
    "../platform/applicationOperations": {
      bindEnumeratorCollections: async collections => { binds.push(collections); return binds.length; },
      queryEnumeratorExpression: async (id, expression) => { queries.push([id, expression]); return {values:[expression], dependencies:["a"], errorKey: null, token:null}; },
    },
  });
  const collections = [{id:"id", name:"中文", symbol:"A", values:["z","a","m"], description:"does not cross IPC"}];
  const first = api.evaluateEnumeratorExpression("A", collections);
  const second = api.evaluateEnumeratorExpression("A|A", collections);
  assert.deepEqual([...(await first).values], ["A"]);
  assert.deepEqual([...(await second).values], ["A|A"]);
  assert.equal(binds.length, 1);
  assert.deepEqual(Object.keys(binds[0][0]).sort(), ["id","name","symbol","values"]);
  await api.evaluateEnumeratorExpression("A", [...collections]);
  assert.deepEqual(queries, [[1,"A"],[1,"A|A"],[2,"A"]]);
});
