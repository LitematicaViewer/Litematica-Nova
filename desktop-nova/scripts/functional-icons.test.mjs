import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import * as icons from 'lucide-react';

const source = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const functionalPages = [
  'ui/shell/AppShell.tsx',
  'ui/windows/local_library_folders/LocalLibraryFoldersContent.tsx',
  'ui/windows/main/pages/flake/FlakePage.tsx',
  'ui/windows/main/pages/library/LibraryPage.tsx',
  'ui/windows/main/pages/properties/PropertiesPage.tsx',
  'ui/windows/main/pages/render/RenderPage.tsx',
];

test('导航、投影库、属性与分层功能图标来自同一可用 SVG 组件集', () => {
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

test('主题切换不再换用功能位图或反色滤镜，工具不以字形冒充图标', () => {
  for (const path of functionalPages.slice(0, 3)) {
    const code = source(path);
    assert.doesNotMatch(code, /import\.meta\.glob|themeResourceKey|glyph:/, path);
  }
  assert.match(source('ui/shell/AppShell.tsx'), /<Icon className="nav-icon" aria-hidden="true"/);
  assert.doesNotMatch(source('ui/themes/modern.css'), /invert\(/);
  assert.match(source('ui/shell/nav-icons.css'), /color:\s*currentColor/);
  const css = source('ui/styles/controls.css');
  assert.match(css, /html\[data-theme\] svg\.lucide\s*\{[^}]*--nova-icon-size, 18px/);
  assert.match(css, /stroke-width:\s*1\.75/);
});

test('Minecraft 材料图标继续读取真实游戏资源而不是通用 SVG', () => {
  const code = source('ui/components/BlockIcon.tsx');
  assert.match(code, /getBlockIconDataUrl\(blockId\)/);
  assert.match(code, /<img className="block-icon-image" src=\{src\}/);
  assert.doesNotMatch(code, /lucide-react|<svg/);
  assert.match(source('ui/windows/main/pages/statistics/StatisticsPage.tsx'), /<BlockIcon blockId=\{material.iconHint\}/);
});
