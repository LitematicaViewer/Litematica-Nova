import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const read = path => fs.readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
function load(path) {
  const exports = {};
  const output = ts.transpileModule(read(path), { compilerOptions: {
    module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React,
  }}).outputText;
  vm.runInNewContext(output, { exports, require: name => name === 'react'
    ? { default: React } : name.includes('i18n') ? { useI18n: () => ({ locale: 'zh-CN' }) }
    : { Button: ({ children, variant, ...props }) => React.createElement('button', props, children) } });
  return exports;
}

test('界面倍率：完整区间、精确输入、0.05步进，非法值不静默截断', () => {
  const api = load('ui/windows/main/pages/settings/uiScaleInput.ts');
  for (const value of [.5, .75, 1, 1.25, 2, 3, 4, 5, 1.23]) assert.equal(api.parseUiScaleDraft(value.toFixed(2)), value);
  for (const value of ['', 'NaN', 'Infinity', '.49', '0.49', '5.01', '1.234', 'abc']) assert.equal(api.parseUiScaleDraft(value), null);
  assert.equal(api.stepUiScale(1.23, 1), 1.28);
  assert.equal(api.stepUiScale(.5, -1), null);
  assert.equal(api.stepUiScale(5, 1), null);
  assert.equal(api.UI_SCALE_PRESETS.at(-1), 5);
});

test('目录和旧路由：六个工作台入口与七类设置均真实保留', () => {
  const workbench = load('ui/shell/WorkbenchNavigation.tsx');
  for (const route of ['properties', 'render', 'flake', 'statistics', 'replace', 'composition']) assert.equal(workbench.isWorkbenchRoute(route), true);
  assert.equal(workbench.isWorkbenchRoute('settings'), false);
  const html = renderToStaticMarkup(React.createElement(workbench.WorkbenchNavigation, { route: 'render', onNavigate() {} }));
  assert.equal((html.match(/<button/g) ?? []).length, 6);
  const settings = load('ui/windows/main/pages/settings/SettingsCategories.tsx');
  const categories = renderToStaticMarkup(React.createElement(settings.SettingsCategories, { category: 'appearance', onSelect() {} }));
  assert.equal((categories.match(/<button/g) ?? []).length, 7);
  for (const label of ['外观', '数据与目录', '渲染', '光影与资源包', 'AI', '高级与诊断', '维护']) assert.ok(categories.includes(label));
});

test('Host全局缩放：新开窗口、已开窗口、保存和原生恢复共享同一配置', () => {
  const host = read('src-tauri/src/main.rs');
  const zoom = read('src-tauri/src/application/ui_scale.rs');
  assert.ok(host.includes('webview.set_zoom(read_user_config_data(&dir).ui_scale)'));
  assert.ok(host.includes('application::ui_scale::apply(&app, info.config.ui_scale)?'));
  assert.ok(zoom.includes('for window in app.webview_windows().values()'));
  assert.ok(zoom.includes('CmdOrCtrl+Alt+0'));
  assert.ok(zoom.includes('crate::save_user_config_inner(input)'));
  assert.ok(!zoom.includes('build_scene'));
  assert.ok(read('src/platform/nativeWindowGeometry.ts').includes('listenEvent<number>("nova-ui-scale-changed", notify)'));
  assert.ok(!read('ui/shell/workspace.css').includes('transform: scale'));
});

test('原生视口：各UI倍率×系统DPI只换算一次，不改变几何身份', async () => {
  for (const uiScale of [.5, .75, 1, 1.25, 2, 3, 4, 5]) {
    for (const osDpi of [1, 1.25, 1.5, 2]) {
      const exports = {};
      const code = ts.transpileModule(read('ui/windows/main/embeddedViewerGeometry.ts'), { compilerOptions: { module: ts.ModuleKind.CommonJS }}).outputText;
      const dpr = uiScale * osDpi;
      vm.runInNewContext(code, { exports, require: () => ({
        getApplicationWindowOrigin: async () => ({ x: 0, y: 0 }), getApplicationWindowVisibility: async () => true,
      }), document: { hasFocus: () => true }, window: { devicePixelRatio: dpr, innerWidth: 1000, innerHeight: 800 } });
      const result = await exports.elementToNativeChildPlacement({ parentElement: null,
        getBoundingClientRect: () => ({ left: 10, top: 20, right: 110, bottom: 70 }) });
      assert.equal(result.width, Math.round(100 * dpr));
      assert.equal(result.height, Math.round(50 * dpr));
    }
  }
});
