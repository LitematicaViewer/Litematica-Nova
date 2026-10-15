import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import zlib from 'node:zlib';
import test from 'node:test';
import { platformFor, tauriOverride, iconPng } from './build-public-platform.mjs';

const base = JSON.parse(fs.readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const make = (platform) => tauriOverride(base, { platform, stage: '/build/helpers', runtime: '/build/runtime', stockpile: '/build/stockpile_server', icon: '/build/icons/icon.icns', candidate: false });
test('正式平台入口绑定真实 native host，不拿交叉构建冒充可运行目标', () => {
  assert.equal(platformFor('x86_64-pc-windows-msvc', { platform: 'win32', arch: 'x64' }).bundles, 'nsis');
  assert.equal(platformFor('aarch64-apple-darwin', { platform: 'darwin', arch: 'arm64' }).module, 'libnova_protected_module.dylib');
  assert.throws(() => platformFor('aarch64-apple-darwin', { platform: 'win32', arch: 'x64' }), /native runner/);
});
test('Linux/mac 资源覆盖显式移除旧 EXE/COM/DLL 包映射并保留普通资源', () => {
  for (const target of ['x86_64-unknown-linux-gnu', 'x86_64-apple-darwin']) {
    const platform = platformFor(target, { platform: target.includes('linux') ? 'linux' : 'darwin', arch: 'x64' });
    const config = make(platform);
    assert.equal(config.bundle.windows, null);
    for (const [key, destination] of Object.entries(base.bundle.resources)) {
      assert.equal(config.bundle.resources[key], null);
      if (/stockpile_server\.exe|litematica_thumbnail\.dll/.test(key)) assert.equal(Object.values(config.bundle.resources).includes(destination), false);
    }
    assert.equal(config.bundle.resources['/build/runtime'], 'bin/viewer-backend/protected');
    assert.equal(config.bundle.resources['/build/helpers'], 'bin/viewer-backend');
    assert.ok(Object.values(config.bundle.resources).includes('pack-in/materials/minecraft-26.3.json'));
    assert.equal(config.bundle.createUpdaterArtifacts, undefined);
    if (target.includes('apple')) assert.deepEqual(config.bundle.icon, [path.join('/build/icons','icon.png'), '/build/icons/icon.icns']);
  }
});
test('Windows 保留 NSIS/COM，只有显式 candidate 才关闭 updater 产物', () => {
  const platform = platformFor('x86_64-pc-windows-msvc', { platform: 'win32', arch: 'x64' });
  const config = make(platform);
  assert.deepEqual(config.bundle.targets, ['nsis']);
  assert.ok(Object.values(config.bundle.resources).includes('litematica_thumbnail.dll'));
  assert.equal(config.bundle.windows, undefined);
  assert.equal(tauriOverride(base, { platform, stage: '/a', runtime: '/b', stockpile: '/c', candidate: true }).bundle.createUpdaterArtifacts, false);
});
test('复用现有产品 ICO 的像素生成 RGBA PNG，不替换品牌图像', (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'nova-ci-icon-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const png = fs.readFileSync(iconPng(directory));
  assert.equal(png.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
  assert.equal(png.readUInt32BE(16), 64);
  assert.equal(png.readUInt32BE(20), 64);
  assert.equal(png[25], 6);
  let offset = 8, rgba;
  while (offset < png.length) {
    const length = png.readUInt32BE(offset), type = png.subarray(offset + 4, offset + 8).toString();
    if (type === 'IDAT') rgba = zlib.inflateSync(png.subarray(offset + 8, offset + 8 + length));
    offset += 12 + length;
  }
  assert.equal(rgba.length, 64 * (1 + 64 * 4));
  assert.ok(rgba.some((byte) => byte !== 0));
});
