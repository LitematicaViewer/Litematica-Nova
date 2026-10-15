[CmdletBinding()]
param(
    [string] $RepoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path -LiteralPath $RepoRoot).Path
if (-not $IsWindows) { throw 'This candidate builder supports Windows x64 only.' }
$packageRoot = Join-Path $repo 'bin/protected'
$configPath = Join-Path $packageRoot 'protected-build.json'
$stage = [IO.Path]::GetFullPath((Join-Path $repo 'bin/public-beta'))
if (-not $stage.StartsWith($repo + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Public staging leaves the repository.' }
$backendNames = @('litematica_core.exe','litematica_native_viewer.exe','nova-render-service.exe','nova-build-worker.exe')

Push-Location -LiteralPath $repo
try {
    # GitHub checkout binds an exact commit, so an empty detached result is valid.
    $branch = & git branch --show-current
    if ($LASTEXITCODE -ne 0) { throw 'Unable to resolve public checkout state.' }
    if ($branch -ne 'main' -and -not ($env:GITHUB_ACTIONS -eq 'true' -and $env:GITHUB_REF -eq 'refs/heads/main' -and -not $branch)) {
        throw 'Compile the audited public main only; never use private dev as public build input.'
    }
    $sha = (& git rev-parse --verify HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $sha -notmatch '^[a-f0-9]{40,64}$') { throw 'Unable to resolve the public source commit.' }
    if (-not (Test-Path -LiteralPath $configPath -PathType Leaf)) { throw 'The signed private-environment payload and its public build identity are missing.' }
    # The three release trust parameters come from one audited public identity.
    # No signing key, private record, plaintext DLL, private source or old backend
    # executable is read by this script.
    $env:NOVA_VERIFY_PROTECTED_PACKAGE = $packageRoot
    $verify = @'
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
(async () => {
  const root = process.env.NOVA_VERIFY_PROTECTED_PACKAGE;
  const readJson = (file, limit) => {
    const size = fs.statSync(file).size;
    if (!size || size > limit) throw new Error('Protected metadata exceeds its byte budget');
    return JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
  };
  const config = readJson(path.join(root, 'protected-build.json'), 16384);
  const fields = ['schema','version','buildId','moduleId','manifestPublicKey','endpoint','manifestFile','ciphertextFile'];
  if (Object.keys(config).some(key => !fields.includes(key))) throw new Error('Unexpected public protected-build metadata field');
  if (config.schema !== 'nova.protected-build.v1' || !/^[a-zA-Z0-9_.:-]{1,128}$/.test(config.buildId)) throw new Error('Invalid protected build identity');
  if (config.moduleId !== 'nova_protected_module.dll' || config.manifestFile !== config.moduleId + '.manifest.json' || config.ciphertextFile !== config.moduleId + '.enc') throw new Error('Invalid protected module package paths');
  const endpoint = new URL(config.endpoint);
  if (endpoint.protocol !== 'https:' || endpoint.username || endpoint.password || endpoint.search || endpoint.hash) throw new Error('The protected endpoint must be ordinary HTTPS');
  const publicKey = Buffer.from(config.manifestPublicKey, 'base64');
  if (publicKey.length !== 32 || publicKey.toString('base64') !== config.manifestPublicKey) throw new Error('Invalid Ed25519 public key');
  const allowed = new Set(['protected-build.json', config.manifestFile, config.ciphertextFile]);
  for (const item of fs.readdirSync(root, { withFileTypes: true })) if (!item.isFile() || !allowed.has(item.name)) throw new Error('Unexpected file in the ciphertext-only public package');
  const signed = readJson(path.join(root, config.manifestFile), 131072);
  if (Object.keys(signed).sort().join(',') !== 'manifest,signature') throw new Error('Unexpected signed manifest fields');
  const m = signed.manifest;
  // Rust serde signs the declared struct field order. Reconstruct that order,
  // do not trust JSON object insertion order or a platform serializer variant.
  const keys = ['protocol','build_id','module_id','plaintext_sha256','ciphertext_sha256','ciphertext_bytes','nonce','created_ms'];
  if (Object.keys(m).length !== keys.length || Object.keys(m).some(key => !keys.includes(key))) throw new Error('Unexpected module manifest fields');
  const canonical = Object.fromEntries(keys.map(key => [key, m[key]]));
  if (m.protocol !== 1 || m.build_id !== config.buildId || m.module_id !== config.moduleId) throw new Error('Module/build binding mismatch');
  if (!/^[a-f0-9]{64}$/.test(m.plaintext_sha256) || !/^[a-f0-9]{64}$/.test(m.ciphertext_sha256)) throw new Error('Invalid module content hashes');
  if (!Number.isSafeInteger(m.created_ms) || m.created_ms <= 0 || !Number.isSafeInteger(m.ciphertext_bytes) || m.ciphertext_bytes < 17 || m.ciphertext_bytes > 512 * 1024 * 1024) throw new Error('Invalid module size/time');
  if (Buffer.from(m.nonce, 'base64').length !== 12) throw new Error('Invalid authenticated-encryption nonce');
  const signature = Buffer.from(signed.signature, 'base64');
  const key = crypto.createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100','hex'), publicKey]), format:'der', type:'spki' });
  if (signature.length !== 64 || !crypto.verify(null, Buffer.from(JSON.stringify(canonical)), key, signature)) throw new Error('Private-build manifest signature verification failed');
  const ciphertext = path.join(root, config.ciphertextFile);
  if (fs.statSync(ciphertext).size !== m.ciphertext_bytes) throw new Error('Protected module ciphertext size mismatch');
  const hash = crypto.createHash('sha256');
  for await (const bytes of fs.createReadStream(ciphertext)) hash.update(bytes);
  if (hash.digest('hex') !== m.ciphertext_sha256) throw new Error('Protected module ciphertext SHA256 mismatch');
  console.log(JSON.stringify(config));
})().catch(error => { console.error(error.message); process.exitCode = 1; });
'@
    $identityJson = $verify | & node --input-type=commonjs -
    if ($LASTEXITCODE -ne 0) { throw 'The signed encrypted private build is not valid; no unprotected substitute is permitted.' }
    $identity = ($identityJson -join "`n") | ConvertFrom-Json
    $tauriPath = Join-Path $repo 'desktop-nova/src-tauri/tauri.conf.json'
    $tauri = Get-Content -LiteralPath $tauriPath -Raw | ConvertFrom-Json
    if ($identity.version -ne $tauri.version) { throw 'Protected module and desktop candidate versions differ.' }
    $resources = $tauri.bundle.resources
    if ($resources.'../../bin/public-beta' -ne 'bin/viewer-backend' -or $resources.'../../bin/protected' -ne 'bin/viewer-backend/protected') {
        throw 'The reviewed public helpers/protected resources are not mapped to the runtime backend paths.'
    }
    foreach ($entry in $resources.PSObject.Properties) {
        $source = [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $tauriPath) $entry.Name))
        $oldPrivate = Join-Path $repo 'bin/viewer-backend'
        if ($source.Equals($oldPrivate,[StringComparison]::OrdinalIgnoreCase) -or $source.StartsWith($oldPrivate + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) {
            throw 'A resource map still references the old private backend; use only the public staging and signed ciphertext.'
        }
    }
    $runtimeArchive = Join-Path $repo 'pack-in/minecraft-runtime-26.3.zip'
    $runtimeManifest = Get-Content -LiteralPath (Join-Path $repo 'pack-in/minecraft-runtime-26.3.manifest.json') -Raw | ConvertFrom-Json
    if ($runtimeManifest.schema -ne 'nova.minecraft-runtime-resources.v1' -or $runtimeManifest.java_class_entries -ne 0 -or $runtimeManifest.executable_entries -ne 0) { throw 'The vanilla runtime resource manifest is invalid.' }
    if ((Get-FileHash -LiteralPath $runtimeArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $runtimeManifest.archive_sha256) { throw 'The vanilla runtime resource archive changed.' }
    $runtimeRoot = Join-Path $repo 'data/mc26'
    New-Item -ItemType Directory -Path $runtimeRoot -Force | Out-Null
    $archive = [IO.Compression.ZipFile]::OpenRead($runtimeArchive)
    try {
        if ($archive.Entries.Count -ne $runtimeManifest.archive_entries) { throw 'Runtime resource entry count mismatch.' }
        foreach ($entry in $archive.Entries) {
            if ($entry.FullName -notmatch '^26\.3/(oracle/[a-z_]+\.jsonl|oracle/oracle-manifest\.json|client\.jar|assets/indexes/34\.json|assets/objects/[a-f0-9]{2}/[a-f0-9]{40})$') { throw 'Unexpected runtime resource path.' }
            $destination = [IO.Path]::GetFullPath((Join-Path $runtimeRoot $entry.FullName))
            if (-not $destination.StartsWith($runtimeRoot + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or $entry.Length -gt 100MB) { throw 'Runtime resource exceeds its path/size boundary.' }
            New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry,$destination,$true)
        }
    } finally { $archive.Dispose() }
    $jar = [IO.Compression.ZipFile]::OpenRead((Join-Path $runtimeRoot '26.3/client.jar'))
    try { foreach ($entry in $jar.Entries) { if ($entry.FullName -match '(?i)\.(class|java|dll|exe|rs|wgsl|ll|pdb)$' -or ($entry.FullName -match '(?i)^META-INF/' -and $entry.FullName -cne 'META-INF/LICENSE')) { throw 'The Minecraft asset jar contains implementation/executable material.' } } }
    finally { $jar.Dispose() }
    # The browser/version database is embedded in the public Core executable.
    # Verify its concrete tint sidecar before compiling, not after packaging.
    $verifyBlockData = @'
const fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto'), zlib = require('node:zlib');
const directory = path.join(process.argv[2], 'data', 'blockdb');
const blocks = fs.readFileSync(path.join(directory, 'blocks.dat.z'));
const tints = JSON.parse(zlib.inflateSync(fs.readFileSync(path.join(directory, 'plains-tints.dat.z')), { maxOutputLength: 16 * 1024 * 1024 }).toString('utf8'));
const sha = crypto.createHash('sha256').update(blocks).digest('hex');
if (tints.schema !== 1 || tints.purpose !== 'block-database-side-plains-origin' || tints.blocksDatSha256 !== sha || !tints.versions || !tints.tables || !Object.keys(tints.versions).length || Object.values(tints.versions).some(key => !Object.hasOwn(tints.tables, key))) throw new Error('Concrete block-data tint metadata does not match the embedded database');
console.log('Concrete block-data resources ready: embedded input and metadata identities match.');
'@
    $verifyBlockData | & node --input-type=commonjs - $repo
    if ($LASTEXITCODE -ne 0) { throw 'The required embedded block-data resources are missing or mismatched.' }
    # Keep public helper and desktop artifacts free of local checkout/cache paths.
    # The private module was separately compiled and protected in its private environment.
    $env:CARGO_ENCODED_RUSTFLAGS = @(
        "--remap-path-prefix=$repo=/nova/public",
        "--remap-path-prefix=$HOME/.cargo=/nova/cargo",
        "--remap-path-prefix=$HOME/.rustup=/nova/rust"
    ) -join [char]31
    $env:RUSTFLAGS = $null
    $env:CARGO_PROFILE_RELEASE_STRIP = 'symbols'
    $env:CARGO_PROFILE_RELEASE_DEBUG = '0'
    $env:NOVA_PROTECTED_BUILD_ID = $identity.buildId
    $env:NOVA_PROTECTED_MANIFEST_PUBLIC_KEY = $identity.manifestPublicKey
    $env:NOVA_PROTECTED_ENDPOINT = $identity.endpoint
    $env:LBA_BUILD_GIT_SHA = $sha
    if ($env:GITHUB_ENV) {
        @("NOVA_PROTECTED_BUILD_ID=$($identity.buildId)","NOVA_PROTECTED_MANIFEST_PUBLIC_KEY=$($identity.manifestPublicKey)","NOVA_PROTECTED_ENDPOINT=$($identity.endpoint)","LBA_BUILD_GIT_SHA=$sha","CARGO_ENCODED_RUSTFLAGS=$($env:CARGO_ENCODED_RUSTFLAGS)","CARGO_PROFILE_RELEASE_STRIP=symbols","CARGO_PROFILE_RELEASE_DEBUG=0") | Add-Content -LiteralPath $env:GITHUB_ENV -Encoding utf8
    }
    & cargo build --locked --release --manifest-path tools/viewer-core/Cargo.toml --target-dir tools/viewer-core/target/public-beta --no-default-features --features public-beta --bin litematica_core --bin litematica_native_viewer --bin stockpile_server
    if ($LASTEXITCODE -ne 0) { throw 'Public Fast/core/stockpile compilation failed.' }
    & cargo build --locked --release --manifest-path tools/nova-protected-loader/Cargo.toml --target-dir tools/nova-protected-loader/target/public-beta --bins
    if ($LASTEXITCODE -ne 0) { throw 'Public authenticated loader compilation failed.' }
    & (Join-Path $repo 'tools/windows-thumbnail-provider/build.ps1')
    if ($LASTEXITCODE -ne 0) { throw 'The normal Windows thumbnail provider resource failed to build.' }
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    foreach ($existing in Get-ChildItem -LiteralPath $stage -Force) {
        if ($existing.PSIsContainer -or $existing.Name -notin ($backendNames + 'backend-deployment.json')) { throw "Unexpected file in generated public backend staging: $($existing.Name)" }
    }
    $sources = [ordered]@{
        'litematica_core.exe' = Join-Path $repo 'tools/viewer-core/target/public-beta/release/litematica_core.exe'
        'litematica_native_viewer.exe' = Join-Path $repo 'tools/viewer-core/target/public-beta/release/litematica_native_viewer.exe'
        'nova-render-service.exe' = Join-Path $repo 'tools/nova-protected-loader/target/public-beta/release/nova-render-service.exe'
        'nova-build-worker.exe' = Join-Path $repo 'tools/nova-protected-loader/target/public-beta/release/nova-build-worker.exe'
    }
    $artifacts = [ordered]@{}
    foreach ($entry in $sources.GetEnumerator()) {
        $destination = Join-Path $stage $entry.Key
        Copy-Item -LiteralPath $entry.Value -Destination $destination -Force
        $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($hash -ne (Get-FileHash -LiteralPath $entry.Value -Algorithm SHA256).Hash.ToLowerInvariant()) { throw "Public helper staging is incomplete: $($entry.Key)" }
        $artifacts[$entry.Key] = [ordered]@{ bytes=(Get-Item -LiteralPath $destination).Length; sha256=$hash }
    }
    $deployment = [ordered]@{ schema='lba.backend.deployment.v1'; gitSha=$sha; profile='release'; artifacts=$artifacts }
    [IO.File]::WriteAllText((Join-Path $stage 'backend-deployment.json'), ($deployment | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    foreach ($entry in $resources.PSObject.Properties) {
        # build.rs owns generation AND validation of every flat PNG in this map.
        # A fresh checkout has the source archive, not the derived directory yet.
        if ($entry.Name -eq 'gen/block-icons') {
            if (-not (Test-Path -LiteralPath (Join-Path $repo 'pack-in/game-icons/block.zip') -PathType Leaf)) { throw 'Missing source archive for generated block icons.' }
            continue
        }
        $source = [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $tauriPath) $entry.Name))
        if (-not (Test-Path -LiteralPath $source)) { throw "Missing required product resource: $($entry.Name)" }
    }
    if ((& git rev-parse HEAD).Trim() -ne $sha) { throw 'Public source commit changed during the candidate build.' }
    Write-Host "Public backend ready source=$sha protected_build=$($identity.buildId) helpers=4 staging=$stage"
} finally {
    Remove-Item -LiteralPath Env:NOVA_VERIFY_PROTECTED_PACKAGE -ErrorAction SilentlyContinue
    Pop-Location
}
