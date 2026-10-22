import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { Readable, Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { moduleForTarget, validatePackage, validateDevelopmentPackage } from './check-public-checkout.mjs';

const targets = ['x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'x86_64-apple-darwin', 'aarch64-apple-darwin'];
const settingsRoot = () => process.platform === 'win32' ? path.join(process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData/Local'), 'Litematica-Nova') : path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'), 'Litematica-Nova');
const cacheRoot = () => process.platform === 'win32' ? path.join(settingsRoot(), 'runtime-cache') : path.join(process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache'), 'Litematica-Nova/runtime');
const defaultIndex = 'https://45-62-112-121.sslip.io:8448/nova-development/index.json';
const requestHeaders = () => {
  const token = process.env.NOVA_DEVELOPMENT_READ_TOKEN || process.env.NOVA_RUNTIME_DOWNLOAD_TOKEN;
  return token ? { Authorization: `Bearer ${token}` } : {};
};
function responseError(status) {
  const error = new Error(status === 401 || status === 403 ? 'NOVA_RUNTIME_DOWNLOAD_AUTH_REQUIRED: the read-only development runtime credential is missing or expired.' : `Runtime download failed with HTTP ${status}.`);
  if (status === 401 || status === 403) error.code = 'NOVA_RUNTIME_DOWNLOAD_AUTH_REQUIRED';
  return error;
}
async function configuredDownload(target) {
  if (process.env.NOVA_RUNTIME_URL || process.env.NOVA_RUNTIME_SHA256) return { url: process.env.NOVA_RUNTIME_URL, sha256: process.env.NOVA_RUNTIME_SHA256 };
  const filename = path.join(settingsRoot(), 'development-runtime.json');
  if (fs.existsSync(filename)) {
    const stat = fs.lstatSync(filename);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 65536) throw new Error('The local development runtime configuration is invalid.');
    return JSON.parse(fs.readFileSync(filename, 'utf8')).runtimes?.[target];
  }
  const indexUrl = downloadUrl(process.env.NOVA_DEVELOPMENT_RUNTIME_INDEX || defaultIndex);
  const response = await fetch(indexUrl, { headers: requestHeaders(), redirect: 'error', signal: AbortSignal.timeout(30_000) });
  if (!response.ok || !response.body) throw responseError(response.status);
  let text = '';
  for await (const bytes of Readable.fromWeb(response.body)) {
    text += bytes.toString('utf8');
    if (Buffer.byteLength(text) > 65536) throw new Error('The runtime channel index exceeds its size limit.');
  }
  const index = JSON.parse(text);
  if (index.schema !== 'nova.development-runtime-index.v1') throw new Error('The development runtime channel index is invalid.');
  const record = index.runtimes?.[target];
  if (record && downloadUrl(record.url).origin !== indexUrl.origin) throw new Error('The runtime channel cannot forward its read-only credential to another HTTPS origin.');
  return record ? { url: record.url, sha256: record.archive_sha256 } : null;
}
function downloadUrl(text) {
  const url = new URL(text);
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash) throw new Error('Runtime downloads require an ordinary configured HTTPS URL.');
  return url;
}
function extractArchive(archive, output) {
  // Use the platform's existing ZIP implementation. Only four flat, bounded
  // files are allowed; validate all entries before the first file is extracted.
  const python = String.raw`
import os,stat,sys,zipfile
archive,output=sys.argv[1:]
with zipfile.ZipFile(archive) as z:
    entries=z.infolist(); names=[e.filename for e in entries]
    if len(entries)!=4 or len(set(names))!=4 or sum(e.file_size for e in entries)>768*1024**2: raise ValueError('Unexpected runtime package contents or size')
    if any('/' in n or '\\' in n or n in ('.','..') for n in names): raise ValueError('Runtime package must contain four flat files')
    if any(e.is_dir() or stat.S_ISLNK(e.external_attr>>16) or e.flag_bits&1 for e in entries): raise ValueError('Runtime package links/encrypted ZIP entries are not permitted')
    if not {'protected-build.json','runtime-compatibility.json'}.issubset(names): raise ValueError('Runtime identity/compatibility is missing')
    if any(e.file_size>131072 for e in entries if not e.filename.endswith('.enc')): raise ValueError('Runtime metadata exceeds its size limit')
    os.makedirs(output,mode=0o700)
    for e in entries:
        with z.open(e) as src,open(os.path.join(output,e.filename),'xb') as dst:
            while True:
                chunk=src.read(1024*1024)
                if not chunk: break
                dst.write(chunk)
`;
  const powershell = String.raw`
$ErrorActionPreference = 'Stop'
$archive = [IO.Compression.ZipFile]::OpenRead($env:NOVA_RUNTIME_ARCHIVE)
try {
  $names = @($archive.Entries | ForEach-Object FullName)
  if ($names.Count -ne 4 -or @($names | Select-Object -Unique).Count -ne 4 -or ($archive.Entries | Measure-Object Length -Sum).Sum -gt 768MB) { throw 'Unexpected runtime package contents or size' }
  foreach ($entry in $archive.Entries) {
    if ($entry.FullName -match '[/\\]' -or $entry.FullName -in @('.','..') -or (($entry.ExternalAttributes -shr 16) -band 61440) -eq 40960 -or ($entry.Name -notlike '*.enc' -and $entry.Length -gt 128KB)) { throw 'Invalid runtime archive entry' }
  }
  if ('protected-build.json' -notin $names -or 'runtime-compatibility.json' -notin $names) { throw 'Runtime identity/compatibility is missing' }
  [IO.Directory]::CreateDirectory($env:NOVA_RUNTIME_EXTRACT) | Out-Null
  foreach ($entry in $archive.Entries) { [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $env:NOVA_RUNTIME_EXTRACT $entry.Name), $false) }
} finally { $archive.Dispose() }
`;
  const result = process.platform === 'win32'
    ? spawnSync('pwsh', ['-NoProfile', '-Command', powershell], { stdio: 'inherit', windowsHide: true, env: { ...process.env, NOVA_RUNTIME_ARCHIVE: archive, NOVA_RUNTIME_EXTRACT: output } })
    : spawnSync('python3', ['-c', python, archive, output], { stdio: 'inherit' });
  if (result.error || result.status !== 0) throw new Error('Runtime archive extraction failed.');
}
export async function fetchRuntime(target, source, { release = false } = {}) {
  if (!targets.includes(target) || !source?.url || !/^[a-f0-9]{64}$/.test(source.sha256 ?? '')) throw new Error(`An approved read-only runtime package URL/SHA256 is not configured for ${target}.`);
  const url = downloadUrl(source.url), destination = path.join(cacheRoot(), target, source.sha256);
  if (fs.existsSync(destination)) {
    await validatePackage(destination, moduleForTarget(target), { target, release });
    return destination;
  }
  fs.mkdirSync(path.dirname(destination), { recursive: true, mode: 0o700 });
  const temporary = fs.mkdtempSync(path.join(path.dirname(destination), '.download-'));
  const archive = path.join(temporary, 'runtime.zip'), output = path.join(temporary, 'package');
  try {
    const response = await fetch(url, { headers: requestHeaders(), signal: AbortSignal.timeout(180_000), redirect: 'error' });
    if (!response.ok || !response.body) throw responseError(response.status);
    let bytes = 0;
    const hash = crypto.createHash('sha256');
    await pipeline(Readable.fromWeb(response.body), new Transform({ transform(chunk, encoding, callback) {
      bytes += chunk.length;
      if (bytes > 768 * 1024 ** 2) return callback(new Error('Runtime package exceeds its download limit.'));
      hash.update(chunk); callback(null, chunk);
    } }), fs.createWriteStream(archive, { flags: 'wx', mode: 0o600 }));
    if (hash.digest('hex') !== source.sha256) throw new Error('Runtime package SHA256 differs from the approved value.');
    extractArchive(archive, output);
    await validatePackage(output, moduleForTarget(target), { target, release });
    // A concurrent development launch may have completed the same download.
    if (!fs.existsSync(destination)) fs.renameSync(output, destination);
    await validatePackage(destination, moduleForTarget(target), { target, release });
    console.error(`Signed runtime package ready: target=${target} bytes=${bytes}`);
    return destination;
  } finally { fs.rmSync(temporary, { recursive: true, force: true }); }
}
export async function ensureDevelopmentRuntime(repo, target, selected) {
  if (selected) {
    await validatePackage(selected, moduleForTarget(target), { target });
    return selected;
  }
  const staged = path.join(repo, 'bin/viewer-backend/protected');
  let existingError;
  if (fs.existsSync(staged)) {
    try { await validatePackage(staged, moduleForTarget(target), { target }); return staged; }
    catch (error) { existingError = error; }
  }
  const source = await configuredDownload(target);
  if (!source) throw new Error(`No compatible signed development runtime is available for ${target}. Ask a build administrator for the read-only runtime channel (NOVA_RUNTIME_URL/NOVA_RUNTIME_SHA256 or ${path.join(settingsRoot(), 'development-runtime.json')}).${existingError ? ` Existing package: ${existingError.message}` : ''}`);
  return fetchRuntime(target, source);
}
export async function acquireLocalDevelopmentRuntime(repo, target, { privateSourceDir, checkOnly = false, reuseExisting = false } = {}) {
  if (!targets.includes(target) || !privateSourceDir) throw new Error('Select an authorized private source checkout with --private-source or NOVA_PRIVATE_SOURCE_DIR for the local runtime route.');
  const privateRoot = fs.realpathSync(path.resolve(privateSourceDir));
  const builder = path.join(privateRoot, 'desktop-nova/scripts/build-development-runtime.ps1');
  if (!fs.existsSync(builder) || !fs.lstatSync(builder).isFile()) throw new Error('The selected private checkout is missing its authoritative Development Runtime builder.');
  const result = spawnSync('pwsh', ['-NoProfile', '-File', builder, '-RepoRoot', privateRoot, '-PublicRepoRoot', path.resolve(repo), '-TargetTriple', target, ...(checkOnly ? ['-CheckOnly'] : []), ...(reuseExisting ? ['-ReuseExisting'] : [])], { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024, windowsHide: true });
  const lines = String(result.stdout || '').split(/\r?\n/);
  const markers = lines.filter(line => line.startsWith('NOVA_DEVELOPMENT_RUNTIME='));
  const logs = lines.filter(line => line && !line.startsWith('NOVA_DEVELOPMENT_RUNTIME=')).join('\n');
  if (logs) process.stderr.write(logs + '\n');
  if (result.stderr) process.stderr.write(result.stderr);
  if (result.error || result.status !== 0 || markers.length !== 1) throw new Error('The private Development Runtime build/reuse failed; no production key or component fallback is used.');
  const directory = fs.realpathSync(markers[0].slice('NOVA_DEVELOPMENT_RUNTIME='.length).trim());
  await validateDevelopmentPackage(directory, moduleForTarget(target), { target });
  return directory;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2), target = args[args.indexOf('--target') + 1];
  configuredDownload(target).then(source => fetchRuntime(target, source, { release: !args.includes('--development') })).then(output => {
    if (process.env.GITHUB_ENV) fs.appendFileSync(process.env.GITHUB_ENV, `NOVA_RUNTIME_PACKAGE_DIR=${output}\n`);
    else console.log(output);
  }).catch(error => { console.error(error.message); process.exitCode = error.code === 'NOVA_RUNTIME_DOWNLOAD_AUTH_REQUIRED' ? 77 : 1; });
}
