import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { thumbnailTarget, thumbnailSigningArguments, moltenVkLibrary } from './build-macos-thumbnail.mjs';

const script = fileURLToPath(new URL('./build-macos-thumbnail.mjs', import.meta.url));
const desktop = path.resolve(path.dirname(script), '..');

test('macOS extension target matches both actual supported architectures', () => {
  assert.deepEqual(thumbnailTarget({}, 'arm64'), { arch: 'arm64', minimum: '13.0' });
  assert.deepEqual(thumbnailTarget({}, 'x64'), { arch: 'x86_64', minimum: '13.0' });
  assert.equal(thumbnailTarget({ TAURI_ENV_ARCH: 'aarch64' }, 'x64').arch, 'arm64');
  assert.equal(thumbnailTarget({ TAURI_ENV_ARCH: 'x86_64' }, 'arm64').arch, 'x86_64');
});

test('explicit target wins over host architecture and incompatible targets fail', () => {
  assert.equal(thumbnailTarget({ TAURI_ENV_TARGET_TRIPLE: 'x86_64-apple-darwin' }, 'arm64').arch, 'x86_64');
  assert.equal(thumbnailTarget({ CARGO_BUILD_TARGET: 'aarch64-apple-darwin' }, 'x64').arch, 'arm64');
  for (const target of ['universal-apple-darwin', 'x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc']) {
    assert.throws(() => thumbnailTarget({ TAURI_ENV_TARGET_TRIPLE: target }), /no matching macOS target/);
  }
});

test('ad-hoc signing is explicit and never pretends to timestamp or notarize', () => {
  assert.deepEqual(thumbnailSigningArguments('', 'test.appex', 'test.plist'),
    ['--force', '--sign', '-', '--entitlements', 'test.plist', 'test.appex']);
});

test('configured signing uses the extension sandbox entitlements and hardened runtime', () => {
  const args = thumbnailSigningArguments('Developer ID Application: fixture', 'test.appex', 'test.plist');
  assert.ok(args.includes('--timestamp'));
  assert.equal(args[args.indexOf('--options') + 1], 'runtime');
  assert.equal(args[args.indexOf('--entitlements') + 1], 'test.plist');
  assert.ok(!args.includes('--deep'));
});

test('Tauri builds and embeds the actual extension and registers the same UTI', () => {
  const config = JSON.parse(fs.readFileSync(path.join(desktop, 'src-tauri/tauri.conf.json'), 'utf8'));
  assert.match(config.build.beforeBuildCommand, /node scripts\/build-macos-thumbnail\.mjs/);
  assert.equal(config.bundle.macOS.minimumSystemVersion, '13.0');
  const extension = config.bundle.macOS.files['PlugIns/NovaThumbnail.appex'];
  assert.equal(extension, '../../tools/macos-thumbnail-extension/target/NovaThumbnail.appex');
  assert.ok(config.bundle.macOS.frameworks.includes('target/platform-frameworks/libMoltenVK.dylib'));
  assert.ok(config.bundle.fileAssociations.some(item => item.ext.includes('litematic')));
  const application = fs.readFileSync(path.join(desktop, 'src-tauri', config.bundle.macOS.infoPlist), 'utf8');
  const extensionPlist = fs.readFileSync(path.resolve(desktop, '../tools/macos-thumbnail-extension/Info.plist'), 'utf8');
  assert.match(application, /com\.litematica\.nova\.litematic/);
  assert.match(extensionPlist, /com\.litematica\.nova\.litematic/);
  assert.match(extensionPlist, /com\.apple\.quicklook\.thumbnail/);
});

test('explicit real MoltenVK path wins and a missing explicit path never falls back silently', () => {
  const explicit = path.resolve('fixture/MoltenVK/libMoltenVK.dylib');
  assert.equal(moltenVkLibrary({ NOVA_MOLTENVK_LIBRARY: explicit, VULKAN_SDK: 'other-sdk' }, value => value === explicit), explicit);
  assert.throws(() => moltenVkLibrary({ NOVA_MOLTENVK_LIBRARY: explicit }, () => false), /Configured NOVA_MOLTENVK_LIBRARY is missing/);
});

test('existing Vulkan SDK layout is recognized without downloading or manufacturing a library', () => {
  const root = path.resolve('fixture/SDK/macOS');
  const library = path.join(root, 'lib/libMoltenVK.dylib');
  assert.equal(moltenVkLibrary({ VULKAN_SDK: root }, value => value === library), library);
  assert.throws(() => moltenVkLibrary({}, () => false), /real macOS libMoltenVK\.dylib is required/);
});

test('non-macOS build hook is a no-op without invoking a fake SDK', { skip: process.platform === 'darwin' }, () => {
  const result = spawnSync(process.execPath, [script], { encoding: 'utf8', env: { ...process.env, TAURI_ENV_TARGET_TRIPLE: 'intentionally-not-a-macos-target' } });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, '');
});
