import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { sealTask, openTask } from './ci-task-envelope.mjs';
import { allowedSnapshotPath, validateSnapshot } from './validate-private-snapshot.mjs';

const scripts = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(scripts, '../..');
const arg = (key) => { const index = process.argv.indexOf(key); return index < 0 ? undefined : process.argv[index + 1]; };
function context(direction) {
  return { schema: 'nova.ci.task.v1', taskId: arg('--task') || process.env.GITHUB_RUN_ID, sourceSha: arg('--source-sha'), target: arg('--target'), direction };
}
function run(program, args, { cwd = root, log, input } = {}) {
  const environment = { ...process.env }; delete environment.NOVA_CI_TASK_KEY;
  const fd = log ? fs.openSync(log, 'a', 0o600) : null;
  let result;
  try { result = spawnSync(program, args, { cwd, env: environment, windowsHide: true, input, encoding: input ? 'utf8' : undefined, stdio: log ? ['ignore', fd, fd] : ['pipe', 'pipe', 'pipe'], maxBuffer: 32 * 1024 ** 2 }); }
  finally { if (fd !== null) fs.closeSync(fd); }
  if (result.error || result.status !== 0) throw new Error('Task subprocess failed.');
  return result.stdout?.toString('utf8').trim();
}
function python(args) { return run(process.platform === 'win32' ? 'py' : 'python3', [...(process.platform === 'win32' ? ['-3'] : []), path.join(scripts, 'ci-task-archive.py'), ...args]); }
function temporary() {
  const parent = process.env.RUNNER_TEMP || path.join(root, '.tmp'); fs.mkdirSync(parent, { recursive: true });
  return fs.mkdtempSync(path.join(parent, 'nova-ci-task-'));
}
function info(message) { console.log(`[nova-task] stage=${message}`); }
async function prepare() {
  const ctx = context('source'), seed = arg('--seed');
  if (!/^[a-f0-9]{40}$/.test(ctx.sourceSha || '') || !/^[A-Za-z0-9_.-]{8,100}$/.test(seed || '')) throw new Error('Invalid fixed source or build seed.');
  if (run('git', ['rev-parse', `${ctx.sourceSha}^{commit}`]) !== ctx.sourceSha) throw new Error('The requested immutable source commit is absent.');
  const entries = run('git', ['ls-tree', '-r', ctx.sourceSha]).split('\n').filter(Boolean);
  const files = entries.map((entry) => {
    const match = /^(\d+) blob [a-f0-9]+\t(.+)$/.exec(entry);
    if (!match || !allowedSnapshotPath(match[2]) || /\/(?:tests|benches|examples|fixtures|testdata)\//.test(match[2]) || /\.(?:litematic|schematic)(?:\.gz)?$|\.(?:jar|zip)$/i.test(match[2])) return null;
    if (match[1] !== '100644' && match[1] !== '100755') throw new Error('Source links are not admitted.');
    return match[2];
  }).filter(Boolean);
  if (!files.includes('desktop-nova/scripts/validate-private-snapshot.mjs')) throw new Error('Commit the snapshot-capable build scripts before dispatching this source task.');
  const directory = temporary(), tar = path.join(directory, 'source.tar'), zip = path.join(directory, 'source.zip');
  try {
  const descriptor = fs.openSync(tar, 'wx', 0o600);
  try {
    const environment = { ...process.env }; delete environment.NOVA_CI_TASK_KEY;
    const result = spawnSync('git', ['archive', '--format=tar', ctx.sourceSha, 'tools/viewer-core','tools/nova-protected-module','tools/nova-protected-loader','tools/nova-protection','tools/nova-light-wire','third_party/wgpu-hal-patched','data/blockdb','pack-in/materials/minecraft-26.3.json','pack-in/lang/zh_cn.json','desktop-nova/scripts','desktop-nova/src-tauri/tauri.conf.json','desktop-nova/src-tauri/icons/icon.png','desktop-nova/src-tauri/icons/icon.ico'], { cwd: root, env: environment, stdio: ['ignore', descriptor, 'pipe'], windowsHide: true });
    if (result.error || result.status !== 0) throw new Error('Immutable source export failed.');
  } finally { fs.closeSync(descriptor); }
  const tooling = path.join(root, '.tmp/beta-ui-corrections/amice-toolchain');
  const publicKey = fs.readFileSync(arg('--public-key-file') || path.join(root, '.tmp/beta-ui-corrections/protection-release/public/manifest-ed25519.pub'), 'utf8').trim();
  if (Buffer.from(publicKey, 'base64').length !== 32) throw new Error('The public build identity is invalid.');
  const request = { sourceSha: ctx.sourceSha, target: ctx.target, taskId: ctx.taskId, seed, manifestPublicKey: publicKey, protectedEndpoint: 'https://45-62-112-121.sslip.io:8448/nova-core', files, amiceSource: path.join(tooling, 'source/fuqiuluo-amice-aa5cce0') };
  const requestFile = path.join(directory, 'request.json'); fs.writeFileSync(requestFile, JSON.stringify(request), { mode: 0o600 });
  const output = path.resolve(arg('--output')); fs.mkdirSync(path.dirname(output), { recursive: true });
  python(['source', requestFile, tar, zip]); await sealTask(zip, output, ctx);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
  info('input-ready');
}
async function unpack(direction) {
  const directory = path.resolve(arg('--directory')), zip = path.join(temporary(), 'payload.zip');
  try { await openTask(path.resolve(arg('--input')), zip, context(direction)); python(['extract', zip, directory, direction]); }
  finally { if (fs.existsSync(zip)) fs.unlinkSync(zip); }
  if (direction === 'source') {
    const task = JSON.parse(fs.readFileSync(path.join(directory, 'task.json'), 'utf8'));
    const ctx = context(direction);
    if (task.sourceSha !== ctx.sourceSha || task.target !== ctx.target || task.taskId !== ctx.taskId) throw new Error('Source task binding differs.');
    await validateSnapshot(path.join(directory, 'source'), path.join(directory, 'source/snapshot.json'), ctx.target);
  }
  info(`${direction}-ready`);
}
async function execute() {
  if (process.platform !== 'darwin' || process.arch !== 'arm64') throw new Error('Use the selected native ARM macOS build runner.');
  const directory = path.resolve(arg('--directory')), task = JSON.parse(fs.readFileSync(path.join(directory, 'task.json'), 'utf8'));
  const ctx = context('result');
  if (task.sourceSha !== ctx.sourceSha || task.target !== ctx.target || task.taskId !== ctx.taskId) throw new Error('Source task binding differs.');
  const source = path.join(directory, 'source'), build = path.join(source, '.tmp/protected-build', task.seed, task.target);
  fs.mkdirSync(build, { recursive: true });
  const log = path.join(build, 'task-driver.log'), config = path.join(directory, 'toolchain.json'); let exitCode = 0, phase = 'toolchain';
  try {
    await validateSnapshot(source, path.join(source, 'snapshot.json'), task.target);
    run(process.execPath, [path.join(scripts, 'setup-macos-ci.mjs'), '--target', task.target, '--tooling-root', path.join(directory, 'tooling'), '--output', config], { log });
    const tools = JSON.parse(fs.readFileSync(config, 'utf8'));
    process.env.RUSTUP_TOOLCHAIN = tools.rustToolchain;
    process.env.PATH = [path.join(os.homedir(), '.cargo/bin'), path.join(tools.llvmRoot, 'bin'), process.env.PATH].join(path.delimiter);
    // Keep Rust and all Cargo C/C++ dependencies on the same deployment target.
    // Toolchain bootstrap above runs on the native runner and is not distributed.
    process.env.MACOSX_DEPLOYMENT_TARGET = '13.0';
    phase = 'module';
    run('pwsh', ['-NoProfile','-File',path.join(source, 'desktop-nova/scripts/build-private-protected.ps1'),'-Stage','backend','-RepoRoot',source,'-SourceSnapshotManifest',path.join(source,'snapshot.json'),'-Seed',task.seed,'-TargetTriple',task.target,'-LlvmRoot',tools.llvmRoot,'-AmicePlugin',tools.amicePlugin,'-VmProfile',tools.vmProfile,'-ManifestPublicKey',task.manifestPublicKey,'-ProtectedEndpoint',task.protectedEndpoint], { cwd: source, log });
  } catch { exitCode = 1; }
  const status = path.join(directory, 'status.json'); fs.writeFileSync(status, JSON.stringify({ schema: 'nova.ci.result.v1', sourceSha: task.sourceSha, target: task.target, taskId: task.taskId, seed: task.seed, exitCode, phase }), { mode: 0o600 });
  const zip = path.join(directory, 'result.zip');
  try { python(['result', status, build, zip, path.join(process.env.RUNNER_TEMP, 'nova-macos-toolchain', task.target), path.join(process.env.RUNNER_TEMP, 'nova-macos-ci-failure.log')]); await sealTask(zip, path.resolve(arg('--output')), ctx); }
  finally { if (fs.existsSync(zip)) fs.unlinkSync(zip); }
  info(`module-exit-${exitCode}`); process.exitCode = exitCode;
}
async function stageResult() {
  const directory = path.resolve(arg('--directory')), ctx = context('result');
  const result = JSON.parse(fs.readFileSync(path.join(directory, 'result.json'), 'utf8'));
  if (result.schema !== 'nova.ci.result.v1' || result.sourceSha !== ctx.sourceSha || result.target !== ctx.target || result.taskId !== ctx.taskId || result.exitCode !== 0) throw new Error('The cloud backend task did not succeed.');
  const manifest = JSON.parse(fs.readFileSync(path.join(directory, 'result/private-build.json'), 'utf8'));
  const module = path.join(directory, 'result/backend/libnova_protected_module.dylib');
  if (manifest.gitSha !== ctx.sourceSha || manifest.targetTriple !== ctx.target || manifest.seed !== result.seed || manifest.amice?.vmApplied !== true || manifest.amice?.targetTriple !== ctx.target || manifest.amice?.objectFormat !== 'macho' || manifest.amice?.sourceCalls !== 1 || manifest.amice?.irCalls !== 4 || Object.keys(manifest.artifacts ?? {}).join() !== 'libnova_protected_module.dylib') throw new Error('The returned module/receipt identity differs.');
  const record = manifest.artifacts['libnova_protected_module.dylib'];
  const hash = crypto.createHash('sha256'); for await (const bytes of fs.createReadStream(module)) hash.update(bytes);
  if (hash.digest('hex') !== record.sha256 || fs.statSync(module).size !== record.bytes) throw new Error('The returned module hash differs.');
  const destination = path.join(root, '.tmp/protected-build', result.seed, result.target), backend = path.join(destination, 'backend');
  fs.mkdirSync(backend, { recursive: true }); const final = path.join(backend, path.basename(module)); fs.copyFileSync(module, final);
  record.path = final;
  fs.writeFileSync(path.join(destination, 'private-build.json'), JSON.stringify(manifest, null, 2), { mode: 0o600 });
  info('module-ready-for-local-seal');
}
async function main() {
  if (process.argv[2] === 'prepare') return prepare();
  if (process.argv[2] === 'open-source') return unpack('source');
  if (process.argv[2] === 'open-result') return unpack('result');
  if (process.argv[2] === 'execute') return execute();
  if (process.argv[2] === 'stage-result') return stageResult();
  if (process.argv[2] === 'new-key') {
    const key = crypto.randomBytes(32), text = key.toString('base64');
    try { run('gh', ['secret','set','NOVA_CI_TASK_KEY','--repo','LitematicaViewer/Litematica-Nova'], { input: text }); }
    finally { key.fill(0); }
    return info('one-task-key-ready');
  }
  throw new Error('Unknown task stage.');
}
main().catch(() => { console.error('[nova-task] stage=failed exit=1; private diagnostics are not printed'); process.exitCode = 1; });
