import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

function geometry(scale, bounds, ancestors = []) {
  const rect = (r) => ({ ...r, right: r.left + r.width, bottom: r.top + r.height });
  let parentElement = null;
  for (const ancestor of [...ancestors].reverse()) {
    parentElement = { parentElement, getBoundingClientRect: () => rect(ancestor), style: ancestor };
  }
  const element = { parentElement, getBoundingClientRect: () => rect(bounds) };
  const exports = {};
  const code = ts.transpileModule(fs.readFileSync(new URL("../ui/windows/main/embeddedViewerGeometry.ts", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(code, { exports,
    require: () => ({ getApplicationWindowOrigin: async () => ({ x: -100, y: 200 }), getApplicationWindowVisibility: async () => true }),
    window: { devicePixelRatio: scale, innerWidth: 640, innerHeight: 480, getComputedStyle: (parent) => parent.style },
    document: { hasFocus: () => true },
  });
  return { api: exports, element };
}

test("原生子窗口：DPI与WebView缩放仅转换一次，不抢夺HTML焦点", async () => {
  const { api, element } = geometry(1.5, { left: 20, top: 40, width: 400, height: 300 });
  assert.deepEqual(JSON.parse(JSON.stringify(await api.elementToNativeChildPlacement(element))),
    { x: -70, y: 260, width: 600, height: 450, dpiScale: 1.5, focused: false });
});

test("原生子窗口：滚动容器和页面边界裁切视口，不覆盖工具栏", async () => {
  const { api, element } = geometry(2, { left: 20, top: 40, width: 400, height: 300 },
    [{ left: 10, top: 100, width: 390, height: 100, overflowX: "hidden", overflowY: "auto" }]);
  assert.deepEqual(JSON.parse(JSON.stringify(await api.elementToNativeChildPlacement(element))),
    { x: -60, y: 400, width: 760, height: 200, dpiScale: 2, focused: false });
});

test("原生子窗口：完全离开可见区时隐藏，不发布零尺寸surface", async () => {
  const { api, element } = geometry(1, { left: 20, top: 500, width: 400, height: 300 });
  const placement = await api.elementToNativeChildPlacement(element);
  assert.equal(api.isUsableEmbeddedRect(placement), false);
  assert.equal(placement.height, 0);
});

test("快速模式：host来自原生主窗口，UI不再暴露popup/overlay入口", () => {
  const read = (path) => fs.readFileSync(new URL(path, import.meta.url), "utf8");
  assert.ok(read("../ui/windows/main/useQuickPreview.ts").includes('inlineMode: "embedded"'));
  assert.ok(!read("../ui/windows/main/pages/render/RenderPage.tsx").includes("preview.popup"));
  const backend = read("../src-tauri/src/application/quick_preview.rs");
  assert.ok(backend.includes("attach_embedded"));
  assert.ok(!backend.includes("show_overlay"));
  assert.ok(!backend.includes("show_popup"));
});
