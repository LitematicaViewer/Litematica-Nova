import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const privatePaths = /(?:^|\/)(?:render_v5|renderer-gl|renderer-vulkan|iris-compat|nova-build-worker|nova-render-service|shaderpacks-private)(?:\/|$)/i;
const generatedOrSecretPaths = /(?:^|\/)(?:\.tmp|\.ssh|node_modules|target|arr-private|viewer-backend)(?:\/|$)|\.(?:exe|dll|pdb|jar|pem|key|pfx|p12|jks|sqlite|sqlite3)$/i;
const privateMarkers = ['RenderV5Backend', 'RenderV5Request', 'PreparedV5Artifact', 'nova-render-service', 'nova-build-worker', 'iris_runtime', 'renderer-gl', 'renderer-vulkan', 'iris-compat', 'src/render_v5/', 'src\\render_v5\\'];
const privateModule = /\b(?:mod|use)\s+(?:crate::)?(?:render_v5|iris_runtime|iris_gl)\b|\b(?:renderer-gl|renderer-vulkan|iris-compat)\s*=/;
const privateKey = /-----BEGIN (?:RSA |EC |OPENSSH |DSA |ENCRYPTED )?PRIVATE KEY-----[\r\n]+\s*[A-Za-z0-9+/=]{32,}/;
const sensitiveCookie = /(?:set-cookie\s*:\s*[^\r\n]*?\bctoken\s*=|["']?ctoken["']?\s*[:=]\s*["']?)([A-Za-z0-9_+/.=-]{12,}|\[REDACTED\])/ig;
const safeCookie = value => value === '[REDACTED]' || value === 'REDACTED';
const textExtensions = /\.(?:rs|java|toml|lock|json|js|mjs|ts|tsx|yml|yaml|md|txt|ps1|sh|css|wgsl|glsl|vert|frag)$/i;

function run(executable, args, options = {}) {
  const result = spawnSync(executable, args, { cwd: repo, maxBuffer: 256 * 1024 * 1024, ...options });
  if (result.error || result.status !== 0) throw new Error(`${path.basename(executable)} command failed (${result.status ?? 'spawn'}): ${result.error?.message ?? String(result.stderr).trim().slice(0, 800)}`);
  return result.stdout;
}
function git(args, binary = false) { return run('git', args, { encoding: binary ? undefined : 'utf8' }); }
function cookieLeak(text) {
  sensitiveCookie.lastIndex = 0;
  for (const match of text.matchAll(sensitiveCookie)) if (!safeCookie(match[1])) return true;
  return false;
}
function deniedSource(name) {
  if (generatedOrSecretPaths.test(name) || privatePaths.test(name)) return true;
  return false;
}
function binaryMarkerHits(bytes) {
  return privateMarkers.filter(marker => bytes.includes(Buffer.from(marker)) || bytes.includes(Buffer.from(marker, 'utf16le')));
}
function isWindowsX64Exe(bytes) {
  if (bytes.length < 64 || bytes.toString('ascii', 0, 2) !== 'MZ') return false;
  const offset = bytes.readUInt32LE(0x3c);
  return offset + 6 <= bytes.length && bytes.readUInt32LE(offset) === 0x00004550 && bytes.readUInt16LE(offset + 4) === 0x8664;
}

function sourceGate(ref) {
  const commit = git(['rev-parse', '--verify', `${ref}^{commit}`]).trim();
  const records = git(['ls-tree', '-rz', '--full-tree', commit]).split('\0').filter(Boolean).map(line => {
    const [metadata, name] = line.split('\t');
    const [mode, type, object] = metadata.split(' ');
    return { mode, type, object, name };
  });
  const rejected = records.filter(item => item.type !== 'blob' || item.mode !== '100644' && item.mode !== '100755' || deniedSource(item.name));
  if (rejected.length) throw new Error(`Public source contains forbidden paths (${rejected.length}): ${rejected.slice(0, 12).map(item => item.name).join(', ')}`);
  const sourcePaths = new Set(records.map(item => item.name));
  for (const name of ['pack-in/materials/minecraft-26.3.json', 'pack-in/materials/item-icons-26.3.json', 'pack-in/lang/zh_cn.json', 'pack-in/game-icons/block.zip']) {
    if (!sourcePaths.has(name)) throw new Error(`Required public compile input is missing: ${name}`);
  }
  if (ref === 'HEAD') {
    const indexed = git(['ls-files', '-z']).split('\0').filter(Boolean);
    const tree = new Set(records.map(item => item.name));
    if (indexed.some(name => deniedSource(name) || !tree.has(name)) || tree.size !== indexed.length) throw new Error('Public source/index paths do not match the audited HEAD tree.');
  }
  for (const item of records.filter(item => textExtensions.test(item.name))) {
    const text = git(['cat-file', 'blob', item.object], true).toString('utf8');
    if (privateKey.test(text) || cookieLeak(text)) throw new Error(`Sensitive text found in ${item.name} (value redacted).`);
    if (/^tools\/viewer-core\/.*\.(?:rs|toml)$|^desktop-nova\/src-tauri\/.*\.(?:rs|toml)$/.test(item.name) && privateModule.test(text)) throw new Error(`Private module or dependency reference in ${item.name}.`);
  }
  // Only real credential headers in history are rejected; identifiers and
  // redacted header samples remain valid. Old Full history is not rejected.
  const history = git(['rev-list', '--objects', commit]).split('\n').map(line => {
    const at = line.indexOf(' '); return at < 0 ? null : { object: line.slice(0, at), name: line.slice(at + 1) };
  }).filter(item => item && /(?:headers|reden-api-samples).*\.(?:txt|md|json)$/i.test(item.name));
  let historicalHeaders = 0;
  for (const item of history) {
    if (git(['cat-file', '-t', item.object]).trim() !== 'blob') continue;
    historicalHeaders += 1;
    if (cookieLeak(git(['cat-file', 'blob', item.object], true).toString('utf8'))) throw new Error(`Historical credential header remains in ${item.name} (value redacted).`);
  }
  console.log(`public_source=verified; tracked_files=${records.length}; historical_headers=${historicalHeaders}; ref=${commit}`);
}

function filesUnder(directory) {
  const files = [];
  for (const entry of readdirSync(directory)) {
    const filename = path.join(directory, entry);
    const stat = lstatSync(filename);
    if (stat.isSymbolicLink()) throw new Error(`Package symlink rejected: ${entry}`);
    if (stat.isDirectory()) files.push(...filesUnder(filename));
    else if (stat.isFile()) files.push(filename);
    else throw new Error(`Package special file rejected: ${entry}`);
  }
  return files;
}
function buildIdentity(executable) {
  const value = JSON.parse(run(executable, ['--public-build-info'], { encoding: 'utf8', timeout: 30_000 }));
  if (value.schema !== 'nova.public.beta.build.v1' || value.flavor !== 'fast-only' || value.publicBeta !== true || value.fullMode !== false) throw new Error(`Public build identity rejected: ${path.basename(executable)}`);
  return value;
}
function verifyMappedResources(directory, records) {
  const manifestDir = path.join(repo, 'desktop-nova/src-tauri');
  const resourceMap = JSON.parse(readFileSync(path.join(manifestDir, 'tauri.conf.json'), 'utf8')).bundle.resources;
  if (!resourceMap || Array.isArray(resourceMap)) throw new Error('Public build requires the audited explicit resource map.');
  const expected = new Set(['Litematica-Nova.exe']);
  const actual = new Map(records.map(record => [record.path, record]));
  let verified = 0;
  for (const [sourceName, destinationName] of Object.entries(resourceMap)) {
    const source = path.resolve(manifestDir, sourceName);
    if (!source.startsWith(`${repo}${path.sep}`) || !existsSync(source) || lstatSync(source).isSymbolicLink()) throw new Error(`Public resource source missing or invalid: ${sourceName}`);
    const destination = path.resolve(directory, destinationName);
    if (!destination.startsWith(`${directory}${path.sep}`)) throw new Error(`Public resource destination escapes package: ${destinationName}`);
    const directorySource = lstatSync(source).isDirectory();
    for (const file of directorySource ? filesUnder(source) : [source]) {
      const relative = directorySource ? path.join(destinationName, path.relative(source, file)) : destinationName;
      const name = relative.split(path.sep).join('/');
      if (expected.has(name)) throw new Error(`Duplicate mapped resource: ${name}`);
      expected.add(name);
      const bytes = readFileSync(file);
      const deployed = actual.get(name);
      if (!deployed || deployed.bytes !== bytes.length || deployed.sha256 !== createHash('sha256').update(bytes).digest('hex')) throw new Error(`Mapped resource missing or byte mismatch: ${name}`);
      verified += 1;
    }
  }
  assert.deepEqual([...actual.keys()].sort(), [...expected].sort(), 'Public package inventory must exactly match app and declared resources.');
  return verified;
}
function artifactGate(input) {
  if (process.platform !== 'win32') throw new Error('Beta 0.1 desktop runtime artifact verification supports Windows only; macOS/Linux are unsupported, not simulated.');
  const directory = path.resolve(repo, input);
  const expectedExe = new Set(['Litematica-Nova.exe', 'bin/public-beta/litematica_core.exe', 'bin/public-beta/litematica_native_viewer.exe']);
  const permitted = /^(?:Litematica-Nova\.exe|public-build-audit\.json|bin\/public-beta\/(?:litematica_core\.exe|litematica_native_viewer\.exe|backend-deployment\.json)|block\/[^/]+\.png|pack-in\/(?:enumerator|flake|item-icons-26\.3|lang|materials)\/.+|data\/(?:generation-templates|minecraft_blockstates)\/.+|licenses\/(?:THIRD_PARTY_NOTICES\.txt|LICENSE|THIRD_PARTY_UI\.md))$/;
  const files = filesUnder(directory);
  const records = [];
  const exeNames = new Set();
  for (const filename of files) {
    const name = path.relative(directory, filename).split(path.sep).join('/');
    if (!permitted.test(name) || /\.(?:dll|pdb|jar|pem|key|pfx|p12|jks)$/i.test(name) || privatePaths.test(name) || /\.exe$/i.test(name) && !expectedExe.has(name)) throw new Error(`Unexpected public package file: ${name}`);
    if (name === 'public-build-audit.json') continue;
    const bytes = readFileSync(filename);
    const hits = binaryMarkerHits(bytes);
    if (hits.length || privateKey.test(bytes.toString('utf8')) || cookieLeak(bytes.toString('utf8'))) throw new Error(`Sensitive package content in ${name} (markers: ${hits.join(', ') || 'credential'}; values redacted).`);
    if (/\.exe$/i.test(name)) {
      if (!isWindowsX64Exe(bytes)) throw new Error(`Package executable is not a real Windows x64 PE: ${name}`);
      exeNames.add(name);
    }
    records.push({ path: name, bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') });
  }
  assert.deepEqual([...exeNames].sort(), [...expectedExe].sort(), 'Public package must contain exactly three real executables.');
  for (const name of ['THIRD_PARTY_NOTICES.txt', 'LICENSE', 'THIRD_PARTY_UI.md']) {
    if (!existsSync(path.join(directory, 'licenses', name))) throw new Error(`Public package license missing: ${name}`);
  }
  const mappedResources = verifyMappedResources(directory, records);
  const identities = [...expectedExe].map(name => ({ path: name, ...buildIdentity(path.join(directory, name)) }));
  const sourceCommit = process.env.LBA_BUILD_GIT_SHA ?? identities[0].gitSha;
  if (!/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(sourceCommit ?? '') || identities.some(identity => identity.gitSha !== sourceCommit)) throw new Error('Public executables do not match one audited source commit.');
  const result = run(path.join(directory, 'Litematica-Nova.exe'), ['--verify-public-resources'], { encoding: 'utf8', timeout: 60_000 }).trim();
  assert.equal(result, 'public_resources=verified; backends=fast-only; full_mode=absent');
  const report = { schema: 'nova.public.beta.audit.v1', platform: 'windows-x64', sourceCommit, privateMarkerHits: 0, resourcesVerified: true, mappedResources, updaterPayload: false, supportedPlatforms: ['windows-x64'], unsupportedPlatforms: ['macos-x64', 'macos-arm64', 'linux-x64', 'linux-arm64'], identities, files: records };
  const reportPath = path.join(directory, 'public-build-audit.json');
  writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  assert.deepEqual(JSON.parse(readFileSync(reportPath, 'utf8')), report);
  console.log(`public_artifacts=verified; executables=3; files=${records.length}; mapped_resources=${mappedResources}; resources=verified; updater_payload=false`);
}
function verifyPayloadCoverage(expected, extracted) {
  const available = new Map();
  for (const record of extracted) {
    const key = `${record.bytes}:${record.sha256}`;
    available.set(key, (available.get(key) ?? 0) + 1);
  }
  for (const record of expected) {
    const key = `${record.bytes}:${record.sha256}`;
    const count = available.get(key) ?? 0;
    if (!count) throw new Error(`NSIS payload omits or changes audited runtime file: ${record.path}`);
    available.set(key, count - 1);
  }
}
function verifyOfficialNsisPatch(reference, installed) {
  if (reference.equals(installed)) return null;
  const marker = Buffer.from('__TAURI_BUNDLE_TYPE_VAR_UNK');
  const markerOffset = reference.indexOf(marker);
  const offset = markerOffset + marker.length - 3;
  if (reference.length !== installed.length || markerOffset < 0 || reference.indexOf(marker, markerOffset + 1) !== -1 || installed.toString('ascii', offset, offset + 3) !== 'NSS' || !reference.subarray(0, offset).equals(installed.subarray(0, offset)) || !reference.subarray(offset + 3).equals(installed.subarray(offset + 3))) throw new Error('Installed main executable differs outside the official Tauri UNK-to-NSS bundle-type field.');
  return { offset, bytes: 3, before: 'UNK', after: 'NSS' };
}
function installerGate(input, runtimeInput) {
  if (process.platform !== 'win32') throw new Error('NSIS payload verification requires Windows.');
  const installer = path.resolve(repo, input);
  const directory = path.resolve(repo, runtimeInput);
  const audit = JSON.parse(readFileSync(path.join(directory, 'public-build-audit.json'), 'utf8'));
  if (audit.schema !== 'nova.public.beta.audit.v1' || audit.platform !== 'windows-x64' || audit.privateMarkerHits !== 0 || audit.resourcesVerified !== true || !audit.files?.length || !/^[a-f0-9]{40}$/.test(audit.sourceCommit ?? '')) throw new Error('A successful fresh runtime audit is required before NSIS verification.');
  if (process.env.LBA_BUILD_GIT_SHA && audit.sourceCommit !== process.env.LBA_BUILD_GIT_SHA) throw new Error('NSIS runtime audit source commit mismatch.');
  const config = JSON.parse(readFileSync(path.join(repo, 'desktop-nova/src-tauri/tauri.conf.json'), 'utf8'));
  assert.deepEqual(Array.isArray(config.bundle.targets) ? config.bundle.targets : [config.bundle.targets], ['nsis'], 'Public installers must use NSIS only.');
  for (const record of audit.files) {
    const filename = path.resolve(directory, record.path);
    if (!filename.startsWith(`${directory}${path.sep}`) || lstatSync(filename).isSymbolicLink()) throw new Error('Runtime audit path rejected.');
    const bytes = readFileSync(filename);
    if (bytes.length !== record.bytes || createHash('sha256').update(bytes).digest('hex') !== record.sha256) throw new Error(`Runtime changed after audit: ${record.path}`);
  }
  if (!/\.exe$/i.test(installer) || lstatSync(installer).isSymbolicLink()) throw new Error('NSIS installer must be a regular Setup.exe.');
  const bytes = readFileSync(installer);
  // Official NSIS bootstrapper is x86 even when every installed application is x64.
  if (bytes.length < 64 || bytes.toString('ascii', 0, 2) !== 'MZ') throw new Error('NSIS installer is not a Windows executable.');
  const sevenZip = process.env.SEVENZIP_PATH ?? path.join(process.env.ProgramFiles ?? 'C:\\Program Files', '7-Zip/7z.exe');
  const listing = run(sevenZip, ['l', '-slt', installer], { encoding: 'utf8', timeout: 60_000 });
  if (!/^Type = Nsis\s*$/im.test(listing)) throw new Error('Installer is not an actual NSIS archive.');
  const parent = path.dirname(installer);
  if (!parent.startsWith(`${repo}${path.sep}`)) throw new Error('Installer audit staging must stay inside the repository build output.');
  const extractedDir = mkdtempSync(path.join(parent, '.nsis-payload-audit-'));
  const records = [];
  let bundleTypePatch;
  let identities;
  let resourceResult;
  try {
    // Archive extraction only: neither the installer nor any plugin is executed.
    run(sevenZip, ['x', installer, `-o${extractedDir}`, '-y'], { encoding: 'utf8', timeout: 120_000 });
    for (const filename of filesUnder(extractedDir)) {
      const name = path.relative(extractedDir, filename).split(path.sep).join('/');
      const payload = readFileSync(filename);
      if (privatePaths.test(name) || /\.(?:pdb|jar|pem|key|pfx|p12|jks)$/i.test(name) || binaryMarkerHits(payload).length || privateKey.test(payload.toString('utf8')) || cookieLeak(payload.toString('utf8'))) throw new Error(`Sensitive NSIS payload content: ${name} (values redacted).`);
      records.push({ path: name, bytes: payload.length, sha256: createHash('sha256').update(payload).digest('hex') });
    }
    const installedMain = path.join(extractedDir, 'Litematica-Nova.exe');
    bundleTypePatch = verifyOfficialNsisPatch(readFileSync(path.join(directory, 'Litematica-Nova.exe')), readFileSync(installedMain));
    const installedMainRecord = records.find(record => record.path === 'Litematica-Nova.exe');
    if (!installedMainRecord) throw new Error('NSIS main executable is missing from the installed layout.');
    verifyPayloadCoverage(audit.files.map(record => record.path === 'Litematica-Nova.exe' ? installedMainRecord : record), records);
    // Check the actual extracted payload, not the standalone build or the installer.
    const expectedExe = ['Litematica-Nova.exe', 'bin/public-beta/litematica_core.exe', 'bin/public-beta/litematica_native_viewer.exe'];
    identities = expectedExe.map(name => ({ path: name, ...buildIdentity(path.join(extractedDir, name)) }));
    if (identities.some(identity => identity.gitSha !== audit.sourceCommit)) throw new Error('Installed executables do not match the audited source commit.');
    resourceResult = run(installedMain, ['--verify-public-resources'], { encoding: 'utf8', timeout: 60_000 }).trim();
    assert.equal(resourceResult, 'public_resources=verified; backends=fast-only; full_mode=absent');
  } finally {
    const resolved = path.resolve(extractedDir);
    if (!resolved.startsWith(`${parent}${path.sep}.nsis-payload-audit-`) || lstatSync(resolved).isSymbolicLink()) throw new Error('Installer audit cleanup path rejected.');
    rmSync(resolved, { recursive: true });
  }
  const report = { schema: 'nova.public.beta.installer-audit.v1', platform: 'windows-x64', format: 'nsis', sourceCommit: audit.sourceCommit, installer: { path: path.basename(installer), bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') }, privateMarkerHits: 0, auditedRuntimeFiles: audit.files.length, extractedFiles: records.length, payloadContentVerified: true, bundleTypePatch, identities, resourceResult, updaterSignatureVerified: false, installerExecutionVerified: false, files: records };
  const reportPath = path.join(parent, 'public-installer-audit.json');
  writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  assert.deepEqual(JSON.parse(readFileSync(reportPath, 'utf8')), report);
  console.log(`public_installer=verified; format=nsis; runtime_files=${audit.files.length}; extracted_files=${records.length}; private_markers=0; bundle_type_patch=${bundleTypePatch ? 'UNK-to-NSS-only' : 'none'}; resources=verified; updater_signature=false; installer_execution=false`);
}
function selfTest() {
  for (const name of ['tools/viewer-core/src/render_v5/backend.rs', 'tools/viewer-core/crates/iris-compat/src/lib.rs', 'private.key', 'bin/viewer-backend/old.exe']) assert(deniedSource(name));
  for (const name of ['tools/viewer-core/src/native_viewer.rs', 'desktop-nova/src-tauri/src/application/quick_preview.rs', 'desktop-nova/src-tauri/src/application/render_facade.rs', 'pack-in/game-icons/block.zip']) assert(!deniedSource(name));
  assert(cookieLeak(`set-cookie: ctoken=${'sample'.repeat(5)}; Path=/`));
  assert(!cookieLeak('set-cookie: ctoken=[REDACTED]; Path=/'));
  assert(!cookieLeak('ctoken identifier only'));
  assert(privateKey.test(`-----BEGIN PRIVATE KEY-----\n${'A'.repeat(64)}`));
  assert.deepEqual(binaryMarkerHits(Buffer.from('RenderV5Backend', 'utf16le')), ['RenderV5Backend']);
  assert.deepEqual(binaryMarkerHits(Buffer.from('完整模式正在开发中 Full display DTO fast_experimental')), []);
  const payload = { path: 'Litematica-Nova.exe', bytes: 123, sha256: 'sample-hash' };
  verifyPayloadCoverage([payload, payload], [payload, payload]);
  assert.throws(() => verifyPayloadCoverage([payload, payload], [payload]), /omits or changes/);
  assert.throws(() => verifyPayloadCoverage([payload], [{ ...payload, sha256: 'different-hash' }]), /omits or changes/);
  assert(!deniedSource('tools/viewer-core/crates/nova-render-protocol/src/lib.rs'));
  assert.deepEqual(binaryMarkerHits(Buffer.from('nova_render_protocol')), []);
  assert(!privateModule.test('nova-render-protocol = { path = "crates/nova-render-protocol" }'));
  const unknown = Buffer.from('prefix__TAURI_BUNDLE_TYPE_VAR_UNKsuffix');
  const nsis = Buffer.from('prefix__TAURI_BUNDLE_TYPE_VAR_NSSsuffix');
  assert.deepEqual(verifyOfficialNsisPatch(unknown, nsis), { offset: 30, bytes: 3, before: 'UNK', after: 'NSS' });
  assert.equal(verifyOfficialNsisPatch(unknown, unknown), null);
  assert.throws(() => verifyOfficialNsisPatch(unknown, Buffer.from('Prefix__TAURI_BUNDLE_TYPE_VAR_NSSsuffix')), /outside/);
  assert.throws(() => verifyOfficialNsisPatch(unknown, Buffer.from('prefix__TAURI_BUNDLE_TYPE_VAR_MSIsuffix')), /outside/);
  assert.throws(() => verifyOfficialNsisPatch(Buffer.concat([unknown, unknown]), Buffer.concat([nsis, unknown])), /outside/);
  assert.throws(() => verifyOfficialNsisPatch(unknown, Buffer.concat([nsis, Buffer.from('extra')])), /outside/);
  console.log('public_gate_self_test=passed; checks=26');
}

try {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--self-test') selfTest();
  else if (args[0] === '--source' && args[1] === '--ref' && args.length === 3) sourceGate(args[2]);
  else if (args[0] === '--ref' && args.length === 2) sourceGate(args[1]);
  else if (args[0] === '--source' && args.length <= 2) sourceGate(args[1] ?? 'HEAD');
  else if (args[0] === '--artifacts' && args.length === 2 && existsSync(path.resolve(repo, args[1]))) artifactGate(args[1]);
  else if (args[0] === '--installer' && args[2] === '--runtime' && args.length === 4) installerGate(args[1], args[3]);
  else throw new Error('Usage: check-public-build.mjs --source [--ref <sha>] | --artifacts <runtime-package-dir> | --installer <Setup.exe> --runtime <audited-runtime-dir> | --self-test');
} catch (error) { console.error(`public_gate=blocked; ${error.message}`); process.exitCode = 1; }
