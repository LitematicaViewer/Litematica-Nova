import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const arg = (key) => process.argv[process.argv.indexOf(key) + 1];
const target = arg('--target'), sha = arg('--sha');
const patterns = {
  'x86_64-pc-windows-msvc': /(?:setup\.exe|\.exe\.sig)$/i,
  'x86_64-unknown-linux-gnu': /\.(?:deb|AppImage)(?:\.sig)?$/,
  'x86_64-apple-darwin': /(?:\.dmg|\.app\.tar\.gz)(?:\.sig)?$/,
  'aarch64-apple-darwin': /(?:\.dmg|\.app\.tar\.gz)(?:\.sig)?$/,
};
if (!patterns[target] || !/^[a-f0-9]{40}$/.test(sha ?? '')) throw new Error('Expected an exact supported target and source SHA.');
const directory = path.join(root, 'dist', 'ci', target);
const manifestPath = path.join(directory, 'artifacts.json');
if (fs.statSync(manifestPath).size > 65536) throw new Error('Artifact manifest is too large.');
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
if (manifest.schema !== 'nova.ci.artifacts.v1' || manifest.target !== target || manifest.sourceSha !== sha ||
    !Array.isArray(manifest.files) || !manifest.files.length || manifest.files.length > 12) throw new Error('Candidate artifact identity or file list is invalid.');
const destination = path.join(root, 'dist', 'ci-upload', target);
fs.mkdirSync(destination, { recursive: true });
const paths = [], names = new Set();
for (const entry of manifest.files) {
  const source = path.resolve(root, entry.path), name = path.basename(source);
  const relative = path.relative(directory, source);
  if (relative.startsWith('..') || path.isAbsolute(relative) || !patterns[target].test(name) || names.has(name)) throw new Error('Only explicit final distributables inside this target directory may be uploaded.');
  names.add(name);
  const stat = fs.lstatSync(source);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size !== entry.bytes || stat.size < 1 || stat.size > 2 * 1024 ** 3 || !/^[a-f0-9]{64}$/.test(entry.sha256)) throw new Error(`Invalid final artifact metadata: ${name}`);
  const hash = crypto.createHash('sha256');
  for await (const bytes of fs.createReadStream(source)) hash.update(bytes);
  if (hash.digest('hex') !== entry.sha256) throw new Error(`Final artifact hash mismatch: ${name}`);
  const staged = path.join(destination, name);
  fs.copyFileSync(source, staged);
  paths.push(staged);
}
if (!paths.some((file) => !file.endsWith('.sig'))) throw new Error('No installable or runnable candidate is present.');
if (process.env.GITHUB_OUTPUT) {
  const delimiter = `NOVA_FILES_${crypto.randomBytes(12).toString('hex')}`;
  fs.appendFileSync(process.env.GITHUB_OUTPUT, `paths<<${delimiter}\n${paths.join('\n')}\n${delimiter}\n`);
}
console.log(`Final distributables ready: target=${target} source=${sha} files=${paths.length}`);
