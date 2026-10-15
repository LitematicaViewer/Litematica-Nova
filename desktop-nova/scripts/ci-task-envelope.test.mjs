import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { sealTask, openTask } from './ci-task-envelope.mjs';
import { allowedSnapshotPath, validateSnapshot } from './validate-private-snapshot.mjs';
test('task AEAD preserves bytes and rejects another architecture/task and modified tags', async (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'nova-ci-envelope-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const source = path.join(directory, 'source'), envelope = path.join(directory, 'task.nci'); fs.writeFileSync(source, crypto.randomBytes(65539));
  const key = crypto.randomBytes(32).toString('base64');
  const context = { schema: 'nova.ci.task.v1', taskId: '12345', sourceSha: 'a'.repeat(40), target: 'aarch64-apple-darwin', direction: 'source' };
  await sealTask(source, envelope, context, key);
  await openTask(envelope, path.join(directory, 'restored'), context, key);
  assert.deepEqual(fs.readFileSync(source), fs.readFileSync(path.join(directory, 'restored')));
  await assert.rejects(openTask(envelope, path.join(directory, 'wrong'), { ...context, taskId: '12346' }, key));
  await assert.rejects(openTask(envelope, path.join(directory, 'wrong'), { ...context, target: 'x86_64-apple-darwin' }, key));
  const tampered = fs.readFileSync(envelope); tampered[tampered.length - 1] ^= 1; fs.writeFileSync(envelope, tampered);
  await assert.rejects(openTask(envelope, path.join(directory, 'wrong'), context, key));
  assert.equal(fs.existsSync(path.join(directory, 'wrong')), false);
});
test('source snapshot bounds reject keys, generated binaries and escaping paths', () => {
  assert.equal(allowedSnapshotPath('tools/viewer-core/src/native_viewer.rs'), true);
  for (const name of ['../private.rs','tools/viewer-core/../../secret','tools/viewer-core/signing.pk8','tools/viewer-core/target/private.o','tools/viewer-core/module.dll','C:\\keys\\root.pem']) assert.equal(allowedSnapshotPath(name), false);
});
test('authenticated snapshot target/content changes fail closed', async (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'nova-ci-snapshot-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const names = ['tools/viewer-core/Cargo.toml','tools/nova-protected-module/Cargo.toml','tools/nova-protected-loader/Cargo.toml','desktop-nova/scripts/protect-rustc.py','desktop-nova/scripts/build-private-protected.ps1','desktop-nova/scripts/validate-private-snapshot.mjs'];
  const files = names.map((name) => {
    const file = path.join(directory, name), bytes = Buffer.from('fixture-only'); fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, bytes);
    return { path: name, bytes: bytes.length, sha256: crypto.createHash('sha256').update(bytes).digest('hex') };
  });
  const manifest = path.join(directory, 'snapshot.json'), target = 'aarch64-apple-darwin';
  fs.writeFileSync(manifest, JSON.stringify({ schema: 'nova.private-source-snapshot.v1', sourceSha: 'b'.repeat(40), target, files }));
  assert.equal((await validateSnapshot(directory, manifest, target)).files, 6);
  await assert.rejects(validateSnapshot(directory, manifest, 'x86_64-apple-darwin'));
  fs.writeFileSync(path.join(directory, names[0]), 'modified');
  await assert.rejects(validateSnapshot(directory, manifest, target));
});

test('task ZIP rejects links/traversal and returns only module, receipt and private diagnostics', (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'nova-ci-archive-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const program = String.raw`
import importlib.util,json,pathlib,stat,sys,zipfile
script,temporary=sys.argv[1:]
spec=importlib.util.spec_from_file_location('task_archive',script)
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
root=pathlib.Path(temporary)
for index,name in enumerate(['source/../bad','source/C:/bad','/source/bad','source/./bad','source/link']):
    archive=root/f'bad{index}.zip'
    with zipfile.ZipFile(archive,'w') as output:
        info=zipfile.ZipInfo(name)
        if index==4: info.external_attr=(stat.S_IFLNK|0o777)<<16
        output.writestr(info,b'fixture')
    try: module.extract(archive,root/f'out{index}','source')
    except ValueError: pass
    else: raise AssertionError('Unexpected archive path was accepted')
build=root/'build';(build/'backend').mkdir(parents=True)
(build/'backend/libnova_protected_module.dylib').write_bytes(b'fixture-module')
(build/'private-build.json').write_text('{}',encoding='utf8')
(build/'receipt.json').write_text('{}',encoding='utf8')
(build/'task-driver.log').write_text('fixture-log',encoding='utf8')
(build/'private.rs').write_text('not included',encoding='utf8')
request=root/'status.json';request.write_text(json.dumps({'exitCode':0}),encoding='utf8')
output=root/'result.zip';module.result(request,build,output)
with zipfile.ZipFile(output) as archive:
    assert set(archive.namelist())=={'result.json','result/private-build.json','result/receipt.json','result/task-driver.log','result/backend/libnova_protected_module.dylib'}
    assert json.loads(archive.read('result.json'))['internalSymbolsPresent'] is False
module.extract(output,root/'restored','result')
assert (root/'restored/result/backend/libnova_protected_module.dylib').read_bytes()==b'fixture-module'
print('PASS paths=5 result=module-receipt-diagnostics')
`;
  const result = spawnSync(process.platform === 'win32' ? 'py' : 'python3', [...(process.platform === 'win32' ? ['-3'] : []), '-c', program, fileURLToPath(new URL('./ci-task-archive.py', import.meta.url)), directory], { encoding: 'utf8', windowsHide: true });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /PASS paths=5 result=module-receipt-diagnostics/);
});
