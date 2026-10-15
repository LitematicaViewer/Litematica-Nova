import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import { LLVM_DOWNLOADS, LLVM_VERSION, RUST_TOOLCHAIN, parseArguments, targetArchitecture,
  sourceBuildArguments, validateVersions, validateMachObject, setupMacosCi, amiceBuildArguments } from './setup-macos-ci.mjs';

test('CLI requires one explicit supported target and existing-input/output locations', () => {
  const result = parseArguments(['--target', 'aarch64-apple-darwin', '--tooling-root', 'fixture/tooling', '--output', 'fixture/result.json']);
  assert.equal(result.target, 'aarch64-apple-darwin');
  assert.throws(() => parseArguments(['--target', 'x86_64-unknown-linux-gnu']), /supported macOS target/);
  assert.throws(() => parseArguments(['--target', 'aarch64-apple-darwin', '--target', 'x86_64-apple-darwin']), /exactly once/);
  assert.throws(() => parseArguments(['--unknown', 'value']), /exactly once/);
});

test('exact LLVM archives are pinned and Intel uses real same-version source', () => {
  assert.equal(LLVM_VERSION, '22.1.2'); assert.equal(RUST_TOOLCHAIN, '1.95.0');
  assert.equal(LLVM_DOWNLOADS.arm64.sha256, '7b290ec4529c309b6b952d64a3513595c816dff1c217d54466a586d6e35dcec9');
  assert.equal(LLVM_DOWNLOADS.source.sha256, '62f2f13ff25b1bb28ea507888e858212d19aafb65e8e72b4a65ee0629ec4ae0c');
  const args = sourceBuildArguments('source', 'build', 'prefix');
  assert.ok(args.includes('-DLLVM_TARGETS_TO_BUILD=X86;AArch64'));
  assert.ok(args.includes('-DLLVM_BUILD_LLVM_DYLIB=ON'));
  assert.ok(args.includes('-DLLVM_LINK_LLVM_DYLIB=ON'));
  assert.ok(args.some(value => value.includes('opt;llc;llvm-config;llvm-nm')));
});

test('every real lipo invocation puts its input before the variadic verify_arch option', () => {
  const source = fs.readFileSync(new URL('./setup-macos-ci.mjs', import.meta.url), 'utf8');
  const invocations = [...source.matchAll(/run\('(llvm-native-architecture|amice-native-architecture|moltenvk-native-architecture)', 'xcrun', \[([^\r\n]+)\]\);/g)];
  assert.equal(invocations.length, 3);
  for (const [, stage, argumentsSource] of invocations) {
    assert.ok(argumentsSource.startsWith("'lipo', "), stage);
    assert.ok(argumentsSource.endsWith(", '-verify_arch', arch"), stage);
    assert.ok(!argumentsSource.startsWith("'lipo', '-verify_arch'"), stage);
  }
});

test('actual Amice build selects the package inside its virtual workspace', () => {
  const args = amiceBuildArguments('tooling/amice/Cargo.toml', 'amice-target', 'arm64', 'llvm22', 'static');
  assert.equal(args[args.indexOf('-p') + 1], 'amice');
  assert.ok(args.indexOf('-p') < args.indexOf('--'));
  assert.equal(args[args.indexOf('--features') + 1], 'llvm22-1,amice-plugin/target-aarch64');
  assert.ok(args.includes('link-arg=-Wl,-undefined,dynamic_lookup'));
  assert.ok(!amiceBuildArguments('manifest', 'target', 'arm64', 'llvm22', 'shared').includes('link-arg=-Wl,-undefined,dynamic_lookup'));
});

function versions() {
  return { opt: 'LLVM version 22.1.2\n', 'llvm-config': '22.1.2\n', 'llvm-nm': 'LLVM version 22.1.2\n',
    llc: 'LLVM version 22.1.2\n  aarch64 - AArch64\n  x86-64 - 64-bit X86\n' };
}
function rust(target) { return `rustc 1.95.0 (fixture)\nhost: ${target}\nLLVM version: 22.1.2\n`; }

test('version gates reject another Rust, LLVM minor/patch, missing tool or code generator', () => {
  for (const target of ['aarch64-apple-darwin', 'x86_64-apple-darwin']) assert.doesNotThrow(() => validateVersions(rust(target), versions(), target));
  assert.throws(() => validateVersions(rust('aarch64-apple-darwin').replace('1.95.0', '1.99.0'), versions(), 'aarch64-apple-darwin'));
  for (const value of ['LLVM version 23.0.0', 'LLVM version 22.1.20', '']) {
    assert.throws(() => validateVersions(rust('aarch64-apple-darwin'), { ...versions(), opt: value }, 'aarch64-apple-darwin'));
  }
  assert.throws(() => validateVersions(rust('aarch64-apple-darwin'), { ...versions(), llc: 'LLVM version 22.1.2' }, 'aarch64-apple-darwin'));
});

test('native host identity must match the requested executable platform', () => {
  assert.equal(targetArchitecture('aarch64-apple-darwin'), 'arm64');
  assert.equal(targetArchitecture('x86_64-apple-darwin'), 'x86_64');
  assert.throws(() => targetArchitecture('x86_64-pc-windows-msvc'));
  assert.throws(() => validateVersions(rust('aarch64-apple-darwin'), versions(), 'x86_64-apple-darwin'));
});

test('ARM host can genuinely produce Intel objects without calling that native Intel execution', () => {
  assert.doesNotThrow(() => validateVersions(rust('aarch64-apple-darwin'), versions(), 'x86_64-apple-darwin', 'aarch64-apple-darwin'));
  assert.throws(() => validateVersions(rust('aarch64-apple-darwin'), { ...versions(), llc: 'LLVM version 22.1.2\n aarch64 - AArch64\n' }, 'x86_64-apple-darwin', 'aarch64-apple-darwin'));
  assert.throws(() => validateVersions(rust('aarch64-apple-darwin'), { ...versions(), llc: 'LLVM version 22.1.2\n x86-64 - X86\n' }, 'x86_64-apple-darwin', 'aarch64-apple-darwin'));
});

test('VM object gate rejects non-Mach-O, wrong CPU, linked dylib or truncated data', () => {
  const bytes = Buffer.alloc(32);
  bytes.writeUInt32LE(0xfeedfacf, 0); bytes.writeUInt32LE(0x0100000c, 4); bytes.writeUInt32LE(1, 12);
  assert.doesNotThrow(() => validateMachObject(bytes, 'aarch64-apple-darwin'));
  assert.throws(() => validateMachObject(bytes, 'x86_64-apple-darwin'));
  bytes.writeUInt32LE(6, 12);
  assert.throws(() => validateMachObject(bytes, 'aarch64-apple-darwin'));
  assert.throws(() => validateMachObject(Buffer.from('ELF fixture'), 'aarch64-apple-darwin'));
});

test('a non-macOS machine never downloads tools or pretends to execute the native probe', { skip: process.platform === 'darwin' }, async () => {
  await assert.rejects(setupMacosCi({ target: 'aarch64-apple-darwin' }), /actual macOS runner/);
});
