import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import test from 'node:test';
import { platformFor, tauriOverride, iconPng, tauriBuildStages, cargoDirectories, validateDistributableDirectory, restoreAppImageHelpers } from './build-public-platform.mjs';

const base = JSON.parse(fs.readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const make = (platform) => tauriOverride(base, { platform, stage: '/build/helpers', runtime: '/build/runtime', stockpile: '/build/stockpile_server', icon: '/build/icons/icon.icns', candidate: false });
test('正式平台入口绑定真实 native host，不拿交叉构建冒充可运行目标', () => {
  assert.equal(platformFor('x86_64-pc-windows-msvc', { platform: 'win32', arch: 'x64' }).bundles, 'nsis');
  assert.equal(platformFor('aarch64-apple-darwin', { platform: 'darwin', arch: 'arm64' }).module, 'libnova_protected_module.dylib');
  assert.throws(() => platformFor('aarch64-apple-darwin', { platform: 'win32', arch: 'x64' }), /native runner/);
});

test('macOS和Linux编译与打包分离，Windows保持既有NSIS流程', () => {
  assert.deepEqual(tauriBuildStages({ os: 'darwin', bundles: 'app,dmg' }, '/build/config.json'), [
    ['build', '--config', '/build/config.json', '--no-bundle'],
    ['bundle', '--config', '/build/config.json', '--bundles', 'app,dmg'],
  ]);
  assert.deepEqual(tauriBuildStages({ os: 'win32', bundles: 'nsis' }, '/build/config.json'), [['build', '--config', '/build/config.json', '--bundles', 'nsis']]);
  assert.deepEqual(tauriBuildStages({ os: 'linux', bundles: 'deb,appimage' }, '/build/config.json'), [
    ['build', '--config', '/build/config.json', '--no-bundle'],
    ['bundle', '--config', '/build/config.json', '--bundles', 'deb,appimage'],
  ]);
});
test('AppImage恢复正式helper原始字节，不篡改部署身份以接受RPATH变换', async (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'nova-appimage-identity-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const stage = path.join(directory, 'stage'), app = path.join(directory, 'Nova.AppDir');
  const destination = path.join(app, 'usr/lib/Litematica Nova/bin/viewer-backend');
  fs.mkdirSync(stage, { recursive: true }); fs.mkdirSync(destination, { recursive: true });
  const artifacts = {};
  for (const name of ['litematica_core', 'litematica_native_viewer', 'nova-render-service', 'nova-build-worker']) {
    const original = Buffer.from(`original-${name}`);
    fs.writeFileSync(path.join(stage, name), original);
    fs.writeFileSync(path.join(destination, name), 'linuxdeploy-rpath-transform');
    artifacts[name] = { bytes: original.length, sha256: crypto.createHash('sha256').update(original).digest('hex') };
  }
  const manifest = JSON.stringify({ artifacts });
  fs.writeFileSync(path.join(stage, 'backend-deployment.json'), manifest);
  await restoreAppImageHelpers(app, stage, 'Litematica Nova');
  for (const name of Object.keys(artifacts)) assert.deepEqual(fs.readFileSync(path.join(destination, name)), fs.readFileSync(path.join(stage, name)));
  assert.equal(fs.readFileSync(path.join(stage, 'backend-deployment.json'), 'utf8'), manifest);
  fs.writeFileSync(path.join(stage, 'litematica_core'), 'changed');
  await assert.rejects(restoreAppImageHelpers(app, stage, 'Litematica Nova'), /identity changed/);
});
test('仅 macOS 公开三 crate 共用 Cargo target；Windows/Linux 既有输出位置不变', () => {
  const selected = path.resolve('/build/public-cargo');
  const mac = cargoDirectories('aarch64-apple-darwin', { os: 'darwin' }, selected);
  assert.deepEqual(mac, { host: selected, core: selected, loader: selected });
  for (const [target, osName, directory] of [['x86_64-pc-windows-msvc', 'win32', 'public-beta'], ['x86_64-unknown-linux-gnu', 'linux', 'public-beta-x86_64-unknown-linux-gnu']]) {
    const paths = cargoDirectories(target, { os: osName }, selected);
    assert.equal(paths.host, selected);
    assert.ok(paths.core.endsWith(path.join('tools/viewer-core/target', directory)));
    assert.ok(paths.loader.endsWith(path.join('tools/nova-protected-loader/target', directory)));
    assert.notEqual(paths.core, paths.host);
  }
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

test('Linux 包声明动态 Vulkan 与设备存储所需库，不污染 Windows/macOS', () => {
  assert.deepEqual(make(platformFor('x86_64-unknown-linux-gnu', { platform: 'linux', arch: 'x64' })).bundle.linux.deb.depends, ['libvulkan1', 'libdbus-1-3', 'libc6 (>= 2.39)']);
  for (const platform of [{ os: 'win32', bundles: 'nsis' }, { os: 'darwin', bundles: 'app,dmg' }]) assert.equal(make(platform).bundle.linux, undefined);
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

test('固定源码的可信 CI 可以复用明确绑定的 Cargo 目标，根目录及未绑定路径仍被拒绝', () => {
  const sourceRoot = path.resolve('/ci/source'), cacheRoot = path.resolve('/ci/cache'), runnerTemp = path.resolve('/ci/runner');
  const options = { sourceRoot, cacheRoot, runnerTemp, approvedSha: 'a'.repeat(40) };
  const shared = path.join(cacheRoot, 'desktop-nova/src-tauri/target/ci-windows-x64');
  assert.equal(validateDistributableDirectory(shared, options), shared);
  assert.equal(validateDistributableDirectory(path.join(runnerTemp, 'nova-public-target'), options), path.join(runnerTemp, 'nova-public-target'));
  for (const outside of [cacheRoot, runnerTemp, path.join(runnerTemp, 'other-target')]) assert.throws(() => validateDistributableDirectory(outside, options), /isolated/);
  assert.throws(() => validateDistributableDirectory(shared, { ...options, approvedSha: '' }), /isolated/);
});
