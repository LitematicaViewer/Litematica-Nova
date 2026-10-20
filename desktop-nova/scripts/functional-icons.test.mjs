import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import * as icons from 'lucide-react';

const source = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const functionalPages = [
  'ui/windows/local_library_folders/LocalLibraryFoldersContent.tsx',
  'ui/windows/main/pages/flake/FlakePage.tsx',
  'ui/windows/main/pages/home/HomePage.tsx',
  'ui/windows/main/pages/library/LibraryPage.tsx',
  'ui/windows/main/pages/properties/PropertiesPage.tsx',
  'ui/windows/main/pages/render/RenderPage.tsx',
  'ui/windows/main/pages/settings/SettingsPage.tsx',
];

test('导航、投影库、属性与分层功能图标保留可用的 lucide 回退', () => {
  for (const path of functionalPages) {
    const code = source(path);
    const names = /import\s*\{([^}]+)\}\s*from\s*["']lucide-react["']/.exec(code)?.[1];
    assert.ok(names, path);
    for (const name of names.split(',').map(value => value.trim()).filter(value => value && !value.startsWith('type '))) {
      const Icon = icons[name];
      assert.ok(Icon, `${path}: installed lucide export ${name}`);
      const html = renderToStaticMarkup(React.createElement(Icon, { 'aria-hidden': true, focusable: false }));
      assert.match(html, /^<svg /);
      assert.match(html, /stroke="currentColor"/);
      assert.match(html, /aria-hidden="true"/);
      assert.match(html, /viewBox="0 0 24 24"/);
    }
  }
});

test('功能图标可由主题覆盖：SVG 走 currentColor 遮罩，PNG 保留原图，缺失时回退', () => {
  const component = source('ui/components/ThemeIcon.tsx');
  assert.match(component, /resource\/icon\/button\/\*\.\{png,svg\}/);
  assert.match(component, /resource\/icon\/tool\/\*\.\{png,svg\}/);
  assert.match(component, /currentThemeId\(\)/);
  assert.match(component, /maskImage: `url\("\$\{asset\.url\}"\)`/);
  assert.match(component, /theme-icon-image/);
  assert.match(component, /fallback: Fallback/);

  const controls = source('ui/styles/controls.css');
  assert.match(controls, /span\.theme-icon\s*\{[^}]*background-color:\s*currentColor/);
  assert.match(controls, /\.theme-icon\s*\{[^}]*mask-size:\s*contain/);
  assert.match(controls, /\.theme-icon-image\s*\{\s*object-fit:\s*contain/);

  // The asset lookup stays centralised: pages name an icon, they never glob files.
  for (const path of functionalPages) {
    assert.doesNotMatch(source(path), /import\.meta\.glob/, path);
  }
});

test('页面按功能位图清单使用主题图标名', () => {
  const expected = {
    'ui/windows/main/pages/home/HomePage.tsx': ['folder', 'gallery', 'properties', 'statistics', 'render'],
    'ui/windows/main/pages/library/LibraryPage.tsx': ['import', 'download', 'search', 'explorer', 'refresh', 'folder_new', 'all', 'unknown', 'folder', 'picture_error', 'delete'],
    'ui/windows/main/pages/properties/PropertiesPage.tsx': ['import', 'gallery', 'picture_import', 'picture_error', 'outlink', 'run', 'build', 'picture_trans', 'empty', 'undo', 'save'],
    'ui/windows/main/pages/render/RenderPage.tsx': ['run', 'build', 'hide', 'close', 'picture_trans', 'empty'],
    'ui/windows/main/pages/settings/SettingsPage.tsx': ['light', 'dark', 'system'],
  };
  for (const [path, names] of Object.entries(expected)) {
    const code = source(path);
    assert.match(code, /<ThemeIcon\b/, path);
    for (const name of names) assert.ok(code.includes(`"${name}"`), `${path}: ThemeIcon ${name}`);
  }

  const flake = source('ui/windows/main/pages/flake/FlakePage.tsx');
  assert.match(flake, /<ThemeIcon set="tool"/);
  for (const name of ['hand', 'stick', 'ruler', 'pin', 'select', 'pencil', 'eraser', 'pick', 'rectangle', 'circle', 'fill', 'text']) {
    assert.ok(flake.includes(`themeIcon: "${name}"`), `FlakePage: tool ${name}`);
  }
});

test('导航继续使用共享矢量回退，主题图标不引入反色滤镜', () => {
  assert.match(source('ui/shell/NavIcon.tsx'), /className=\{\["nav-icon"/);
  assert.match(source('ui/shell/nav-icons.css'), /color:\s*currentColor/);
  assert.doesNotMatch(source('ui/themes/modern.css'), /invert\(/);
  const css = source('ui/styles/controls.css');
  assert.match(css, /html\[data-theme\] svg\.lucide\s*\{[^}]*--nova-icon-size, 18px/);
  assert.match(css, /stroke-width:\s*1\.75/);
});

test('Minecraft 材料图标继续读取真实游戏资源而不是主题图标', () => {
  const code = source('ui/components/BlockIcon.tsx');
  assert.match(code, /getBlockIconDataUrl\(blockId, slot, lookupMode\)/);
  assert.match(code, /<img className="block-icon-image" src=\{src\}/);
  assert.doesNotMatch(code, /lucide-react|<svg|ThemeIcon/);
  assert.match(source('ui/windows/main/pages/statistics/StatisticsPage.tsx'), /<BlockIcon blockId=\{material.iconHint\}/);
});
