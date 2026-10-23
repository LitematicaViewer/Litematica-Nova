import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import zlib from 'node:zlib';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

function sourceRecords(input, prefix, records) {
  if (!fs.existsSync(input)) return;
  const stat = fs.lstatSync(input);
  if (stat.isSymbolicLink()) throw new Error(`Game-data adapter inputs cannot be symlinks: ${input}`);
  if (stat.isDirectory()) {
    for (const name of fs.readdirSync(input).sort()) sourceRecords(path.join(input, name), `${prefix}/${name}`, records);
  } else if (stat.isFile()) {
    const hash = createHash('sha256');
    const fd = fs.openSync(input, 'r');
    const buffer = Buffer.allocUnsafe(1024 * 1024);
    try { for (let count; (count = fs.readSync(fd, buffer, 0, buffer.length, null)) > 0;) hash.update(buffer.subarray(0, count)); }
    finally { fs.closeSync(fd); }
    records.push([prefix, stat.size, hash.digest('hex')]);
  }
}

// The adapter inputs below are existing generated, versioned facts. Extraction,
// validation, normalization and package identity belong to the public importer.
export function prepareDefaultGameData(core, options = {}) {
  const root = options.root || repository;
  const version = options.gameVersion || '26.3';
  if (!/^[A-Za-z0-9_.-]{1,100}$/.test(version)) throw new Error('Invalid game version.');
  const output = path.resolve(options.output || path.join(root, 'pack-in/game-data/default'));
  const manifest = path.join(output, 'manifest.json');
  const jar = path.resolve(options.jar || path.join(root, 'data/mc26', version, 'client.jar'));
  if (!fs.existsSync(jar)) {
    if (options.optional) {
      console.log('No default game data package in this checkout. Import a client JAR in Resources before using game resources.');
      return null;
    }
    throw new Error('The default data source is missing. Restore the bundled resource archive or specify --jar; no local Minecraft installation is required.');
  }
  const inputs = [
    [`pack-in/materials/minecraft-${version}.json`, 'inventory.json'],
    ...(version === '26.3' ? [['pack-in/lang/zh_cn.json', 'lang/zh_cn.json'], ['pack-in/lang/en_us.json', 'lang/en_us.json']] : []),
    ['data/blockdb', 'blockdb'],
    [`data/mc26/${version}/oracle`, 'oracle'],
    [`pack-in/item-icons-${version}`, 'item-icons'],
    ...(version === '26.3' ? [['pack-in/game-icons/block.zip', 'block-icons.zip']] : []),
  ];
  const versionsRoot = path.join(root, 'data/game-data/version-adapters');
  if (fs.existsSync(versionsRoot)) {
    const entries = fs.readdirSync(versionsRoot, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name));
    const archives = new Set(entries.filter(entry => entry.isFile() && entry.name.endsWith('.zip')).map(entry => entry.name.slice(0, -4)));
    for (const entry of entries) {
      if (entry.isFile() && entry.name.endsWith('.zip')) {
        if (!/^[A-Za-z0-9_.-]{1,100}$/.test(entry.name.slice(0, -4))) throw new Error('Invalid version adapter archive.');
        // Keep other versions compressed. The importer opens only the selected
        // archive and verifies its version and file-level content identity.
        inputs.push([path.relative(root, path.join(versionsRoot, entry.name)), `version-adapters/${entry.name}`]);
        continue;
      }
      if (!entry.isDirectory() || !/^[A-Za-z0-9_.-]{1,100}$/.test(entry.name)) throw new Error('Invalid version adapter directory.');
      if (archives.has(entry.name)) continue;
      const directory = path.join(versionsRoot, entry.name);
      const identity = JSON.parse(fs.readFileSync(path.join(directory, 'adapter.json'), 'utf8'));
      if (identity.game_version !== entry.name || identity.schema_version !== 1 || !identity.content_id || !identity.files) {
        throw new Error(`Version adapter is missing its exact content identity: ${entry.name}`);
      }
      inputs.push([path.relative(root, directory), `version-adapters/${entry.name}`]);
    }
  }
  const assets = options.assets || path.join(root, 'data/mc26', version, 'assets');
  const records = [];
  sourceRecords(jar, 'source.jar', records);
  sourceRecords(assets, 'assets', records);
  sourceRecords(fileURLToPath(import.meta.url), 'prepare-script', records);
  sourceRecords(path.join(root, 'tools/viewer-core/src/game_data.rs'), 'importer', records);
  for (const [source, destination] of inputs) sourceRecords(path.join(root, source), destination, records);
  const inputId = createHash('sha256').update(JSON.stringify([1, version, records])).digest('hex');
  const marker = path.join(output, 'derived/prepare-inputs.json');
  if (!options.force && fs.existsSync(manifest) && fs.existsSync(marker)
      && JSON.parse(fs.readFileSync(manifest, 'utf8')).game_version === version
      && JSON.parse(fs.readFileSync(marker, 'utf8')).source_input_identity === inputId) {
    console.log('Default game data: reusing the matching source/adapter identity (independent of AppVersion).');
    return output;
  }
  const stagingRoot = path.resolve(root, '.tmp/game-data');
  const adapter = path.resolve(stagingRoot, `default-adapter-${version}`);
  if (path.dirname(adapter) !== stagingRoot) throw new Error('Adapter staging path leaves its build directory.');
  // Only this generated staging directory is replaced: removed source tables
  // must not survive a later build with the same Minecraft version.
  fs.rmSync(adapter, { recursive: true, force: true });
  fs.mkdirSync(adapter, { recursive: true });
  fs.writeFileSync(path.join(adapter, 'adapter.json'), JSON.stringify({ game_version: version }));
  fs.writeFileSync(path.join(adapter, 'prepare-inputs.json'), JSON.stringify({ schema_version: 1, source_input_identity: inputId }));
  for (const [source, destination] of inputs) {
    const input = path.join(root, source);
    if (fs.existsSync(input)) fs.cpSync(input, path.join(adapter, destination), { recursive: true });
  }
  const blocksFile = path.join(adapter, 'blockdb/blocks.dat.z');
  if (fs.existsSync(blocksFile)) {
    const blocks = zlib.inflateSync(fs.readFileSync(blocksFile));
    const marker = blocks.lastIndexOf(Buffer.from('NOVAST04'));
    if (marker < 0) throw new Error('The existing version registry has no canonical adapter supplement.');
    const length = blocks.readUInt32LE(marker + 8);
    const supplement = JSON.parse(zlib.inflateSync(blocks.subarray(marker + 12, marker + 12 + length)));
    fs.writeFileSync(path.join(adapter, 'blockdb/versions.json'), JSON.stringify(Object.keys(supplement.versions).sort()));
  }
  const args = ['game-data', 'build-default', '--jar', jar, '--output', output, '--game-version', version, '--adapter-dir', adapter];
  if (fs.existsSync(assets)) args.push('--assets-dir', path.resolve(assets));
  const result = spawnSync(core, args, { cwd: root, stdio: 'inherit', windowsHide: true });
  if (result.error || result.status !== 0) throw new Error(`Default game data import failed: ${result.error?.message || result.status}`);
  if (!fs.existsSync(manifest)) throw new Error('The common importer did not produce a complete default package.');
  return output;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const value = name => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
  try {
    const core = path.resolve(value('--core') || path.join(repository, 'bin/public-beta', process.platform === 'win32' ? 'litematica_core.exe' : 'litematica_core'));
    prepareDefaultGameData(core, { gameVersion: value('--game-version'), jar: value('--jar'), assets: value('--assets-dir'), output: value('--output'), force: args.includes('--force'), optional: args.includes('--optional') });
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
