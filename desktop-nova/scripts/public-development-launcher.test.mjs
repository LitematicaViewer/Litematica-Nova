import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { backendNames, validateCargoMetadata, validatePackage, validateRef, validateStaging } from './check-public-checkout.mjs';

const sha = 'a'.repeat(40), oldSha = 'b'.repeat(40);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
function fixture(t) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'nova-public-launcher-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}
function publicMetadata(root) {
  return { packages: [{
    name: 'litematica_core', manifest_path: path.join(root, 'tools/viewer-core/Cargo.toml'),
    features: { default: ['public-beta'], 'public-beta': [], 'datafix-oracle': [] }, dependencies: [],
    targets: ['derive_resource_pack_32x', 'litematica_core', 'litematica_native_viewer', 'stockpile_server'].map(name => ({ name, kind: ['bin'], src_path: path.join(root, `tools/viewer-core/src/bin/${name}.rs`) })),
  }] };
}
function packageFixture(root) {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const ciphertext = Buffer.alloc(48, 7);
  const config = {
    schema: 'nova.protected-build.v1', version: '0.2.0', buildId: 'test-build', moduleId: 'nova_protected_module.dll',
    manifestPublicKey: publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64'), endpoint: 'https://example.invalid/nova',
    manifestFile: 'nova_protected_module.dll.manifest.json', ciphertextFile: 'nova_protected_module.dll.enc',
  };
  const manifest = { protocol: 1, build_id: config.buildId, module_id: config.moduleId, plaintext_sha256: 'd'.repeat(64), ciphertext_sha256: hash(ciphertext), ciphertext_bytes: ciphertext.length, nonce: Buffer.alloc(12, 3).toString('base64'), created_ms: 1000 };
  const signed = { manifest, signature: sign(null, Buffer.from(JSON.stringify(manifest)), privateKey).toString('base64') };
  writeFileSync(path.join(root, 'protected-build.json'), JSON.stringify(config));
  writeFileSync(path.join(root, config.manifestFile), JSON.stringify(signed));
  writeFileSync(path.join(root, config.ciphertextFile), ciphertext);
  return { config, signed };
}
function stagingFixture(root, identity) {
  const deployment = { schema: 'lba.backend.deployment.v1', gitSha: oldSha, profile: 'release', artifacts: {} };
  for (const name of backendNames) {
    const bytes = Buffer.from(`fixture for ${name} ${identity?.buildId ?? ''} ${identity?.manifestPublicKey ?? ''}`);
    writeFileSync(path.join(root, name), bytes);
    deployment.artifacts[name] = { bytes: bytes.length, sha256: hash(bytes) };
  }
  writeFileSync(path.join(root, 'backend-deployment.json'), JSON.stringify(deployment));
}

test('公开 UI 功能分支允许开发；正式构建仍限定 main 或精确 main CI SHA', () => {
  assert.doesNotThrow(() => validateRef({ branch: 'dev/ui', sha, development: true }));
  assert.throws(() => validateRef({ branch: 'dev/ui', sha }), /reviewed main/);
  assert.doesNotThrow(() => validateRef({ branch: 'main', sha }));
  const env = { GITHUB_ACTIONS: 'true', GITHUB_REF: 'refs/heads/main', GITHUB_SHA: sha };
  assert.doesNotThrow(() => validateRef({ branch: '', sha, env }));
  assert.throws(() => validateRef({ branch: '', sha: oldSha, env }), /reviewed main/);
});

test('开发许可不绕过实际公开 Cargo 图；私有图改分支名仍拒绝', t => {
  const root = fixture(t), metadata = publicMetadata(root);
  assert.doesNotThrow(() => validateCargoMetadata(metadata, root, true));
  metadata.packages[0].features = { default: [] };
  assert.throws(() => validateCargoMetadata(metadata, root, true), /isolated public Core/);
  const withDependency = publicMetadata(root);
  withDependency.packages[0].dependencies.push({ name: 'renderer-vulkan', path: path.join(root, 'tools/viewer-core/crates/renderer-vulkan') });
  assert.throws(() => validateCargoMetadata(withDependency, root, true), /local build dependency/);
  const disguised = publicMetadata(root);
  disguised.packages[0].dependencies.push({ name: 'apparently-public', path: path.join(root, 'tools/another-local-implementation') });
  assert.throws(() => validateCargoMetadata(disguised, root, true), /local build dependency/);
});

test('传输依赖不得开启服务端 feature 或私有可执行目标', t => {
  const root = fixture(t), metadata = publicMetadata(root);
  metadata.packages.push({ name: 'nova-protection', manifest_path: path.join(root, 'tools/nova-protection/Cargo.toml'), features: { client: [], server: [] }, targets: [], dependencies: [] });
  assert.throws(() => validateCargoMetadata(metadata, root, true), /public client transport/);
});

test('合法签名包可重复只读验证；manifest 认证失败必须终止', async t => {
  const root = fixture(t), { config, signed } = packageFixture(root);
  const before = readFileSync(path.join(root, config.manifestFile));
  assert.equal((await validatePackage(root)).buildId, config.buildId);
  assert.deepEqual(readFileSync(path.join(root, config.manifestFile)), before);
  signed.manifest.created_ms += 1;
  writeFileSync(path.join(root, config.manifestFile), JSON.stringify(signed));
  await assert.rejects(validatePackage(root), /signature verification failed/);
});

test('签名有效也必须验证实际模块内容；缺失结果不放行', async t => {
  const root = fixture(t), { config } = packageFixture(root);
  writeFileSync(path.join(root, config.ciphertextFile), Buffer.alloc(48, 8));
  await assert.rejects(validatePackage(root), /payload content mismatch/);
  rmSync(path.join(root, config.ciphertextFile));
  await assert.rejects(validatePackage(root), /ENOENT/);
});

test('UI 提交改变可复用真实旧 helper；正式发行必须当前 source', async t => {
  const root = fixture(t); stagingFixture(root);
  assert.equal((await validateStaging(root, { sha })).gitSha, oldSha);
  await assert.rejects(validateStaging(root, { sha, requireCurrentSource: true }), /current source commit/);
  assert.equal((await validateStaging(root, { sha: oldSha, requireCurrentSource: true })).gitSha, oldSha);
});

test('skip-prepare 不接受缺失、额外或被替换的四后端', async t => {
  const root = fixture(t); stagingFixture(root);
  const filename = path.join(root, backendNames[0]);
  writeFileSync(filename, Buffer.from('replacement'));
  await assert.rejects(validateStaging(root), /content mismatch/);
  stagingFixture(root); rmSync(filename);
  await assert.rejects(validateStaging(root), /incomplete/);
  stagingFixture(root); writeFileSync(path.join(root, 'unexpected.pdb'), 'not distributed');
  await assert.rejects(validateStaging(root), /unexpected/);
});

test('部署哈希相符仍拒绝与当前签名包身份不同的旧 helper', async t => {
  const root = fixture(t), identity = { buildId: 'fixture-r2', manifestPublicKey: 'public-key-only-fixture' };
  stagingFixture(root, identity);
  assert.equal((await validateStaging(root, { sha, identity })).gitSha, oldSha);
  await assert.rejects(validateStaging(root, { identity: { ...identity, buildId: 'another-build' } }), /runtime identity differs/);
  await assert.rejects(validateStaging(root, { identity: { ...identity, manifestPublicKey: 'another-public-key' } }), /runtime identity differs/);
});

test('Windows 启动器统一进入开发预检；check/skip 不调用 GUI 或跳过校验', () => {
  const launcher = readFileSync(new URL('../../bin/dev-win.cmd', import.meta.url), 'utf8');
  assert.doesNotMatch(launcher, /refuses anything but main|checkout main|if \/i not "%BRANCH%"/);
  assert.match(launcher, /-Development -CheckOnly/);
  assert.match(launcher, /-Development -ReuseExisting[\s\S]*?if errorlevel 1 goto fail[\s\S]*?goto mirror_staging/);
  assert.match(launcher, /"%PREPARE_SCRIPT%" -RepoRoot "%REPO_ROOT%" -Development/);
  const script = readFileSync(new URL('./prepare-protected-backend.ps1', import.meta.url), 'utf8');
  const checkExit = script.indexOf('if ($CheckOnly -or $ReuseExisting)');
  assert.ok(checkExit > script.indexOf('& node @checkArgs'));
  assert.ok(script.indexOf('New-Item -ItemType Directory') > checkExit);
  assert.ok(script.indexOf('& cargo build') > checkExit);
});
