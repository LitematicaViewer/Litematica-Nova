import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { Readable, Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { spawnSync } from 'node:child_process';

const target = process.argv[process.argv.indexOf('--target') + 1];
if (!['x86_64-apple-darwin', 'aarch64-apple-darwin'].includes(target)) throw new Error('This download entry accepts a macOS target only.');
const urlText = process.env.NOVA_RUNTIME_URL, expected = process.env.NOVA_RUNTIME_SHA256;
if (!urlText || !/^[a-f0-9]{64}$/.test(expected ?? '')) throw new Error(`The approved runtime package URL/SHA256 is not configured for ${target}.`);
const url = new URL(urlText);
if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash) throw new Error('Runtime downloads require an ordinary configured HTTPS URL.');
const temporary = fs.mkdtempSync(path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'nova-runtime-'));
const archive = path.join(temporary, 'runtime.zip'), output = path.join(temporary, 'package');
const response = await fetch(url, { signal: AbortSignal.timeout(180_000), redirect: 'error' });
if (!response.ok || !response.body) throw new Error(`Runtime download failed with HTTP ${response.status}.`);
let bytes = 0;
const hash = crypto.createHash('sha256');
await pipeline(Readable.fromWeb(response.body), new Transform({
  transform(chunk, encoding, callback) {
    bytes += chunk.length;
    if (bytes > 768 * 1024 ** 2) return callback(new Error('Runtime package exceeds its download limit.'));
    hash.update(chunk);
    callback(null, chunk);
  },
}), fs.createWriteStream(archive, { flags: 'wx', mode: 0o600 }));
if (hash.digest('hex') !== expected) throw new Error('Runtime package SHA256 differs from the approved value.');
const extract = String.raw`
import json, os, stat, sys, zipfile
archive, output = sys.argv[1:]
with zipfile.ZipFile(archive) as z:
    entries = z.infolist()
    if len(entries) != 3 or sum(e.file_size for e in entries) > 768 * 1024**2:
        raise ValueError('Unexpected runtime package contents or size')
    names = [e.filename for e in entries]
    if any('/' in n or '\\' in n or n in ('.', '..') for n in names) or len(set(names)) != 3:
        raise ValueError('Runtime package must contain three flat files')
    for e in entries:
        if e.is_dir() or stat.S_ISLNK(e.external_attr >> 16):
            raise ValueError('Runtime package links/directories are not permitted')
    if 'protected-build.json' not in names or z.getinfo('protected-build.json').file_size > 16384:
        raise ValueError('Runtime package identity is missing or too large')
    identity = json.loads(z.read('protected-build.json'))
    fields = {'schema','version','buildId','moduleId','manifestPublicKey','endpoint','manifestFile','ciphertextFile'}
    if set(identity) != fields or identity['schema'] != 'nova.protected-build.v1':
        raise ValueError('Unexpected runtime identity fields')
    module = identity['moduleId']
    if not isinstance(module,str) or not module.endswith('.dylib') or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-' for c in module):
        raise ValueError('Runtime module does not match macOS')
    if identity['manifestFile'] != module+'.manifest.json' or identity['ciphertextFile'] != module+'.enc':
        raise ValueError('Runtime identity paths do not match')
    if set(names) != {'protected-build.json',module+'.manifest.json',module+'.enc'}:
        raise ValueError('Runtime package contains unexpected files')
    if z.getinfo(module+'.manifest.json').file_size > 131072:
        raise ValueError('Runtime manifest exceeds its size limit')
    os.makedirs(output,mode=0o700)
    for e in entries:
        with z.open(e) as src, open(os.path.join(output,e.filename),'xb') as dst:
            while True:
                chunk=src.read(1024*1024)
                if not chunk: break
                dst.write(chunk)
`;
const result = spawnSync('python3', ['-c', extract, archive, output], { stdio: 'inherit' });
if (result.status !== 0) throw new Error('Runtime archive extraction failed.');
fs.unlinkSync(archive);
if (!process.env.GITHUB_ENV) throw new Error('The Actions environment output is missing.');
fs.appendFileSync(process.env.GITHUB_ENV, `NOVA_RUNTIME_PACKAGE_DIR=${output}\n`);
console.log(`Runtime package downloaded: target=${target} bytes=${bytes}`);
