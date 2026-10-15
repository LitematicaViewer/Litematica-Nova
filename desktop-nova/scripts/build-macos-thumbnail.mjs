import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

export function thumbnailTarget(env = process.env, architecture = process.arch) {
  const target = env.TAURI_ENV_TARGET_TRIPLE || env.CARGO_BUILD_TARGET || env.TAURI_ENV_ARCH || architecture;
  const arch = /^(?:aarch64|arm64)(?:-apple-darwin)?$/.test(target) ? 'arm64'
    : /^(?:x86_64|x64)(?:-apple-darwin)?$/.test(target) ? 'x86_64' : null;
  if (!arch) throw new Error(`QuickLook extension has no matching macOS target: ${target}`);
  return { arch, minimum: '13.0' };
}

export function thumbnailSigningArguments(identity, bundle, entitlements) {
  const signer = identity?.trim() || '-';
  return ['--force', '--sign', signer,
    ...(signer === '-' ? [] : ['--timestamp', '--options', 'runtime']),
    '--entitlements', entitlements, bundle];
}

export function moltenVkLibrary(env = process.env, isFile = candidate => {
  try { return fs.statSync(candidate).isFile(); } catch { return false; }
}) {
  const explicit = env.NOVA_MOLTENVK_LIBRARY?.trim();
  if (explicit) {
    const library = path.resolve(explicit);
    if (!isFile(library)) throw new Error(`Configured NOVA_MOLTENVK_LIBRARY is missing: ${library}`);
    return library;
  }
  const roots = [env.VULKAN_SDK, env.MOLTENVK_ROOT, env.MOLTENVK_SDK].filter(Boolean);
  const candidates = roots.flatMap(root => [
    path.resolve(root, 'lib/libMoltenVK.dylib'),
    path.resolve(root, 'dylib/macOS/libMoltenVK.dylib'),
    path.resolve(root, 'MoltenVK/dylib/macOS/libMoltenVK.dylib'),
    path.resolve(root, '../MoltenVK/dylib/macOS/libMoltenVK.dylib'),
  ]);
  candidates.push('/opt/homebrew/lib/libMoltenVK.dylib', '/usr/local/lib/libMoltenVK.dylib');
  const library = candidates.find(isFile);
  if (!library) throw new Error('A real macOS libMoltenVK.dylib is required. Set NOVA_MOLTENVK_LIBRARY or VULKAN_SDK; this build never creates a placeholder or relies on the end user installing Vulkan.');
  return library;
}

export function buildMacosThumbnail() {
// Tauri beforeBuildCommand runs on every platform. Only a real macOS SDK may
// produce this extension; no stub executable is put in another platform's app.
if (process.platform !== 'darwin') return;

const desktop = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const root = path.resolve(desktop, '..');
const source = path.join(root, 'tools/macos-thumbnail-extension');
const bundle = path.join(source, 'target/NovaThumbnail.appex');
const contents = path.join(bundle, 'Contents');
const executable = path.join(contents, 'MacOS/NovaThumbnail');
const { arch, minimum } = thumbnailTarget();
const moltenVk = moltenVkLibrary();
const framework = path.join(desktop, 'src-tauri/target/platform-frameworks/libMoltenVK.dylib');
const config = JSON.parse(fs.readFileSync(path.join(desktop, 'src-tauri/tauri.conf.json'), 'utf8'));
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(config.version)) throw new Error('Invalid application version for the QuickLook bundle.');
// CFBundleVersion cannot contain a prerelease suffix.
const version = config.version.split(/[-+]/, 1)[0];

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`macOS thumbnail build failed (${command}, exit ${result.status}).`);
}

// Check the actual Mach-O slices before copying. Tauri embeds this framework
// in Contents/Frameworks and signs it as nested code using the app identity.
run('xcrun', ['lipo', moltenVk, '-verify_arch', arch]);
fs.mkdirSync(path.dirname(framework), { recursive: true });
fs.copyFileSync(fs.realpathSync(moltenVk), framework);

fs.mkdirSync(path.dirname(executable), { recursive: true });
fs.writeFileSync(path.join(contents, 'Info.plist'),
  fs.readFileSync(path.join(source, 'Info.plist'), 'utf8')
    .replaceAll('__NOVA_VERSION__', version).replaceAll('__NOVA_MINIMUM_MACOS__', minimum));
run('xcrun', ['--sdk', 'macosx', 'swiftc', '-parse-as-library', '-O',
  '-application-extension', '-module-name', 'NovaThumbnail',
  '-target', `${arch}-apple-macosx${minimum}`,
  '-framework', 'Foundation', '-framework', 'AppKit', '-framework', 'QuickLookThumbnailing',
  '-Xlinker', '-e', '-Xlinker', '_NSExtensionMain',
  path.join(source, 'ThumbnailProvider.swift'), '-o', executable]);
run('xcrun', ['strip', '-S', '-x', executable]);

const identity = process.env.APPLE_SIGNING_IDENTITY?.trim() || '-';
run('codesign', thumbnailSigningArguments(identity, bundle, path.join(source, 'entitlements.plist')));
console.log(`[nova-macos-thumbnail] architecture=${arch} signing=${identity === '-' ? 'ad-hoc' : 'configured'} notarization=not_performed runtime=not_verified`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) buildMacosThumbnail();
