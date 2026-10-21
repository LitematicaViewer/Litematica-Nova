import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

export function allowedSnapshotPath(name) {
  if (typeof name !== 'string' || name.startsWith('/') || name.includes('\\') || name.split('/').some((part) => !part || part === '.' || part === '..' || ['.git','target','node_modules'].includes(part))) return false;
  if (/\.(?:key|pem|pk8|p12|pfx|dpapi|env|exe|dll|dylib|so|pdb|o|obj|rlib|rmeta|bc|ll|log)$/i.test(name)) return false;
  return /^(?:tools\/(?:viewer-core|nova-protected-module|nova-protected-loader|nova-protection|nova-light-wire)\/|third_party\/wgpu-hal-patched\/|data\/blockdb\/)/.test(name)
    || /^(?:pack-in\/materials\/minecraft-26\.3\.json|pack-in\/lang\/zh_cn\.json|desktop-nova\/src-tauri\/(?:tauri\.conf\.json|icons\/icon\.(?:png|ico))|desktop-nova\/scripts\/(?:build-private-protected\.ps1|protect-rustc\.py|protect-rustc-launcher\.rs|validate-private-snapshot\.mjs))$/.test(name);
}
export async function validateSnapshot(repo, manifestFile, target) {
  repo = fs.realpathSync(repo);
  const stat = fs.lstatSync(manifestFile);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 8 * 1024 ** 2) throw new Error('Invalid source snapshot manifest.');
  const manifest = JSON.parse(fs.readFileSync(manifestFile, 'utf8'));
  if (manifest.schema !== 'nova.private-source-snapshot.v1' || !/^[a-f0-9]{40}$/.test(manifest.sourceSha) ||
      manifest.target !== target || !['x86_64-apple-darwin','aarch64-apple-darwin','x86_64-unknown-linux-gnu'].includes(target) ||
      !Array.isArray(manifest.files) || !manifest.files.length || manifest.files.length > 20_000) throw new Error('Source snapshot identity differs from the task.');
  const names = new Set(); let total = 0;
  for (const file of manifest.files) {
    if (!allowedSnapshotPath(file.path) || names.has(file.path) || !/^[a-f0-9]{64}$/.test(file.sha256) || !Number.isSafeInteger(file.bytes) || file.bytes < 0 || file.bytes > 64 * 1024 ** 2) throw new Error('Unexpected source snapshot entry.');
    names.add(file.path); total += file.bytes;
    if (total > 512 * 1024 ** 2) throw new Error('Source snapshot exceeds its size limit.');
    let current = repo;
    for (const part of file.path.split('/')) { current = path.join(current, part); if (fs.lstatSync(current).isSymbolicLink()) throw new Error('Source snapshot links are not permitted.'); }
    const entry = fs.lstatSync(current), relative = path.relative(repo, fs.realpathSync(current));
    if (!entry.isFile() || entry.size !== file.bytes || relative.startsWith('..') || path.isAbsolute(relative)) throw new Error('Source snapshot file changed or leaves the task directory.');
    const digest = crypto.createHash('sha256'); for await (const bytes of fs.createReadStream(current)) digest.update(bytes);
    if (digest.digest('hex') !== file.sha256) throw new Error('Source snapshot content differs from the authenticated task.');
  }
  for (const required of ['tools/viewer-core/Cargo.toml','tools/nova-protected-module/Cargo.toml','tools/nova-protected-loader/Cargo.toml','desktop-nova/scripts/protect-rustc.py','desktop-nova/scripts/build-private-protected.ps1','desktop-nova/scripts/validate-private-snapshot.mjs']) if (!names.has(required)) throw new Error('The source snapshot is incomplete.');
  return { sourceSha: manifest.sourceSha, target: manifest.target, files: names.size };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const arg = (name) => process.argv[process.argv.indexOf(name) + 1];
  validateSnapshot(arg('--repo'), arg('--manifest'), arg('--target')).then((value) => console.log(JSON.stringify(value)))
    .catch(() => { console.error('Source snapshot validation failed.'); process.exitCode = 1; });
}
