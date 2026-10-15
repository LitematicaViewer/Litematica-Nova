import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { moltenVkLibrary } from './build-macos-thumbnail.mjs';

export const LLVM_VERSION = '22.1.2';
export const RUST_TOOLCHAIN = '1.95.0';
export const LLVM_DOWNLOADS = Object.freeze({
  arm64: Object.freeze({
    name: 'LLVM-22.1.2-macOS-ARM64.tar.xz',
    sha256: '7b290ec4529c309b6b952d64a3513595c816dff1c217d54466a586d6e35dcec9',
  }),
  source: Object.freeze({
    name: 'llvm-project-22.1.2.src.tar.xz',
    sha256: '62f2f13ff25b1bb28ea507888e858212d19aafb65e8e72b4a65ee0629ec4ae0c',
  }),
});

export function parseArguments(args) {
  const values = {};
  for (let index = 0; index < args.length; index += 2) {
    const option = args[index];
    if (!['--target', '--tooling-root', '--output'].includes(option)
        || values[option] || !args[index + 1] || args[index + 1].startsWith('--')) {
      throw new Error('Expected --target, --tooling-root and --output exactly once.');
    }
    values[option] = args[index + 1];
  }
  if (!['x86_64-apple-darwin', 'aarch64-apple-darwin'].includes(values['--target'])
      || !values['--tooling-root'] || !values['--output']) {
    throw new Error('A supported macOS target and both input/output paths are required.');
  }
  return { target: values['--target'], toolingRoot: path.resolve(values['--tooling-root']), output: path.resolve(values['--output']) };
}

export function targetArchitecture(target) {
  if (target === 'aarch64-apple-darwin') return 'arm64';
  if (target === 'x86_64-apple-darwin') return 'x86_64';
  throw new Error('Unsupported macOS target.');
}

export function sourceBuildArguments(source, build, prefix) {
  return ['-S', path.join(source, 'llvm'), '-B', build, '-G', 'Ninja',
    '-DCMAKE_BUILD_TYPE=Release', `-DCMAKE_INSTALL_PREFIX=${prefix}`,
    '-DLLVM_TARGETS_TO_BUILD=X86;AArch64', '-DLLVM_ENABLE_PROJECTS=',
    '-DLLVM_BUILD_LLVM_DYLIB=ON', '-DLLVM_LINK_LLVM_DYLIB=ON',
    '-DLLVM_DYLIB_COMPONENTS=all', '-DLLVM_ENABLE_RTTI=ON',
    '-DLLVM_ENABLE_ASSERTIONS=OFF', '-DLLVM_INCLUDE_TESTS=OFF',
    '-DLLVM_INCLUDE_EXAMPLES=OFF', '-DLLVM_INCLUDE_BENCHMARKS=OFF',
    '-DLLVM_ENABLE_ZLIB=OFF', '-DLLVM_ENABLE_ZSTD=OFF',
    '-DLLVM_ENABLE_LIBXML2=OFF', '-DLLVM_ENABLE_LIBEDIT=OFF',
    '-DLLVM_PARALLEL_LINK_JOBS=1',
    '-DLLVM_DISTRIBUTION_COMPONENTS=opt;llc;llvm-config;llvm-nm;llvm-ar;LLVM;llvm-headers;cmake-exports'];
}

export function validateVersions(rust, versions, target, hostTarget = target) {
  if (!rust.startsWith(`rustc ${RUST_TOOLCHAIN} `)
      || !rust.includes(`LLVM version: ${LLVM_VERSION}`)
      || !rust.split(/\r?\n/).includes(`host: ${hostTarget}`)) {
    throw new Error('The native Rust compiler must be exactly 1.95.0 / LLVM 22.1.2 for this target.');
  }
  for (const name of ['opt', 'llc', 'llvm-nm', 'llvm-config']) {
    const expected = name === 'llvm-config' ? /^22\.1\.2\s*$/ : /\bversion 22\.1\.2(?:\s|$)/;
    if (!expected.test(versions[name] || '')) throw new Error(`Missing exact LLVM ${LLVM_VERSION} tool: ${name}`);
  }
  for (const selected of new Set([target, hostTarget])) {
    const backend = targetArchitecture(selected) === 'arm64' ? 'aarch64' : 'x86-64';
    if (!new RegExp(`^\\s*${backend}\\s+-`, 'm').test(versions.llc)) throw new Error('LLVM lacks the actual selected-target/host code generator.');
  }
}

export function validateMachObject(bytes, target) {
  const architecture = targetArchitecture(target) === 'arm64' ? 0x0100000c : 0x01000007;
  if (bytes.length < 32 || bytes.readUInt32LE(0) !== 0xfeedfacf
      || bytes.readUInt32LE(4) !== architecture || bytes.readUInt32LE(12) !== 1) {
    throw new Error('The VM probe did not produce a matching real 64-bit Mach-O object.');
  }
}

async function digest(file) {
  const hash = crypto.createHash('sha256');
  for await (const chunk of fs.createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

// Both stdout and stderr of compiler/Amice commands stay in RUNNER_TEMP.
// Only stage names and exit statuses are written to the public job log.
function runner(logRoot, environment) {
  return (stage, program, args, { cwd, capture = false } = {}) => {
    const file = path.join(logRoot, `${stage}.log`);
    console.log(`[nova-macos-ci] stage=${stage} status=start`);
    const fd = fs.openSync(file, 'a', 0o600);
    let result;
    try {
      result = spawnSync(program, args, { cwd, env: environment,
        stdio: capture ? ['ignore', 'pipe', 'pipe'] : ['ignore', fd, fd],
        encoding: capture ? 'utf8' : undefined, maxBuffer: 4 * 1024 * 1024 });
      if (capture) {
        fs.writeSync(fd, result.stdout || ''); fs.writeSync(fd, result.stderr || '');
      }
      if (result.error) fs.writeSync(fd, String(result.error));
    } finally { fs.closeSync(fd); }
    console.log(`[nova-macos-ci] stage=${stage} exit=${result.status ?? 1}`);
    if (result.error || result.status !== 0) throw new Error(`${stage} failed; private diagnostics: ${file}`);
    return capture ? result.stdout : '';
  };
}

async function archive(descriptor, toolingRoot, scratch, run) {
  const supplied = path.join(toolingRoot, descriptor.name);
  const destination = fs.existsSync(supplied) ? supplied : path.join(scratch, descriptor.name);
  if (!fs.existsSync(destination)) {
    const temporary = `${destination}.download`;
    const url = `https://github.com/llvm/llvm-project/releases/download/llvmorg-${LLVM_VERSION}/${descriptor.name}`;
    run(`download-${descriptor === LLVM_DOWNLOADS.source ? 'llvm-source' : 'llvm-arm64'}`, 'curl',
      ['--fail', '--location', '--proto', '=https', '--proto-redir', '=https', '--retry', '2', '--connect-timeout', '30', '--output', temporary, url]);
    if (await digest(temporary) !== descriptor.sha256) throw new Error('Official LLVM archive SHA256 mismatch.');
    fs.renameSync(temporary, destination);
  } else if (await digest(destination) !== descriptor.sha256) {
    throw new Error('Cached/supplied LLVM archive SHA256 mismatch.');
  }
  return destination;
}

async function llvmPrefix(toolingRoot, scratch, arch, run, env) {
  if (env.NOVA_LLVM_ROOT) return path.resolve(env.NOVA_LLVM_ROOT);
  const supplied = path.join(toolingRoot, 'llvm22');
  if (fs.existsSync(supplied)) return supplied;
  const prefix = path.join(scratch, 'llvm22');
  if (fs.existsSync(path.join(prefix, 'bin/llvm-config'))) return prefix;
  if (arch === 'arm64') {
    const input = await archive(LLVM_DOWNLOADS.arm64, toolingRoot, scratch, run);
    fs.mkdirSync(prefix, { recursive: true });
    run('extract-llvm-arm64', 'tar', ['-xf', input, '--strip-components=1', '-C', prefix]);
    return prefix;
  }
  // LLVM 22.1.2 has no official macOS Intel binary asset. Compile its exact,
  // hash-pinned release source instead of installing a different Homebrew LLVM.
  const input = await archive(LLVM_DOWNLOADS.source, toolingRoot, scratch, run);
  const source = path.join(scratch, 'llvm-source');
  const build = path.join(scratch, 'llvm-build');
  fs.mkdirSync(source, { recursive: true });
  run('extract-llvm-source', 'tar', ['-xf', input, '--strip-components=1', '-C', source]);
  run('llvm-build-dependencies', 'brew', ['install', 'cmake', 'ninja']);
  run('configure-llvm-intel', 'cmake', sourceBuildArguments(source, build, prefix));
  const jobs = Math.max(1, Math.min(4, os.availableParallelism() - 1, Math.floor(os.totalmem() / (3 * 1024 ** 3))));
  run('build-llvm-intel', 'cmake', ['--build', build, '--target', 'distribution', '--parallel', String(jobs)]);
  run('install-llvm-intel', 'cmake', ['--build', build, '--target', 'install-distribution', '--parallel', '1']);
  return prefix;
}

export async function setupMacosCi(options) {
  if (process.platform !== 'darwin') throw new Error('This bootstrap runs on an actual macOS runner only.');
  targetArchitecture(options.target);
  const arch = process.arch === 'arm64' ? 'arm64' : process.arch === 'x64' ? 'x86_64' : '';
  if (!arch) throw new Error('Unsupported actual macOS host architecture.');
  const hostTriple = arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
  if (!process.env.RUNNER_TEMP) throw new Error('RUNNER_TEMP is required for private build diagnostics.');
  const scratch = path.join(path.resolve(process.env.RUNNER_TEMP), 'nova-macos-toolchain', options.target);
  const logs = path.join(scratch, 'logs');
  fs.mkdirSync(logs, { recursive: true, mode: 0o700 });
  const env = { ...process.env, HOMEBREW_NO_AUTO_UPDATE: '1', RUSTUP_TOOLCHAIN: RUST_TOOLCHAIN };
  for (const name of ['RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'NOVA_AMICE_CONTEXT']) delete env[name];
  for (const name of Object.keys(env)) if (name.startsWith('AMICE_')) delete env[name];
  const run = runner(logs, env);
  run('apple-sdk', 'xcrun', ['--sdk', 'macosx', '--show-sdk-path'], { capture: true });
  run('rust-toolchain', 'rustup', ['toolchain', 'install', RUST_TOOLCHAIN, '--profile', 'minimal', '--component', 'llvm-tools-preview', '--target', options.target]);
  const rustc = run('locate-rustc', 'rustup', ['which', '--toolchain', RUST_TOOLCHAIN, 'rustc'], { capture: true }).trim();
  const rustVersion = run('rust-version', rustc, ['-vV'], { capture: true });
  const llvmRoot = await llvmPrefix(options.toolingRoot, scratch, arch, run, env);
  const versions = {};
  for (const tool of ['opt', 'llc', 'llvm-nm', 'llvm-config']) {
    versions[tool] = run(`llvm-version-${tool}`, path.join(llvmRoot, 'bin', tool), ['--version'], { capture: true });
  }
  validateVersions(rustVersion, versions, options.target, hostTriple);
  run('llvm-native-architecture', 'xcrun', ['lipo', '-verify_arch', arch, path.join(llvmRoot, 'bin/opt')]);
  env.LLVM_SYS_221_PREFIX = llvmRoot;
  env.LLVM_CONFIG_PATH = path.join(llvmRoot, 'bin/llvm-config');
  env.PATH = `${path.join(llvmRoot, 'bin')}${path.delimiter}${env.PATH}`;
  const mode = run('llvm-link-mode', env.LLVM_CONFIG_PATH, ['--shared-mode'], { capture: true }).trim();
  if (!['shared', 'static'].includes(mode)) throw new Error('LLVM reports an unsupported linking mode.');
  const amice = path.join(options.toolingRoot, 'amice');
  const manifest = path.join(amice, 'Cargo.toml');
  if (!fs.existsSync(manifest) || !fs.existsSync(path.join(amice, 'Cargo.lock'))) throw new Error('The fixed, patched Amice source and Cargo.lock must be provided in tooling/amice.');
  if (mode === 'static') {
    const support = fs.readFileSync(path.join(amice, 'crates/amice-build-support/src/lib.rs'), 'utf8');
    if (!support.includes('AMICE_LLVM_STATIC_HOST') || !support.includes('static=LLVMExecutionEngine')) {
      throw new Error('The verified static-host Amice tool patch is missing.');
    }
    if (!fs.existsSync(path.join(llvmRoot, 'lib/libLLVMExecutionEngine.a'))) throw new Error('Exact official LLVMExecutionEngine component archive is missing.');
    env.AMICE_LLVM_STATIC_HOST = '1';
  }
  const amiceTarget = path.join(scratch, 'amice-target');
  const features = arch === 'arm64' ? 'llvm22-1,amice-plugin/target-aarch64' : 'llvm22-1';
  // dynamic_lookup resolves the static opt host's real exported LLVM symbols;
  // it does not link another LLVM core or provide stub C APIs. The probe below
  // must actually load, virtualize, codegen and run or no output is admitted.
  const link = mode === 'static' ? ['-C', 'link-arg=-Wl,-undefined,dynamic_lookup'] : [];
  run('build-amice', 'cargo', [`+${RUST_TOOLCHAIN}`, 'rustc', '--locked', '--release', '--manifest-path', manifest,
    '--target-dir', amiceTarget, '--no-default-features', '--features', features, '--lib', '--',
    ...link, '-C', `link-arg=-Wl,-rpath,${path.join(llvmRoot, 'lib')}`], { cwd: amice });
  const amicePlugin = path.join(amiceTarget, 'release/libamice.dylib');
  run('amice-native-architecture', 'xcrun', ['lipo', '-verify_arch', arch, amicePlugin]);
  const vmProfile = path.join(amice, 'crates/amice-vm/profiles/ruoke');
  if (!fs.existsSync(path.join(vmProfile, 'manifest.toml'))) throw new Error('The real release VM profile is missing.');
  const wrapper = path.join(options.toolingRoot, 'protect-rustc.py');
  if (!fs.existsSync(wrapper)) throw new Error('The unchanged release protection wrapper must be included in private tooling.');
  const context = { schema: 'nova.amice.wrapper.v1', seed: 'nova-macos-toolchain-r1', llvmVersion: LLVM_VERSION,
    llvmRoot, plugin: amicePlugin, pluginSha256: await digest(amicePlugin), vmProfile,
    privateRunRoot: path.join(scratch, 'probe-target'), hostTriple, targetTriple: options.target, objectFormat: 'macho' };
  const contextFile = path.join(scratch, 'probe-context.json');
  fs.writeFileSync(contextFile, JSON.stringify(context), { mode: 0o600 });
  run('real-vm-target-probe', 'python3', [wrapper, '--check-target', rustc, contextFile]);
  validateMachObject(fs.readFileSync(path.join(context.privateRunRoot, 'target-toolchain-probe/probe.o')), options.target);
  // A host plugin and a target object are different identities. The selected
  // cross-target object is genuinely VM-transformed above, but native parity
  // executes a separate host object. No Rosetta/GPU execution is invented.
  let nativeContext = context;
  if (options.target !== hostTriple) {
    nativeContext = { ...context, targetTriple: hostTriple, privateRunRoot: path.join(scratch, 'probe-host') };
    const nativeContextFile = path.join(scratch, 'probe-host-context.json');
    fs.writeFileSync(nativeContextFile, JSON.stringify(nativeContext), { mode: 0o600 });
    run('real-vm-native-host-probe', 'python3', [wrapper, '--check-target', rustc, nativeContextFile]);
  }
  const probe = path.join(nativeContext.privateRunRoot, 'target-toolchain-probe');
  validateMachObject(fs.readFileSync(path.join(probe, 'probe.o')), hostTriple);
  const main = path.join(probe, 'native-parity.rs');
  fs.writeFileSync(main, `extern "C" { fn nova_vm_target_probe(value: u64) -> u64; }
fn main() {
    let mut values = vec![0, 1, 2, 511, 512, u64::MAX, u64::MAX - 1, 1_u64 << 63];
    let mut value = 0x6a09_e667_f3bc_c909_u64;
    for _ in 0..4096 { value ^= value << 13; value ^= value >> 7; value ^= value << 17; values.push(value); }
    let mut checksum = 0_u64;
    for &input in &values { let result = unsafe { nova_vm_target_probe(std::hint::black_box(input)) };
        assert_eq!(result, (input ^ 519).wrapping_add(17)); checksum = checksum.wrapping_mul(0x100000001b3) ^ result; }
    println!("PASS cases={} checksum={checksum:016X}", values.len());
}
`);
  run('baseline-codegen', path.join(llvmRoot, 'bin/llc'), ['-O3', '-filetype=obj', `-mtriple=${hostTriple}`, '-relocation-model=pic', path.join(probe, 'probe.ll'), '-o', path.join(probe, 'baseline.o')]);
  validateMachObject(fs.readFileSync(path.join(probe, 'baseline.o')), hostTriple);
  const results = [];
  for (const [name, object] of [['baseline', 'baseline.o'], ['protected', 'probe.o']]) {
    const binary = path.join(probe, name);
    run(`${name}-link`, rustc, ['--edition=2021', '-Copt-level=2', '-Cpanic=abort', main, `-Clink-arg=${path.join(probe, object)}`, '-o', binary]);
    results.push(run(`${name}-native-run`, binary, [], { capture: true }).trim());
  }
  if (!/^PASS cases=4104 checksum=[0-9A-F]{16}$/.test(results[0]) || results[0] !== results[1]) throw new Error('Actual native VM probe output differs from the baseline.');
  fs.writeFileSync(path.join(probe, 'native-parity-result.json'), JSON.stringify({ hostTriple, targetTriple: options.target,
    executionTriple: hostTriple, selectedTargetExecutionVerified: options.target === hostTriple,
    cases: 4104, result: results[1], vmApplied: true,
    scope: 'toolchain compatibility only; not Nova GPU or production-function acceptance' }), { mode: 0o600 });
  let moltenVk;
  try { moltenVk = moltenVkLibrary(env); }
  catch (error) {
    if (env.NOVA_MOLTENVK_LIBRARY) throw error;
    run('moltenvk-dependency', 'brew', ['install', 'molten-vk']);
    const prefix = run('moltenvk-prefix', 'brew', ['--prefix', 'molten-vk'], { capture: true }).trim();
    moltenVk = moltenVkLibrary({ NOVA_MOLTENVK_LIBRARY: path.join(prefix, 'lib/libMoltenVK.dylib') });
  }
  run('moltenvk-native-architecture', 'xcrun', ['lipo', '-verify_arch', arch, moltenVk]);
  // This library is for the private build host only. Each public target's own
  // packaging runner must locate and lipo-check its target-native MoltenVK;
  // never copy an ARM host library into an Intel candidate.
  const result = { llvmRoot, amicePlugin, vmProfile, moltenVkLibrary: fs.realpathSync(moltenVk),
    moltenVkLibraryArchitecture: arch, hostTriple, targetTriple: options.target, rustToolchain: RUST_TOOLCHAIN };
  fs.mkdirSync(path.dirname(options.output), { recursive: true });
  fs.writeFileSync(`${options.output}.tmp`, `${JSON.stringify(result, null, 2)}\n`, { mode: 0o600 });
  fs.renameSync(`${options.output}.tmp`, options.output);
  console.log('[nova-macos-ci] stage=complete exit=0');
  return result;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { await setupMacosCi(parseArguments(process.argv.slice(2))); }
  catch (error) {
    if (process.env.RUNNER_TEMP) {
      try { fs.appendFileSync(path.join(process.env.RUNNER_TEMP, 'nova-macos-ci-failure.log'), `${error.stack || error}\n`, { mode: 0o600 }); } catch {}
    }
    console.error('[nova-macos-ci] stage=failed exit=1; diagnostics remain in RUNNER_TEMP');
    process.exitCode = 1;
  }
}
