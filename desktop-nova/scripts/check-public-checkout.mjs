import { createHash, createPublicKey, verify } from 'node:crypto';
import { createReadStream, existsSync, lstatSync, readFileSync, readdirSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const privateSource = /(?:^|\/)(?:render_v[35]|full_mode(?:_v2)?|renderer-vulkan|renderer-gl|iris-compat|nova-protected-module|nova-light-service)(?:\/|\.|$)|(?:^|\/)(?:scene_builder|section_light|model_lighting)\.rs$/;
export const communityBackendNames = ['litematica_core.exe', 'litematica_native_viewer.exe'];
export const communityIdentity = target => ({ runtimeKind: 'community', buildId: 'community', runtimeAbi: 1, target });
export const backendNames = ['litematica_core.exe', 'litematica_native_viewer.exe', 'nova-render-service.exe', 'nova-build-worker.exe'];
export const nativeTargets = {
  'win32-x64': 'x86_64-pc-windows-msvc',
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'darwin-x64': 'x86_64-apple-darwin',
  'darwin-arm64': 'aarch64-apple-darwin',
};
export function nativeTarget() {
  const target = nativeTargets[`${process.platform}-${process.arch}`];
  if (!target) fail('This platform does not have a compatible Nova runtime component.');
  return target;
}
export const moduleForTarget = target => target.includes('windows') ? 'nova_protected_module.dll' : target.includes('apple') ? 'libnova_protected_module.dylib' : 'libnova_protected_module.so';
const fail = message => { throw new Error(message); };
const relative = (repo, filename) => path.relative(repo, filename).split(path.sep).join('/');
const contractPath = fileURLToPath(new URL('../../tools/nova-protection/runtime-contract.json', import.meta.url));
export function runtimeContract() { return json(contractPath, 131072); }
function plainFile(filename, limit) {
  const stat = lstatSync(filename);
  if (!stat.isFile() || stat.isSymbolicLink() || !stat.size || stat.size > limit) fail(`Invalid or oversized build input: ${path.basename(filename)}`);
  return stat;
}
function json(filename, limit) {
  plainFile(filename, limit);
  return JSON.parse(readFileSync(filename, 'utf8').replace(/^\uFEFF/, ''));
}
async function sha256(filename, requiredText = []) {
  const hash = createHash('sha256');
  const markers = requiredText.map(value => ({ bytes: Buffer.from(value), found: false }));
  const overlap = Math.max(1, ...markers.map(marker => marker.bytes.length)) - 1;
  let tail = Buffer.alloc(0);
  for await (const bytes of createReadStream(filename)) {
    hash.update(bytes);
    if (!markers.length) continue;
    const scan = Buffer.concat([tail, bytes]);
    for (const marker of markers) marker.found ||= scan.includes(marker.bytes);
    tail = overlap ? scan.subarray(-overlap) : Buffer.alloc(0);
  }
  if (markers.some(marker => !marker.found)) fail(`Existing backend runtime identity differs; prepare this package again: ${path.basename(filename)}`);
  return hash.digest('hex');
}
function run(repo, executable, args) {
  const result = spawnSync(executable, args, { cwd: repo, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024, windowsHide: true });
  if (result.error || result.status !== 0) fail(`${executable} input check failed: ${result.error?.message ?? result.stderr.trim().slice(0, 600)}`);
  return result.stdout.trim();
}

export function validateRef({ branch, sha, development = false, community = false, env = process.env }) {
  if (!/^[a-f0-9]{40,64}$/.test(sha)) fail('Invalid source commit.');
  if (development || community) return;
  const exactMain = !branch && env.GITHUB_ACTIONS === 'true' && env.GITHUB_REF === 'refs/heads/main' && env.GITHUB_SHA === sha;
  const trustedPrivateBuild = !branch && env.GITHUB_ACTIONS === 'true' && env.GITHUB_REPOSITORY === 'LitematicaViewer/Litematica-Nova-Private' && env.NOVA_APPROVED_PUBLIC_SHA === sha;
  if (branch !== 'main' && !exactMain && !trustedPrivateBuild) fail('Release builds require the reviewed main checkout or the fixed public SHA selected by the trusted private workflow; use -Development for public feature branches.');
}

export function validateCargoMetadata(metadata, repo) {
  if (!Array.isArray(metadata.packages) || !metadata.packages.length) fail('Missing public Cargo metadata.');
  for (const pkg of metadata.packages) {
    for (const dependency of pkg.dependencies ?? []) {
      if (privateSource.test(dependency.name) || dependency.path && privateSource.test(relative(repo, dependency.path))) fail('A private implementation remains in the public build graph.');
    }
    for (const target of pkg.targets ?? []) if (privateSource.test(relative(repo, target.src_path))) fail('A private implementation remains in the public build targets.');
  }
}

export function validateSource(repo) {
  const files = run(repo, 'git', ['ls-files', '-z']).split('\0').filter(Boolean);
  if (files.some(filename => privateSource.test(filename))) fail('Private implementation source is present in this checkout.');
  for (const filename of ['tools/viewer-core/src/full_mode.rs', 'tools/viewer-core/src/full_mode_v2.rs', 'tools/viewer-core/src/render_v3.rs', 'tools/viewer-core/src/render_v5/mod.rs', 'tools/viewer-core/crates/renderer-vulkan/Cargo.toml', 'tools/viewer-core/crates/renderer-gl/Cargo.toml', 'tools/viewer-core/crates/iris-compat/Cargo.toml', 'tools/nova-protected-module/Cargo.toml', 'tools/nova-light-service/Cargo.toml']) {
    if (existsSync(path.join(repo, filename))) fail('Private implementation source is present in this checkout.');
  }
  const lib = readFileSync(path.join(repo, 'tools/viewer-core/src/lib.rs'), 'utf8');
  if (/\b(?:mod|use)\s+(?:crate::)?(?:render_v[35]|full_mode(?:_v2)?|render_service_worker|iris_runtime)\b/.test(lib)) fail('The Core still compiles private implementation modules.');
  // Cargo itself resolves public crates/features during the real build. A fresh
  // checkout must not require a pre-populated offline metadata cache, nor an
  // administrator-maintained list of every legitimate public package.

}

export async function validatePackage(root, expectedModuleId = 'nova_protected_module.dll', { target, release = false, contract = runtimeContract() } = {}) {
  if (existsSync(path.join(root, 'development-runtime.json'))) fail('A local Development Runtime cannot be used as a signed production component.');
  if (!['nova_protected_module.dll', 'libnova_protected_module.so', 'libnova_protected_module.dylib'].includes(expectedModuleId)) fail('Unsupported runtime module platform.');
  const config = json(path.join(root, 'protected-build.json'), 16384);
  const fields = ['schema', 'version', 'buildId', 'moduleId', 'manifestPublicKey', 'endpoint', 'manifestFile', 'ciphertextFile'];
  if (Object.keys(config).some(key => !fields.includes(key))) fail('Unexpected runtime build metadata field.');
  if (config.schema !== 'nova.protected-build.v1' || !/^[a-zA-Z0-9_.:-]{1,128}$/.test(config.buildId)) fail('Invalid runtime build identity.');
  if (config.moduleId !== expectedModuleId || config.manifestFile !== config.moduleId + '.manifest.json' || config.ciphertextFile !== config.moduleId + '.enc') fail('Invalid runtime package paths.');
  const endpoint = new URL(config.endpoint);
  if (endpoint.protocol !== 'https:' || endpoint.username || endpoint.password || endpoint.search || endpoint.hash) fail('The runtime endpoint must be ordinary HTTPS.');
  const publicKey = Buffer.from(config.manifestPublicKey, 'base64');
  if (publicKey.length !== 32 || publicKey.toString('base64') !== config.manifestPublicKey) fail('Invalid Ed25519 public key.');
  if (!contract.trusted_manifest_public_keys?.includes(config.manifestPublicKey)) fail('The runtime package signer is not trusted by this checkout.');
  const allowed = new Set(['protected-build.json', config.manifestFile, config.ciphertextFile, 'runtime-compatibility.json']);
  for (const item of readdirSync(root, { withFileTypes: true })) if (!item.isFile() || !allowed.has(item.name)) fail('Unexpected file in the runtime package.');
  const signed = json(path.join(root, config.manifestFile), 131072);
  if (Object.keys(signed).sort().join(',') !== 'manifest,signature') fail('Unexpected signed manifest fields.');
  const m = signed.manifest;
  const keys = ['protocol', 'build_id', 'module_id', 'plaintext_sha256', 'ciphertext_sha256', 'ciphertext_bytes', 'nonce', 'created_ms'];
  if (Object.keys(m).length !== keys.length || Object.keys(m).some(key => !keys.includes(key))) fail('Unexpected module manifest fields.');
  const canonical = Object.fromEntries(keys.map(key => [key, m[key]]));
  if (m.protocol !== 1 || m.build_id !== config.buildId || m.module_id !== config.moduleId) fail('Module/build binding mismatch.');
  if (!/^[a-f0-9]{64}$/.test(m.plaintext_sha256) || !/^[a-f0-9]{64}$/.test(m.ciphertext_sha256)) fail('Invalid module content hashes.');
  if (!Number.isSafeInteger(m.created_ms) || m.created_ms <= 0 || !Number.isSafeInteger(m.ciphertext_bytes) || m.ciphertext_bytes < 17 || m.ciphertext_bytes > 512 * 1024 * 1024) fail('Invalid module size/time.');
  if (Buffer.from(m.nonce, 'base64').length !== 12) fail('Invalid authentication nonce.');
  const signature = Buffer.from(signed.signature, 'base64');
  const key = createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), publicKey]), format: 'der', type: 'spki' });
  if (signature.length !== 64 || !verify(null, Buffer.from(JSON.stringify(canonical)), key, signature)) fail('Runtime manifest signature verification failed.');
  const ciphertext = path.join(root, config.ciphertextFile);
  if (plainFile(ciphertext, 512 * 1024 * 1024).size !== m.ciphertext_bytes || await sha256(ciphertext) !== m.ciphertext_sha256) fail('Runtime payload content mismatch.');
  const signedCompatibility = json(path.join(root, 'runtime-compatibility.json'), 131072);
  if (Object.keys(signedCompatibility).sort().join(',') !== 'compatibility,signature') fail('Unexpected runtime compatibility fields.');
  const compatibility = signedCompatibility.compatibility;
  const compatibilityKeys = ['schema', 'runtime_abi', 'runtime_build_id', 'target', 'module_id', 'module_manifest_sha256', 'endpoint', 'capabilities', 'created_ms'];
  if (Object.keys(compatibility).length !== compatibilityKeys.length || Object.keys(compatibility).some(name => !compatibilityKeys.includes(name))) fail('Unexpected signed runtime compatibility metadata.');
  const canonicalCompatibility = Object.fromEntries(compatibilityKeys.map(name => [name, compatibility[name]]));
  const manifestHash = createHash('sha256').update(JSON.stringify({ manifest: canonical, signature: signed.signature })).digest('hex');
  const compatibilitySignature = Buffer.from(signedCompatibility.signature, 'base64');
  if (compatibilitySignature.length !== 64 || !verify(null, Buffer.from(JSON.stringify(canonicalCompatibility)), key, compatibilitySignature)) fail('Runtime compatibility signature verification failed.');
  if (compatibility.schema !== 'nova.runtime-compatibility.v1' || compatibility.runtime_abi !== contract.runtime_abi || compatibility.runtime_build_id !== config.buildId || compatibility.module_id !== config.moduleId || compatibility.module_manifest_sha256 !== manifestHash || compatibility.endpoint !== config.endpoint || !Number.isSafeInteger(compatibility.created_ms) || compatibility.created_ms <= 0) fail('The signed runtime ABI/build/content identity is incompatible with this checkout.');
  if (!contract.targets?.includes(compatibility.target) || (target && compatibility.target !== target)) fail('The signed runtime target does not match the selected native platform.');
  if (!Array.isArray(compatibility.capabilities) || new Set(compatibility.capabilities).size !== compatibility.capabilities.length || contract.required_capabilities.some(name => !compatibility.capabilities.includes(name))) fail('The runtime does not provide the required signed capabilities.');
  if (release && !contract.approved_release_runtimes?.some(item => item.target === compatibility.target && item.runtime_build_id === config.buildId && item.module_manifest_sha256 === manifestHash)) fail('This runtime build has not been approved for a release of this checkout.');
  return { ...config, runtimeAbi: compatibility.runtime_abi, target: compatibility.target, runtimeCapabilities: compatibility.capabilities, signedCompatibility };
}

export function runtimeSource(value = process.env.NOVA_RUNTIME_SOURCE || 'community') {
  if (!['community', 'prebuilt', 'local'].includes(value)) fail('Select --runtime-source community, prebuilt or local.');
  return value;
}

export async function validateDevelopmentPackage(directory, expectedModuleId, { target, contract = runtimeContract() } = {}) {
  const descriptor = json(path.join(directory, 'development-runtime.json'), 131072);
  const fields = ['schema', 'runtime_abi', 'runtime_build_id', 'target', 'module_id', 'module_sha256', 'capabilities'];
  if (Object.keys(descriptor).length !== fields.length || Object.keys(descriptor).some(name => !fields.includes(name)) || descriptor.schema !== 'nova.development-runtime.v1') fail('Invalid local Development Runtime descriptor.');
  if (descriptor.runtime_abi !== contract.runtime_abi || !contract.targets.includes(descriptor.target) || (target && descriptor.target !== target) || descriptor.module_id !== expectedModuleId) fail('The local Development Runtime ABI or target is incompatible with this Host.');
  if (!/^[A-Za-z0-9_.:-]{1,128}$/.test(descriptor.runtime_build_id) || !/^[a-f0-9]{64}$/.test(descriptor.module_sha256) || !Array.isArray(descriptor.capabilities) || descriptor.capabilities.some(name => typeof name !== 'string') || contract.required_capabilities.some(name => !descriptor.capabilities.includes(name)) || new Set(descriptor.capabilities).size !== descriptor.capabilities.length) fail('The local Development Runtime identity or required capabilities are incomplete.');
  const expected = ['development-runtime.json', expectedModuleId].sort();
  if (JSON.stringify(readdirSync(directory).sort()) !== JSON.stringify(expected)) fail('The local Development Runtime directory must contain only its descriptor and actual native module.');
  const modulePath = path.join(directory, expectedModuleId);
  if (plainFile(modulePath, 512 * 1024 * 1024).size < 64 || await sha256(modulePath) !== descriptor.module_sha256) fail('The actual local Development Runtime module is missing or differs from its content identity.');
  return { schema: descriptor.schema, runtimeKind: 'development', buildId: descriptor.runtime_build_id, moduleId: descriptor.module_id, runtimeAbi: descriptor.runtime_abi, target: descriptor.target, runtimeCapabilities: descriptor.capabilities, developmentDescriptor: descriptor, modulePath };
}

// This identity deliberately excludes AppVersion, UI files, Git HEAD and the
// developer's checkout path. Cargo still owns compilation and dependency reuse.
// The digest records the actual public helper source/resources/toolchain and
// the selected signed runtime, not a conveniently similar old executable.
export async function helperInputIdentity(repo, identity, target = nativeTarget()) {
  const roots = ['tools/viewer-core', 'tools/nova-protected-loader', 'tools/nova-protection', 'third_party/wgpu-hal-patched', '.cargo', 'rust-toolchain.toml', 'rust-toolchain', 'data/blockdb', 'pack-in/materials/minecraft-26.3.json', 'pack-in/lang/zh_cn.json', 'desktop-nova/src-tauri/icons/icon.png'];
  const names = [...new Set(run(repo, 'git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', ...roots]).split('\0').filter(Boolean))].filter(name => !/(?:^|\/)(?:tests|examples|scripts|target)\//.test(name) && /\.(?:rs|toml|lock|json|js|css|tsv|wgsl|glsl|vert|frag|h|c|cpp|rc|ico|png|z)$/.test(name)).sort();
  const digest = createHash('sha256').update(JSON.stringify({ schema: 'nova.public-helper-input.v1', target, profile: 'release', features: identity.developmentDescriptor ? ['public-beta', 'development-runtime'] : ['public-beta'], runtimeBuildId: identity.buildId, manifestPublicKey: identity.manifestPublicKey, endpoint: identity.endpoint, compatibility: identity.signedCompatibility, development: identity.developmentDescriptor, developmentModulePath: identity.modulePath, compiler: run(repo, 'rustc', ['-vV']) }));
  for (const name of names) {
    const filename = path.join(repo, name);
    plainFile(filename, 512 * 1024 * 1024);
    digest.update('\0' + name + '\0');
    for await (const bytes of createReadStream(filename)) digest.update(bytes);
  }
  return digest.digest('hex');
}

export async function validateStaging(stage, { sha, requireCurrentSource = false, identity, helperInputId, names = backendNames } = {}) {
  const deployment = json(path.join(stage, 'backend-deployment.json'), 16384);
  const required = identity?.runtimeKind === 'community' ? names.filter(name => name.startsWith('litematica_')) : names;
  const declared = Object.keys(deployment.artifacts ?? {});
  if (deployment.schema !== 'lba.backend.deployment.v1' || deployment.profile !== 'release' || !/^[a-f0-9]{40,64}$/.test(deployment.gitSha) || required.some(name => !declared.includes(name)) || declared.some(name => !/^[A-Za-z0-9_.-]+$/.test(name))) fail('Invalid existing backend deployment identity.');
  if (readdirSync(stage).some(name => name !== 'backend-deployment.json' && !declared.includes(name))) fail('Existing backend staging contains undeclared files.');
  if (requireCurrentSource && deployment.gitSha !== sha) fail('Release helper source does not match the current source commit.');
  if (helperInputId && deployment.helperInputId !== helperInputId) fail('Existing helper build inputs changed; run the normal preparation instead of --skip-prepare.');
  if (identity && deployment.runtimeBuildId && deployment.runtimeBuildId !== identity.buildId) fail('Existing backend runtime build differs from the selected package.');
  if (identity && (identity.runtimeKind || 'production') !== (deployment.runtimeKind || 'production')) fail('The existing helpers were built for a different production/development runtime route.');
  for (const name of declared) {
    const filename = path.join(stage, name), record = deployment.artifacts[name];
    const stat = plainFile(filename, 512 * 1024 * 1024);
    if (stat.size !== record.bytes || !/^[a-f0-9]{64}$/.test(record.sha256) || await sha256(filename, identity && identity.runtimeKind !== 'community' ? [identity.buildId, identity.manifestPublicKey].filter(Boolean) : []) !== record.sha256) fail(`Existing backend content mismatch: ${name}`);
  }
  return deployment;
}

export async function checkCheckout(repo, { development = false, reuseExisting = false, noDownload = false, runtimePackageDir = process.env.NOVA_RUNTIME_PACKAGE_DIR, source = runtimeSource(), privateSourceDir = process.env.NOVA_PRIVATE_SOURCE_DIR } = {}) {
  repo = realpathSync(repo);
  const branch = run(repo, 'git', ['branch', '--show-current']);
  const sha = run(repo, 'git', ['rev-parse', '--verify', 'HEAD']);
  source = runtimeSource(source);
  validateRef({ branch, sha, development, community: source === 'community' });
  validateSource(repo);
  const target = nativeTarget();
  source = runtimeSource(source);
  if (!development && source === 'local') fail('Local Development Runtime components are forbidden in release builds.');
  const directory = source === 'community' ? null : source === 'local'
    ? await (await import('./fetch-ci-runtime.mjs')).acquireLocalDevelopmentRuntime(repo, target, { privateSourceDir, checkOnly: noDownload, reuseExisting })
    : development && !noDownload
    ? await (await import('./fetch-ci-runtime.mjs')).ensureDevelopmentRuntime(repo, target, runtimePackageDir)
    : runtimePackageDir || path.join(repo, development ? 'bin/viewer-backend/protected' : target.includes('windows') ? 'bin/protected' : `bin/runtime/${target}`);
  const identity = source === 'community' ? communityIdentity(target) : source === 'local' ? await validateDevelopmentPackage(directory, moduleForTarget(target), { target }) : await validatePackage(directory, moduleForTarget(target), { target, release: !development });
  const helperInputId = await helperInputIdentity(repo, identity, target);
  let deployment = null;
  if (reuseExisting || existsSync(path.join(repo, 'bin/public-beta/backend-deployment.json'))) {
    try { deployment = await validateStaging(path.join(repo, 'bin/public-beta'), { identity, helperInputId }); }
    catch (error) { if (reuseExisting) throw error; }
  }
  return { sha, identity, runtimeSource: source, runtimePackageDir: directory, helperInputId, backendSource: deployment?.helperSourceSha ?? deployment?.gitSha ?? null };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2), index = args.indexOf('--repo'), take = name => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
  checkCheckout(index >= 0 ? args[index + 1] : process.cwd(), { development: args.includes('--development'), reuseExisting: args.includes('--reuse-existing'), noDownload: args.includes('--no-download'), source: take('--runtime-source') || runtimeSource(), privateSourceDir: take('--private-source') || process.env.NOVA_PRIVATE_SOURCE_DIR })
    .then(result => console.log(JSON.stringify(result)))
    .catch(error => { console.error(error.message); process.exitCode = error.code === 'NOVA_RUNTIME_DOWNLOAD_AUTH_REQUIRED' ? 77 : 1; });
}
