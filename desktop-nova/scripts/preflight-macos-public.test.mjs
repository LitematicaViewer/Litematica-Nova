import assert from 'node:assert/strict';
import test from 'node:test';
import { compareVersions, inspectMachO, validateMacLibrary, preflightMacosPublic } from './preflight-macos-public.mjs';

function dylib(architecture = 'arm64', minimum = 13, dependency = '/usr/lib/libSystem.B.dylib') {
  const name = Buffer.from(`${dependency}\0`), load = Buffer.alloc((24 + name.length + 7) & ~7);
  load.writeUInt32LE(0xc); load.writeUInt32LE(load.length, 4); load.writeUInt32LE(24, 8); name.copy(load, 24);
  const build = Buffer.alloc(24); build.writeUInt32LE(0x32); build.writeUInt32LE(24, 4); build.writeUInt32LE(1, 8); build.writeUInt32LE(minimum << 16, 12); build.writeUInt32LE(15 << 16, 16);
  const header = Buffer.alloc(32); header.writeUInt32LE(0xfeedfacf); header.writeUInt32LE(architecture === 'arm64' ? 0x0100000c : 0x01000007, 4); header.writeUInt32LE(6, 12); header.writeUInt32LE(2, 16); header.writeUInt32LE(build.length + load.length, 20);
  return Buffer.concat([header, build, load]);
}

test('reads actual load command architecture, minimum OS and dependency fields', () => {
  const [slice] = inspectMachO(dylib());
  assert.equal(slice.architecture, 'arm64'); assert.equal(slice.minimumSystemVersion, '13.0.0'); assert.equal(slice.sdk, '15.0.0');
  assert.deepEqual(slice.dependencies, ['/usr/lib/libSystem.B.dylib']);
  assert.equal(validateMacLibrary([slice], 'arm64', '13.0'), slice);
});
test('universal slices use their actual CPU identity rather than host or filename', () => {
  const arm = dylib(), intel = dylib('x86_64'), header = Buffer.alloc(48);
  header.writeUInt32BE(0xcafebabe); header.writeUInt32BE(2, 4);
  for (const [i, cpu, offset, size] of [[0, 0x0100000c, 48, arm.length], [1, 0x01000007, 48 + arm.length, intel.length]]) {
    const at = 8 + i * 20; header.writeUInt32BE(cpu, at); header.writeUInt32BE(offset, at + 8); header.writeUInt32BE(size, at + 12);
  }
  assert.deepEqual(inspectMachO(Buffer.concat([header, arm, intel])).map(slice => slice.architecture), ['arm64', 'x86_64']);
});
test('missing architecture, higher actual OS and unbundled dependencies fail before packaging', () => {
  assert.throws(() => validateMacLibrary(inspectMachO(dylib()), 'x86_64', '13.0'), /lacks a real/);
  assert.throws(() => validateMacLibrary(inspectMachO(dylib('arm64', 14)), 'arm64', '13.0'), /requires macOS 14\.0\.0/);
  assert.throws(() => validateMacLibrary(inspectMachO(dylib('arm64', 13, '/opt/homebrew/lib/nonbundled.dylib')), 'arm64', '13.0'), /unbundled non-system/);
});
test('corrupt native metadata cannot be accepted as a successful platform check', () => {
  assert.throws(() => inspectMachO(Buffer.alloc(3)), /Truncated/);
  const invalid = dylib(); invalid.writeUInt32LE(0xffffffff, 20);
  assert.throws(() => inspectMachO(invalid), /bounds/);
  assert.equal(compareVersions('13.0', '13.0.0'), 0); assert.ok(compareVersions('15.5', '13') > 0);
});
test('a foreign host never claims a native macOS preflight passed', { skip: process.platform === 'darwin' }, () => {
  assert.throws(() => preflightMacosPublic({ target: 'aarch64-apple-darwin' }), /real native macOS runner/);
});
