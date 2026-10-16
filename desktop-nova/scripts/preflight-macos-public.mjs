import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { thumbnailTarget, moltenVkLibrary, buildMacosThumbnail } from './build-macos-thumbnail.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const version = value => `${value >>> 16}.${(value >>> 8) & 255}.${value & 255}`;
export function compareVersions(left, right) {
  const a = left.split('.').map(Number), b = right.split('.').map(Number);
  for (let i = 0; i < 3; i++) if ((a[i] || 0) !== (b[i] || 0)) return (a[i] || 0) - (b[i] || 0);
  return 0;
}

// Read only public Mach-O load commands; no symbol/source or protected payload
// extraction is involved. This also works on Windows for returned build metadata.
export function inspectMachO(bytes) {
  const thin = slice => {
    if (slice.length < 32 || slice.readUInt32LE(0) !== 0xfeedfacf) throw new Error('Expected a real little-endian 64-bit Mach-O binary.');
    const cpu = slice.readUInt32LE(4), architecture = cpu === 0x0100000c ? 'arm64' : cpu === 0x01000007 ? 'x86_64' : `cpu-${cpu}`;
    const count = slice.readUInt32LE(16), end = 32 + slice.readUInt32LE(20);
    if (end > slice.length || count > 65536) throw new Error('Invalid Mach-O load command bounds.');
    const result = { architecture, fileType: slice.readUInt32LE(12), minimumSystemVersion: null, sdk: null, dependencies: [], rpaths: [] };
    let offset = 32;
    for (let i = 0; i < count; i++) {
      if (offset + 8 > end) throw new Error('Truncated Mach-O load command.');
      const cmd = slice.readUInt32LE(offset), size = slice.readUInt32LE(offset + 4);
      if (size < 8 || offset + size > end) throw new Error('Invalid Mach-O load command size.');
      const name = () => {
        if (size < 12) throw new Error('Invalid Mach-O path command.');
        const start = offset + slice.readUInt32LE(offset + 8), nul = slice.indexOf(0, start);
        if (start < offset + 12 || start >= offset + size || nul < 0 || nul >= offset + size) throw new Error('Invalid Mach-O path bounds.');
        return slice.toString('utf8', start, nul);
      };
      if (cmd === 0x32) {
        if (size < 24 || slice.readUInt32LE(offset + 8) !== 1) throw new Error('Expected a macOS LC_BUILD_VERSION.');
        result.minimumSystemVersion = version(slice.readUInt32LE(offset + 12));
        result.sdk = version(slice.readUInt32LE(offset + 16));
      } else if (cmd === 0x24) {
        if (size < 16) throw new Error('Invalid LC_VERSION_MIN_MACOSX.');
        result.minimumSystemVersion = version(slice.readUInt32LE(offset + 8));
        result.sdk = version(slice.readUInt32LE(offset + 12));
      } else if ([0xc, 0x80000018, 0x8000001f, 0x80000023].includes(cmd)) result.dependencies.push(name());
      else if (cmd === 0xd) result.installName = name();
      else if (cmd === 0x8000001c) result.rpaths.push(name());
      offset += size;
    }
    if (offset !== end) throw new Error('Inconsistent Mach-O command count.');
    return result;
  };
  if (bytes.length < 8) throw new Error('Truncated Mach-O file.');
  const magic = bytes.readUInt32BE(0);
  if (![0xcafebabe, 0xcafebabf].includes(magic)) return [thin(bytes)];
  const count = bytes.readUInt32BE(4), stride = magic === 0xcafebabf ? 32 : 20;
  if (!count || count > 16 || 8 + count * stride > bytes.length) throw new Error('Invalid universal Mach-O table.');
  return Array.from({ length: count }, (_, i) => {
    const at = 8 + i * stride;
    const offset = stride === 32 ? Number(bytes.readBigUInt64BE(at + 8)) : bytes.readUInt32BE(at + 8);
    const size = stride === 32 ? Number(bytes.readBigUInt64BE(at + 16)) : bytes.readUInt32BE(at + 12);
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(size) || offset < 8 + count * stride || size < 32 || offset + size > bytes.length) throw new Error('Invalid universal Mach-O slice bounds.');
    const result = thin(bytes.subarray(offset, offset + size));
    if (bytes.readUInt32BE(at) !== bytes.readUInt32LE(offset + 4)) throw new Error('Universal Mach-O architecture table differs from its slice.');
    return result;
  });
}

export function validateMacLibrary(slices, architecture, minimum) {
  const selected = slices.find(slice => slice.architecture === architecture);
  if (!selected || selected.fileType !== 6) throw new Error(`MoltenVK lacks a real ${architecture} Mach-O dynamic library.`);
  if (!selected.minimumSystemVersion || compareVersions(selected.minimumSystemVersion, minimum) > 0) throw new Error(`MoltenVK ${architecture} requires macOS ${selected.minimumSystemVersion || 'unknown'}, above configured ${minimum}. Use a matching real library or explicitly raise the application minimum.`);
  const external = selected.dependencies.filter(name => !name.startsWith('/System/Library/') && !name.startsWith('/usr/lib/'));
  if (external.length) throw new Error(`MoltenVK has unbundled non-system dependencies: ${external.join(', ')}`);
  return selected;
}

export function preflightMacosPublic({ target, output = path.join(root, '.tmp/macos-preflight', target || 'unknown', 'report.json') } = {}) {
  if (process.platform !== 'darwin') throw new Error('macOS public preflight requires a real native macOS runner.');
  const { arch, minimum } = thumbnailTarget({ TAURI_ENV_TARGET_TRIPLE: target });
  if (arch !== (process.arch === 'arm64' ? 'arm64' : 'x86_64')) throw new Error('macOS application preflight must run natively for its target.');
  const report = { schema: 'nova.macos.public-preflight.v1', target, minimumSystemVersion: minimum, stages: [], graphicsRuntime: 'not_verified', notarization: 'not_performed' };
  const stage = (name, operation) => {
    try { const details = operation(); report.stages.push({ name, status: 'passed', ...(details ? { details } : {}) }); console.log(`[nova-macos-preflight] stage=${name} status=passed`); }
    catch (error) { report.stages.push({ name, status: 'failed', error: error.message }); console.error(`[nova-macos-preflight] stage=${name} status=failed ${error.message}`); }
  };
  const command = (program, args) => {
    const result = spawnSync(program, args, { cwd: root, env: process.env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    if (result.error || result.status !== 0) throw new Error(`${program} ${args[0]} failed: ${(result.stderr || result.error?.message || '').trim().slice(-2000)}`);
    return result.stdout.trim();
  };
  stage('native-target-and-sdk', () => {
    const host = command('rustc', ['-vV']).split('\n').find(line => line.startsWith('host: '));
    if (host !== `host: ${target}`) throw new Error('Active Rust host differs from the native application target.');
    const config = JSON.parse(fs.readFileSync(path.join(root, 'desktop-nova/src-tauri/tauri.conf.json'), 'utf8'));
    if (compareVersions(config.bundle.macOS.minimumSystemVersion || '10.13', minimum) !== 0) throw new Error('App and QuickLook minimum macOS versions differ.');
    if (process.env.MACOSX_DEPLOYMENT_TARGET && compareVersions(process.env.MACOSX_DEPLOYMENT_TARGET, minimum) !== 0) throw new Error('Rust/C deployment target differs from the application configuration.');
    process.env.MACOSX_DEPLOYMENT_TARGET = minimum;
    process.env.TAURI_ENV_TARGET_TRIPLE = target;
    return { host: host.slice(6), rust: command('rustc', ['--version']), sdk: command('xcrun', ['--sdk', 'macosx', '--show-sdk-version']), swift: command('xcrun', ['swiftc', '--version']).split('\n')[0] };
  });
  stage('moltenvk-architecture-deployment-dependencies', () => {
    const library = moltenVkLibrary();
    command('xcrun', ['lipo', library, '-verify_arch', arch]);
    return validateMacLibrary(inspectMachO(fs.readFileSync(library)), arch, minimum);
  });
  // This stage really compiles and signs the extension before any Tauri build;
  // it is not a Swift parse-only check and does not launch Finder or the app.
  stage('quicklook-swift-link-and-sign', () => {
    buildMacosThumbnail({ thumbnailOnly: true });
    const executable = path.join(root, 'tools/macos-thumbnail-extension/target/NovaThumbnail.appex/Contents/MacOS/NovaThumbnail');
    const binary = inspectMachO(fs.readFileSync(executable)).find(slice => slice.architecture === arch);
    if (!binary || compareVersions(binary.minimumSystemVersion || '0', minimum) !== 0) throw new Error('Compiled QuickLook target/deployment differs.');
    command('codesign', ['--verify', '--strict', path.dirname(path.dirname(path.dirname(executable)))]);
    return binary;
  });
  fs.mkdirSync(path.dirname(output), { recursive: true }); fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n');
  if (report.stages.some(item => item.status !== 'passed')) throw new Error(`macOS preflight failed; consolidated results: ${path.relative(root, output)}`);
  return report;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const at = process.argv.indexOf('--target');
  try { if (at < 0 || !process.argv[at + 1]) throw new Error('Expected --target <native macOS target>.'); preflightMacosPublic({ target: process.argv[at + 1] }); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
