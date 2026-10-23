import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

// The adapter inputs below are existing generated, versioned facts. Extraction,
// validation, normalization and package identity belong to the public importer.
export function prepareDefaultGameData(core, options = {}) {
  const root = options.root || repository;
  const version = options.gameVersion || '26.3';
  if (!/^[A-Za-z0-9_.-]{1,100}$/.test(version)) throw new Error('Invalid game version.');
  const output = path.resolve(options.output || path.join(root, 'pack-in/game-data/default'));
  const manifest = path.join(output, 'manifest.json');
  if (!options.force && !options.jar && !options.assets && fs.existsSync(manifest)
      && JSON.parse(fs.readFileSync(manifest, 'utf8')).game_version === version) {
    console.log('Default game data: reusing the existing versioned package (independent of AppVersion).');
    return output;
  }
  const jar = path.resolve(options.jar || path.join(root, 'data/mc26', version, 'client.jar'));
  if (!fs.existsSync(jar)) {
    if (options.optional) {
      console.log('No default game data package in this checkout. Import a client JAR in Resources before using game resources.');
      return null;
    }
    throw new Error('The default data source is missing. Restore the bundled resource archive or specify --jar; no local Minecraft installation is required.');
  }
  const adapter = path.join(root, '.tmp/game-data', `default-adapter-${version}`);
  fs.mkdirSync(adapter, { recursive: true });
  fs.writeFileSync(path.join(adapter, 'adapter.json'), JSON.stringify({ game_version: version }));
  for (const [source, destination] of [
    [`pack-in/materials/minecraft-${version}.json`, 'inventory.json'],
    ...(version === '26.3' ? [['pack-in/lang/zh_cn.json', 'lang/zh_cn.json'], ['pack-in/lang/en_us.json', 'lang/en_us.json']] : []),
    ['data/blockdb', 'blockdb'],
    [`data/mc26/${version}/oracle`, 'oracle'],
    [`pack-in/item-icons-${version}`, 'item-icons'],
    ...(version === '26.3' ? [['pack-in/game-icons/block.zip', 'block-icons.zip']] : []),
  ]) {
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
  const assets = options.assets || path.join(root, 'data/mc26', version, 'assets');
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
