import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import zlib from 'node:zlib';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { validateRef, validateSource, validatePackage, validateDevelopmentPackage, helperInputIdentity, validateStaging, nativeTarget, runtimeSource, communityIdentity } from './check-public-checkout.mjs';
import { ensureDevelopmentRuntime, acquireLocalDevelopmentRuntime } from './fetch-ci-runtime.mjs';
import { prepareDefaultGameData } from './prepare-default-game-data.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const desktop = path.join(root, 'desktop-nova'), tauri = path.join(desktop, 'src-tauri');
const platforms = {
  'x86_64-pc-windows-msvc': { os: 'win32', arch: 'x64', module: 'nova_protected_module.dll', suffix: '.exe', stockpile: 'windows-x64', bundles: 'nsis' },
  'x86_64-unknown-linux-gnu': { os: 'linux', arch: 'x64', module: 'libnova_protected_module.so', suffix: '', stockpile: 'linux-x64', bundles: 'deb,appimage' },
  'x86_64-apple-darwin': { os: 'darwin', arch: 'x64', module: 'libnova_protected_module.dylib', suffix: '', stockpile: 'macos-x64', bundles: 'app,dmg' },
  'aarch64-apple-darwin': { os: 'darwin', arch: 'arm64', module: 'libnova_protected_module.dylib', suffix: '', stockpile: 'macos-arm64', bundles: 'app,dmg' },
};
export function platformFor(target, host = { platform: process.platform, arch: process.arch }) {
  const platform = platforms[target];
  if (!platform) throw new Error('Unsupported desktop target.');
  if (platform.os !== host.platform || platform.arch !== host.arch) throw new Error('Use a real native runner for the selected desktop target.');
  return platform;
}
export function tauriBuildStages(platform, config) {
  const build = ['build', '--config', config];
  return platform.os === 'darwin'
    ? [[...build, '--no-bundle'], ['bundle', '--config', config, '--bundles', platform.bundles]]
    : [[...build, '--bundles', platform.bundles]];
}
export function cargoDirectories(target, platform, targetDirectory) {
  const host = path.resolve(targetDirectory || path.join(tauri, 'target', `ci-${target}`));
  const directory = platform.os === 'win32' ? 'public-beta' : `public-beta-${target}`;
  const helperRoot = path.resolve(process.env.NOVA_PUBLIC_CARGO_CACHE_ROOT || root);
  return {
    host,
    core: platform.os === 'darwin' ? host : path.join(helperRoot, 'tools/viewer-core/target', directory),
    loader: platform.os === 'darwin' ? host : path.join(helperRoot, 'tools/nova-protected-loader/target', directory),
  };
}
function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, env: process.env, stdio: 'inherit', windowsHide: true, ...options });
  if (result.error || result.status !== 0) throw new Error(`${path.basename(command)} failed (${result.status ?? result.error?.code}).`);
  return result;
}
function output(command, args) {
  return run(command, args, { stdio: ['ignore', 'pipe', 'pipe'], encoding: 'utf8' }).stdout.trim();
}
async function hash(filename) {
  const digest = crypto.createHash('sha256');
  for await (const bytes of fs.createReadStream(filename)) digest.update(bytes);
  return digest.digest('hex');
}
function npmCli() {
  const bin = path.dirname(process.execPath);
  const choices = [process.env.npm_execpath, path.join(bin, 'node_modules/npm/bin/npm-cli.js'), path.join(bin, '../lib/node_modules/npm/bin/npm-cli.js')];
  const result = choices.find((file) => file && fs.existsSync(file));
  if (!result) throw new Error('The active Node toolchain does not contain npm-cli.js.');
  return result;
}
export function tauriOverride(base, { platform, stage, runtime, stockpile, icon, candidate, fullRuntime = Boolean(runtime) }) {
  const resources = Object.fromEntries(Object.keys(base.bundle.resources).map((key) => [key, null]));
  for (const [key, destination] of Object.entries(base.bundle.resources)) {
    if (key.includes('/bin/public-beta') || key.includes('/bin/protected') || destination.startsWith('bin/viewer-backend') || /stockpile_server\.exe|litematica_thumbnail\.dll/.test(key)) continue;
    resources[path.resolve(tauri, key)] = destination;
  }
  resources[stage] = 'bin/viewer-backend';
  if (runtime) resources[runtime] = 'bin/viewer-backend/protected';
  if (fullRuntime) {
    for (const name of ['AMICE-APACHE-2.0.txt', 'PROTECTED-MODULE.txt']) resources[path.join(root, 'licenses', name)] = `licenses/${name}`;
  }
  const defaultData = path.join(root, 'pack-in/game-data/default');
  if (fs.existsSync(path.join(defaultData, 'manifest.json'))) resources[defaultData] = 'pack-in/game-data/default';
  resources[stockpile] = `bin/stockpile-server/${platform.stockpile}/stockpile_server${platform.suffix}`;
  if (platform.os === 'win32') resources[path.join(root, 'tools/windows-thumbnail-provider/target/release/litematica_thumbnail.dll')] = 'litematica_thumbnail.dll';
  return { bundle: { targets: platform.bundles.split(','), resources, ...(platform.os !== 'win32' ? { icon: platform.os === 'darwin' ? [path.join(path.dirname(icon), 'icon.png'), icon] : [icon], windows: null } : {}), ...(platform.os === 'darwin' && fullRuntime ? { macOS: { frameworks: [path.join(tauri, 'target/platform-frameworks/libMoltenVK.dylib')] } } : {}), ...(platform.os === 'linux' ? { linux: { deb: { depends: [...new Set([...(base.bundle.linux?.deb?.depends ?? []), 'libvulkan1', 'libdbus-1-3', 'libc6 (>= 2.39)'])] } } } : {}), createUpdaterArtifacts: Boolean(runtime && !candidate) } };
}
export async function stageRuntimeResources(runtime, target, platform) {
  if (platform.os !== 'win32' || path.parse(runtime).root.toLowerCase() === path.parse(tauri).root.toLowerCase()) return runtime;
  const identity = await validatePackage(runtime, platform.module, { target });
  const directory = path.join(root, '.tmp/ci-runtime', target);
  const names = ['protected-build.json', 'runtime-compatibility.json', identity.manifestFile, identity.ciphertextFile];
  fs.mkdirSync(directory, { recursive: true });
  if (fs.readdirSync(directory).some(name => !names.includes(name))) throw new Error('Unexpected file in runtime resource staging.');
  for (const name of names) fs.copyFileSync(path.join(runtime, name), path.join(directory, name));
  await validatePackage(directory, platform.module, { target });
  return directory;
}
export async function stageStockpileResource(source, target, platform, resourceRoot = root) {
  source = path.resolve(source);
  resourceRoot = path.resolve(resourceRoot);
  if (!fs.statSync(source).isFile()) throw new Error('The actual compiled stockpile resource is missing.');
  if (platform.os !== 'win32' || path.parse(source).root.toLowerCase() === path.parse(resourceRoot).root.toLowerCase()) return source;
  const directory = path.join(resourceRoot, '.tmp/ci-resources', target);
  fs.mkdirSync(directory, { recursive: true });
  const staged = path.join(directory, 'stockpile_server.exe');
  const sourceHash = await hash(source);
  if (!fs.existsSync(staged) || fs.statSync(staged).size !== fs.statSync(source).size || await hash(staged) !== sourceHash) fs.copyFileSync(source, staged);
  if (await hash(staged) !== sourceHash) throw new Error('The cross-drive stockpile resource staging differs from its real Cargo output.');
  return staged;
}
async function prepareAssets(optional = false) {
  if (fs.existsSync(path.join(root, 'pack-in/game-data/default/manifest.json'))) return;
  if (optional && !fs.existsSync(path.join(root, 'pack-in/minecraft-runtime-26.3.zip'))) return;
  const archive = path.join(root, 'pack-in/minecraft-runtime-26.3.zip');
  const manifest = JSON.parse(fs.readFileSync(path.join(root, 'pack-in/minecraft-runtime-26.3.manifest.json'), 'utf8'));
  if (manifest.schema !== 'nova.minecraft-runtime-resources.v1' || manifest.java_class_entries !== 0 || manifest.executable_entries !== 0 || await hash(archive) !== manifest.archive_sha256) throw new Error('Bundled runtime resource identity differs.');
  const python = String.raw`
import hashlib,os,re,stat,sys,zipfile
archive,root,count=sys.argv[1:]
with zipfile.ZipFile(archive) as z:
    entries=z.infolist()
    if len(entries)!=int(count) or sum(e.file_size for e in entries)>512*1024**2: raise ValueError('Resource archive size/count mismatch')
    for e in entries:
        if not re.fullmatch(r'26\.3/(oracle/[a-z_]+\.jsonl|oracle/oracle-manifest\.json|client\.jar|assets/indexes/34\.json|assets/objects/[a-f0-9]{2}/[a-f0-9]{40})',e.filename) or e.file_size>100*1024**2 or stat.S_ISLNK(e.external_attr>>16): raise ValueError('Unexpected resource entry')
        target=os.path.join(root,*e.filename.split('/'));os.makedirs(os.path.dirname(target),exist_ok=True)
        if os.path.isfile(target) and os.path.getsize(target)==e.file_size:
            with open(target,'rb') as cached,z.open(e) as packed:
                if hashlib.file_digest(cached,'sha256').digest()==hashlib.file_digest(packed,'sha256').digest(): continue
        with z.open(e) as src,open(target,'wb') as dst:
            while True:
                b=src.read(1024*1024)
                if not b: break
                dst.write(b)
with zipfile.ZipFile(os.path.join(root,'26.3/client.jar')) as z:
    for e in z.infolist():
        if re.search(r'\.(class|java|dll|exe|rs|wgsl|ll|pdb)$',e.filename,re.I) or (e.filename.upper().startswith('META-INF/') and e.filename!='META-INF/LICENSE'): raise ValueError('Unexpected executable resource entry')
`;
  run('python3', ['-c', python, archive, path.join(root, 'data/mc26'), String(manifest.archive_entries)]);
  const directory = path.join(root, 'data/blockdb');
  const blocks = fs.readFileSync(path.join(directory, 'blocks.dat.z'));
  const tint = JSON.parse(zlib.inflateSync(fs.readFileSync(path.join(directory, 'plains-tints.dat.z')), { maxOutputLength: 16 * 1024 ** 2 }));
  if (tint.schema !== 1 || tint.purpose !== 'block-database-side-plains-origin' || tint.blocksDatSha256 !== crypto.createHash('sha256').update(blocks).digest('hex') || !Object.keys(tint.versions ?? {}).length || Object.values(tint.versions).some((key) => !Object.hasOwn(tint.tables ?? {}, key))) throw new Error('Block database tint metadata differs.');
}
export function iconPng(outputDirectory) {
  // Share the canonical transparent artwork with the Host, Viewer and bundles.
  // Official Tauri ICOs may contain PNG frames; do not assume legacy BMP data.
  const filename = path.join(outputDirectory, 'product-icon.png');
  fs.copyFileSync(path.join(tauri, 'icons/icon.png'), filename);
  return filename;
}

async function stageBackends(target, platform, identity, sha, directory, cargo, helperInputId, helperSourceSha) {
  const names = (identity.runtimeKind === 'community' ? ['litematica_core', 'litematica_native_viewer'] : ['litematica_core', 'litematica_native_viewer', 'nova-render-service', 'nova-build-worker']).map((name) => name + platform.suffix);
  fs.mkdirSync(directory, { recursive: true });
  if (fs.readdirSync(directory).some((name) => ![...names, 'backend-deployment.json'].includes(name))) throw new Error('Unexpected file in the platform backend staging directory.');
  const artifacts = {};
  for (const name of names) {
    const source = platform.os === 'win32' ? path.join(root, 'bin/public-beta', name) : path.join(name.startsWith('nova-') ? cargo.loader : cargo.core, 'release', name);
    const destination = path.join(directory, name);
    if (!fs.existsSync(destination) || fs.statSync(destination).size !== fs.statSync(source).size || await hash(destination) !== await hash(source)) fs.copyFileSync(source, destination);
    if (platform.os !== 'win32') fs.chmodSync(destination, 0o755);
    artifacts[name] = { bytes: fs.statSync(destination).size, sha256: await hash(destination) };
  }
  const deployment = JSON.stringify({ schema: 'lba.backend.deployment.v1', gitSha: sha, helperSourceSha, helperInputId, profile: 'release', runtimeKind: identity.runtimeKind || (identity.developmentDescriptor ? 'development' : 'production'), runtimeBuildId: identity.buildId, targetTriple: target, artifacts }, null, 2);
  const manifest = path.join(directory, 'backend-deployment.json');
  if (!fs.existsSync(manifest) || fs.readFileSync(manifest, 'utf8') !== deployment) fs.writeFileSync(manifest, deployment);
}
async function collect(target, platform, sha, cargoTarget, version) {
  const source = path.join(cargoTarget, 'release/bundle');
  const directories = platform.os === 'win32' ? ['nsis'] : platform.os === 'linux' ? ['deb', 'appimage'] : ['dmg', 'macos'];
  const allowed = platform.os === 'win32' ? /setup\.exe(?:\.sig)?$/i : platform.os === 'linux' ? /\.(deb|AppImage)(?:\.sig)?$/ : /(?:\.dmg|\.app\.tar\.gz)(?:\.sig)?$/;
  const destination = path.join(root, 'dist/ci', target); fs.mkdirSync(destination, { recursive: true });
  const files = [];
  for (const directory of directories) {
    const from = path.join(source, directory); if (!fs.existsSync(from)) continue;
    for (const name of fs.readdirSync(from)) {
      const filename = path.join(from, name); if (!allowed.test(name) || !fs.statSync(filename).isFile()) continue;
      const finalName = name.replace(/\.app\.tar\.gz(?=\.sig$|$)/, `_${version}_${target}.app.tar.gz`);
      const output = path.join(destination, finalName); fs.copyFileSync(filename, output);
      files.push({ path: path.relative(root, output).split(path.sep).join('/'), bytes: fs.statSync(output).size, sha256: await hash(output) });
    }
  }
  if (!files.some((file) => !file.path.endsWith('.sig'))) throw new Error('Tauri did not produce a real platform distributable.');
  fs.writeFileSync(path.join(destination, 'artifacts.json'), JSON.stringify({ schema: 'nova.ci.artifacts.v1', target, sourceSha: sha, files }, null, 2));
  console.log(`Desktop candidate complete: target=${target} source=${sha} distributables=${files.length}`);
}
function removePreviousDistributables(cargoTarget, platform) {
  const relative = path.relative(path.join(tauri, 'target'), cargoTarget);
  if (!relative || relative.startsWith('..') || path.isAbsolute(relative)) throw new Error('The desktop target directory must be an isolated directory under src-tauri/target.');
  const directories = platform.os === 'win32' ? ['nsis'] : platform.os === 'linux' ? ['deb', 'appimage'] : ['dmg', 'macos'];
  for (const directory of directories) {
    const from = path.join(cargoTarget, 'release/bundle', directory);
    if (!fs.existsSync(from)) continue;
    for (const name of fs.readdirSync(from)) {
      const filename = path.join(from, name);
      if (/(?:setup\.exe|\.deb|\.AppImage|\.dmg|\.app\.tar\.gz)(?:\.sig)?$/i.test(name) && fs.lstatSync(filename).isFile()) fs.unlinkSync(filename);
    }
  }
}
function archiveMacApplication(cargoTarget) {
  const directory = path.join(cargoTarget, 'release/bundle/macos');
  const apps = fs.readdirSync(directory).filter((name) => name.endsWith('.app') && fs.lstatSync(path.join(directory, name)).isDirectory());
  if (apps.length !== 1) throw new Error('Expected one real macOS application bundle.');
  const application = path.join(directory, apps[0]);
  if (!fs.existsSync(`${application}.tar.gz`)) {
    // Match tauri-apps/tauri-action's post-build archive command, retaining symlinks.
    // Archive only after the official native Tauri build has succeeded.
    run('tar', ['czf', `${application}.tar.gz`, '-C', directory, apps[0]]);
  }
}
export async function build(args = process.argv.slice(2)) {
  const targetIndex = args.indexOf('--target');
  const target = targetIndex >= 0 ? args[targetIndex + 1] : nativeTarget(), platform = platformFor(target);
  const checkOnly = args.includes('--check'), candidate = args.includes('--candidate'), development = args.includes('--development'), compileOnly = args.includes('--compile-only');
  const take = name => { const index = args.indexOf(name); return index < 0 ? undefined : args[index + 1]; };
  const source = runtimeSource(take('--runtime-source') || runtimeSource());
  const community = source === 'community';
  const privateSourceDir = take('--private-source') || process.env.NOVA_PRIVATE_SOURCE_DIR;
  if (!community && !development && (source === 'local' || process.env.NOVA_DEVELOPMENT_RUNTIME_JSON || process.env.NOVA_DEVELOPMENT_MODULE_PATH)) throw new Error('A local Development Runtime is forbidden in formal candidate/release packaging.');
  const valueIndexes = ['--target','--runtime-source','--private-source'].map(name => args.indexOf(name)).filter(index => index >= 0).map(index => index + 1);
  const reuseExisting = args.includes('--skip-prepare');
  const devArguments = args.filter((value, index) => !['--development', '--prepare', '--check', '--candidate', '--compile-only', '--target', '--runtime-source', '--private-source', '--skip-prepare', '--'].includes(value) && !valueIndexes.includes(index));
  if (development && platform.os === 'win32' && !checkOnly && !compileOnly) {
    run('pwsh', ['-NoProfile', '-File', path.join(root, 'bin/dev-win.ps1'), '--runtime-source', source, ...(privateSourceDir ? ['--private-source', privateSourceDir] : []), ...(args.includes('--prepare') ? ['--prepare'] : []), ...(reuseExisting ? ['--skip-prepare'] : []), ...devArguments]);
    return;
  }
  const sha = output('git', ['rev-parse', '--verify', 'HEAD']);
  validateRef({ branch: output('git', ['branch', '--show-current']), sha, development, community });
  validateSource(root);
  const runtime = community ? null : source === 'local'
    ? await acquireLocalDevelopmentRuntime(root, target, { privateSourceDir, checkOnly, reuseExisting: args.includes('--skip-prepare') })
    : path.resolve(development && !checkOnly ? await ensureDevelopmentRuntime(root, target, process.env.NOVA_RUNTIME_PACKAGE_DIR) : process.env.NOVA_RUNTIME_PACKAGE_DIR || path.join(root, development ? 'bin/viewer-backend/protected' : platform.os === 'win32' ? 'bin/protected' : `bin/runtime/${target}`));
  const identity = community ? communityIdentity(target) : source === 'local' ? await validateDevelopmentPackage(runtime, platform.module, { target }) : await validatePackage(runtime, platform.module, { target, release: !development });
  const base = JSON.parse(fs.readFileSync(path.join(tauri, 'tauri.conf.json'), 'utf8'));
  const helperInputId = await helperInputIdentity(root, identity, target);
  if (output('rustc', ['-vV']).split('\n').find((line) => line.startsWith('host: ')) !== `host: ${target}`) throw new Error('The active Rust toolchain is not native to this target.');
  if (checkOnly) { console.log(`Platform build inputs verified: target=${target} source=${sha}; no build started.`); return; }
  if (!development && !community) run('git', ['diff', '--quiet', 'HEAD', '--']);
  if (args.includes('--preflight')) {
    if (platform.os !== 'darwin') throw new Error('The concentrated native preflight is macOS-only.');
    const { preflightMacosPublic } = await import('./preflight-macos-public.mjs');
    preflightMacosPublic({ target });
    return;
  }
  if (!community && !development && !candidate && !compileOnly && !process.env.TAURI_SIGNING_PRIVATE_KEY) throw new Error('The updater signing key is missing; use --candidate only for an unpublished candidate.');
  const stage = path.join(root, '.tmp/ci-backends', community ? 'community' : '', target);
  let helperSourceSha = sha;
  let previousDeployment = null;
  if (fs.existsSync(path.join(stage, 'backend-deployment.json'))) {
    try { previousDeployment = await validateStaging(stage, { identity, helperInputId, names: ['litematica_core', 'litematica_native_viewer', 'nova-render-service', 'nova-build-worker'].map(name => name + platform.suffix) }); helperSourceSha = previousDeployment.helperSourceSha || previousDeployment.gitSha; }
    catch (error) { if (reuseExisting) throw error; /* Changed inputs go through Cargo's normal incremental build. */ }
  }
  if (reuseExisting && !previousDeployment) throw new Error('No compatible existing helper stage is available; run normal development preparation first.');
  Object.assign(process.env, { NOVA_RUNTIME_SOURCE: source, NOVA_PUBLIC_HELPER_INPUT_ID: helperInputId, LBA_BUILD_GIT_SHA: helperSourceSha, CARGO_PROFILE_RELEASE_STRIP: 'symbols', CARGO_PROFILE_RELEASE_DEBUG: '0' });
  if (community) {
    process.env.NOVA_COMMUNITY_BUILD = '1';
    for (const name of ['NOVA_RUNTIME_PACKAGE_DIR', 'NOVA_PRIVATE_SOURCE_DIR', 'NOVA_DEVELOPMENT_RUNTIME_JSON', 'NOVA_DEVELOPMENT_MODULE_PATH', 'NOVA_PROTECTED_BUILD_ID', 'NOVA_PROTECTED_MANIFEST_PUBLIC_KEY', 'NOVA_PROTECTED_ENDPOINT', 'NOVA_PROTECTED_RUNTIME_COMPATIBILITY', 'NOVA_PROTECTED_MODULE_PATH', 'NOVA_PROTECTED_TICKET_PATH']) delete process.env[name];
  } else if (source === 'local') {
    delete process.env.NOVA_COMMUNITY_BUILD;
    process.env.NOVA_RUNTIME_PACKAGE_DIR = runtime;
    Object.assign(process.env, { NOVA_DEVELOPMENT_RUNTIME_JSON: JSON.stringify(identity.developmentDescriptor), NOVA_DEVELOPMENT_MODULE_PATH: identity.modulePath });
    for (const name of ['NOVA_PROTECTED_BUILD_ID','NOVA_PROTECTED_MANIFEST_PUBLIC_KEY','NOVA_PROTECTED_ENDPOINT','NOVA_PROTECTED_RUNTIME_COMPATIBILITY']) delete process.env[name];
  } else {
    delete process.env.NOVA_COMMUNITY_BUILD;
    process.env.NOVA_RUNTIME_PACKAGE_DIR = runtime;
    Object.assign(process.env, { NOVA_PROTECTED_BUILD_ID: identity.buildId, NOVA_PROTECTED_MANIFEST_PUBLIC_KEY: identity.manifestPublicKey, NOVA_PROTECTED_ENDPOINT: identity.endpoint, NOVA_PROTECTED_RUNTIME_COMPATIBILITY: JSON.stringify(identity.signedCompatibility) });
    delete process.env.NOVA_DEVELOPMENT_RUNTIME_JSON; delete process.env.NOVA_DEVELOPMENT_MODULE_PATH;
  }
  process.env.CARGO_ENCODED_RUSTFLAGS = [`--remap-path-prefix=${root}=/nova/public`, `--remap-path-prefix=${process.env.CARGO_HOME || path.join(os.homedir(), '.cargo')}=/nova/cargo`, `--remap-path-prefix=${process.env.RUSTUP_HOME || path.join(os.homedir(), '.rustup')}=/nova/rust`].join('\x1f');
  delete process.env.RUSTFLAGS;
  const cargo = cargoDirectories(target, platform, process.env.CARGO_TARGET_DIR);
  // Cargo retains the crate-specific profiles/features and only reuses matching fingerprints.
  if (platform.os === 'darwin') process.env.CARGO_TARGET_DIR = cargo.host;
  const npm = npmCli();
  if ((!development && !community) || !fs.existsSync(path.join(desktop, 'node_modules'))) run(process.execPath, [npm, 'ci'], { cwd: desktop });
  if (platform.os === 'win32') {
    run('pwsh', ['-NoProfile', '-File', path.join(desktop, 'scripts/prepare-protected-backend.ps1'), '-RepoRoot', root, '-RuntimeSource', source, ...(development ? ['-Development'] : [])]);
    helperSourceSha = JSON.parse(fs.readFileSync(path.join(root, 'bin/public-beta/backend-deployment.json'), 'utf8')).helperSourceSha;
  }
  else if (!reuseExisting) {
    await prepareAssets(development || community || compileOnly);
    run('cargo', ['build', '--locked', '--release', '--manifest-path', 'tools/viewer-core/Cargo.toml', '--target-dir', cargo.core, '--no-default-features', '--features', source === 'local' ? 'public-beta,development-runtime' : 'public-beta', '--bin', 'litematica_core', '--bin', 'litematica_native_viewer', '--bin', 'stockpile_server']);
    prepareDefaultGameData(path.join(cargo.core, 'release', `litematica_core${platform.suffix}`), { optional: development || community || compileOnly });
    if (!community) run('cargo', ['build', '--locked', '--release', '--manifest-path', 'tools/nova-protected-loader/Cargo.toml', '--target-dir', cargo.loader, '--bins', ...(source === 'local' ? ['--features','development-runtime'] : [])]);
  }
  if (!reuseExisting) await stageBackends(target, platform, identity, sha, stage, cargo, helperInputId, helperSourceSha);
  let resourceRuntime = community || source === 'local' ? null : await stageRuntimeResources(runtime, target, platform);
  if (development) {
    // Match Host get_root(): packaged debug resources are preferred, with the
    // workspace backend as its normal fallback. Neither path uses tracked
    // bin/protected or a differently selected component.
    const backend = path.join(root, 'bin/viewer-backend');
    const protectedDirectory = path.join(backend, 'protected');
    fs.mkdirSync(backend, { recursive: true });
    if (source === 'prebuilt') fs.mkdirSync(protectedDirectory, { recursive: true });
    for (const [sourceDirectory, destinationDirectory, names] of [
      [stage, backend, fs.readdirSync(stage)],
      ...(source !== 'prebuilt' ? [] : [[runtime, protectedDirectory, ['protected-build.json', 'runtime-compatibility.json', identity.manifestFile, identity.ciphertextFile]]]),
    ]) {
      for (const name of names) {
        const source = path.join(sourceDirectory, name), destination = path.join(destinationDirectory, name);
        if (!fs.existsSync(destination) || fs.statSync(source).size !== fs.statSync(destination).size || await hash(source) !== await hash(destination)) fs.copyFileSync(source, destination);
      }
    }
    resourceRuntime = source !== 'prebuilt' ? null : protectedDirectory;
  }
  process.env.NOVA_PUBLIC_BACKEND_DIR = stage;
  const temporary = path.join(root, '.tmp/ci-config', target); fs.mkdirSync(temporary, { recursive: true });
  let icon;
  if (platform.os !== 'win32') {
    const png = iconPng(temporary);
    run(process.execPath, [npm, 'run', 'tauri', '--', 'icon', png, '-o', path.join(temporary, 'icons')], { cwd: desktop });
    icon = path.join(temporary, 'icons', platform.os === 'darwin' ? 'icon.icns' : 'icon.png');
  }
  const stockpile = await stageStockpileResource(path.join(cargo.core, 'release', `stockpile_server${platform.suffix}`), target, platform);
  const overlay = tauriOverride(base, { platform, stage, runtime: resourceRuntime, stockpile, icon, candidate: candidate || community || compileOnly, fullRuntime: !community });
  if (compileOnly) overlay.build = { beforeBuildCommand: 'npm run build' };
  const config = path.join(temporary, 'tauri.json'); fs.writeFileSync(config, JSON.stringify(overlay, null, 2));
  if (compileOnly) {
    process.env.LBA_BUILD_GIT_SHA = sha;
    process.env.CARGO_TARGET_DIR = cargo.host;
    run(process.execPath, [npm, 'run', 'tauri', '--', 'build', '--debug', '--no-bundle', '--config', config], { cwd: desktop });
    console.log(`Community desktop compiled without private components: target=${target} source=${sha} runtime=${source}; no GUI started.`);
    return;
  }
  if (development) {
    delete process.env.LBA_BUILD_GIT_SHA;
    process.env.LBA_DATA_ROOT ||= path.join(root, 'data');
    if (args.includes('--prepare')) { console.log(`Development inputs ready: target=${target} appVersion=${base.version} runtimeAbi=${identity.runtimeAbi} runtimeBuildId=${identity.buildId}; no GUI started.`); return; }
    run(process.execPath, [npm, 'run', 'tauri', '--', 'dev', '--config', config, ...(source === 'local' ? ['--features','development-runtime'] : []), ...devArguments], { cwd: desktop });
    return;
  }
  process.env.LBA_BUILD_GIT_SHA = sha;
  if (platform.os === 'win32') {
    // Validate the effective production overlay, not just the development
    // config. A source/output-hash receipt reuses the exact prepared plugin.
    run('powershell.exe', ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'tools/nsis-duilib/build.ps1')]);
    run('powershell.exe', ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'tools/nsis-duilib/check-installer.ps1'), '-TauriConfigOverlay', config, '-Compile', '-RunAbi']);
  }
  const cargoTarget = cargo.host; process.env.CARGO_TARGET_DIR = cargoTarget;
  removePreviousDistributables(cargoTarget, platform);
  for (const command of tauriBuildStages(platform, config)) {
    console.log(`Public candidate phase=${command[0]} target=${target}`);
    run(process.execPath, [npm, 'run', 'tauri', '--', ...command], { cwd: desktop });
  }
  if (platform.os === 'darwin') archiveMacApplication(cargoTarget);
  if (output('git', ['rev-parse', '--verify', 'HEAD']) !== sha) throw new Error('Source commit changed during the platform build.');
  await collect(target, platform, sha, cargoTarget, base.version);
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) build().catch((error) => { console.error(error.message); process.exitCode = 1; });
