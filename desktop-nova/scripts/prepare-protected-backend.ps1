[CmdletBinding()]
param(
    [string] $RepoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path,
    [switch] $Development,
    [switch] $CheckOnly,
    [switch] $ReuseExisting,
    [switch] $Force,
    [ValidateSet('prebuilt', 'local')][string] $RuntimeSource = $(if ($env:NOVA_RUNTIME_SOURCE) { $env:NOVA_RUNTIME_SOURCE } else { 'prebuilt' }),
    [string] $PrivateSourceDir = $env:NOVA_PRIVATE_SOURCE_DIR
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
$cargoCacheRoot = if ($env:NOVA_PUBLIC_CARGO_CACHE_ROOT) { [IO.Path]::GetFullPath($env:NOVA_PUBLIC_CARGO_CACHE_ROOT) } else { $repo }
$coreTarget = Join-Path $cargoCacheRoot 'tools/viewer-core/target/public-beta'
$loaderTarget = Join-Path $cargoCacheRoot 'tools/nova-protected-loader/target/public-beta'
$stockpileSource = Join-Path $coreTarget 'release/stockpile_server.exe'

# Development launches run this script on every start. Rewriting a file whose
# bytes did not change only refreshes its mtime, and desktop-nova/src-tauri/build.rs
# watches bin/public-beta, so an unconditional copy forces a full crate rebuild
# (about 14 s) on every launch. All writes below are therefore content-aware;
# -Force restores unconditional rewriting.
function Test-IdenticalFile([string] $Source, [string] $Destination) {
    if (-not (Test-Path -LiteralPath $Destination -PathType Leaf)) { return $false }
    if ((Get-Item -LiteralPath $Source).Length -ne (Get-Item -LiteralPath $Destination).Length) { return $false }
    return (Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash
}

function Copy-IfChanged([string] $Source, [string] $Destination) {
    if (-not $Force -and (Test-IdenticalFile $Source $Destination)) { return $false }
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
    return $true
}

function Write-TextIfChanged([string] $Path, [string] $Content) {
    if (-not $Force -and (Test-Path -LiteralPath $Path -PathType Leaf) -and ([IO.File]::ReadAllText($Path) -ceq $Content)) { return $false }
    [IO.File]::WriteAllText($Path, $Content, [Text.UTF8Encoding]::new($false))
    return $true
}

function Resolve-ProductResourceSource([string] $ConfigPath, [string] $SourceName, [string] $DestinationName, [string] $StockpileSource) {
    # The platform overlay replaces the base resource with this exact Cargo
    # output. A trusted public snapshot may intentionally share its build cache
    # with the existing repository instead of creating a second target tree.
    if ($DestinationName -eq 'bin/stockpile-server/windows-x64/stockpile_server.exe') { return $StockpileSource }
    return [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $ConfigPath) $SourceName))
}

Push-Location -LiteralPath $repo
try {
    # Release builds keep their main-only gate. Public UI feature branches use
    # the same actual source graph/package checks, not a branch-name allowlist.
    $checkArgs = @((Join-Path $PSScriptRoot 'check-public-checkout.mjs'), '--repo', $repo)
    if ($Development) { $checkArgs += '--development' }
    if ($CheckOnly) { $checkArgs += '--no-download' }
    if ($ReuseExisting) { $checkArgs += '--reuse-existing' }
    $checkArgs += @('--runtime-source', $RuntimeSource)
    if ($PrivateSourceDir) { $checkArgs += @('--private-source', $PrivateSourceDir) }
    $checkedJson = & node @checkArgs
    if ($LASTEXITCODE -eq 77) { throw 'NOVA_RUNTIME_DOWNLOAD_AUTH_REQUIRED' }
    if ($LASTEXITCODE -ne 0) { throw 'Public checkout/runtime preflight failed.' }
    $checked = ($checkedJson -join "`n") | ConvertFrom-Json
    $sha = $checked.sha
    $identity = $checked.identity
    $localRuntime = $checked.runtimeSource -eq 'local'
    $packageRoot = $checked.runtimePackageDir
    $helperSource = if ($checked.backendSource) { $checked.backendSource } else { $sha }
    $tauriPath = Join-Path $repo 'desktop-nova/src-tauri/tauri.conf.json'
    $tauri = Get-Content -LiteralPath $tauriPath -Raw | ConvertFrom-Json
    # AppVersion belongs to the Host/installer. ABI, signed capabilities and the
    # approved runtime combination were verified by check-public-checkout.
    # A UI/version-only change does not generate another encrypted renderer.
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
    foreach ($requiredInput in @('data/blockdb/blocks.dat.z', 'data/blockdb/plains-tints.dat.z', 'pack-in/game-icons/block.zip')) {
        if (-not (Test-Path -LiteralPath (Join-Path $repo $requiredInput) -PathType Leaf)) { throw "Missing required compile input: $requiredInput" }
    }
    if ($CheckOnly -or $ReuseExisting) {
        if ($ReuseExisting) {
            Write-Host "Existing backend verified source=$($checked.backendSource); UI source=$sha. No helper rebuild."
        } else {
            Write-Host "Public development inputs verified source=$sha. No files written; no build started."
        }
        if ($CheckOnly) { return }
    }
    $env:NOVA_RUNTIME_SOURCE = $checked.runtimeSource
    if ($localRuntime) {
        $env:NOVA_DEVELOPMENT_RUNTIME_JSON = $identity.developmentDescriptor | ConvertTo-Json -Depth 8 -Compress
        $env:NOVA_DEVELOPMENT_MODULE_PATH = $identity.modulePath
        foreach ($name in @('NOVA_PROTECTED_BUILD_ID','NOVA_PROTECTED_MANIFEST_PUBLIC_KEY','NOVA_PROTECTED_ENDPOINT','NOVA_PROTECTED_RUNTIME_COMPATIBILITY')) { Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue }
    } else {
        $env:NOVA_PROTECTED_BUILD_ID = $identity.buildId
        $env:NOVA_PROTECTED_MANIFEST_PUBLIC_KEY = $identity.manifestPublicKey
        $env:NOVA_PROTECTED_ENDPOINT = $identity.endpoint
        $env:NOVA_PROTECTED_RUNTIME_COMPATIBILITY = $identity.signedCompatibility | ConvertTo-Json -Depth 8 -Compress
        Remove-Item Env:NOVA_DEVELOPMENT_RUNTIME_JSON,Env:NOVA_DEVELOPMENT_MODULE_PATH -ErrorAction SilentlyContinue
    }
    $env:NOVA_PUBLIC_HELPER_INPUT_ID = $checked.helperInputId
    $env:NOVA_RUNTIME_PACKAGE_DIR = $packageRoot
    $runtimeStage = Join-Path $repo 'bin/protected'
    # Development uses the verified cache plus an ignored local resource
    # overlay. Never rewrite the release package tracked by the public tree.
    if (-not $Development -and -not [IO.Path]::GetFullPath($packageRoot).Equals([IO.Path]::GetFullPath($runtimeStage), [StringComparison]::OrdinalIgnoreCase)) {
        New-Item -ItemType Directory -Path $runtimeStage -Force | Out-Null
        $runtimeNames = @('protected-build.json', 'runtime-compatibility.json', $identity.manifestFile, $identity.ciphertextFile)
        foreach ($existing in Get-ChildItem -LiteralPath $runtimeStage -Force) {
            if ($existing.PSIsContainer -or $existing.Name -notin $runtimeNames) { throw 'Unexpected file in the protected runtime staging directory.' }
        }
        foreach ($name in $runtimeNames) { Copy-IfChanged (Join-Path $packageRoot $name) (Join-Path $runtimeStage $name) | Out-Null }
    }
    if ($ReuseExisting) { return }
    $runtimeRoot = Join-Path $repo 'data/mc26'
    New-Item -ItemType Directory -Path $runtimeRoot -Force | Out-Null
    $archive = [IO.Compression.ZipFile]::OpenRead($runtimeArchive)
    try {
        if ($archive.Entries.Count -ne $runtimeManifest.archive_entries) { throw 'Runtime resource entry count mismatch.' }
        # Validate every entry first, then only rewrite the tree when something is
        # actually missing or truncated: the payload is about 158 MB.
        foreach ($entry in $archive.Entries) {
            if ($entry.FullName -notmatch '^26\.3/(oracle/[a-z_]+\.jsonl|oracle/oracle-manifest\.json|client\.jar|assets/indexes/34\.json|assets/objects/[a-f0-9]{2}/[a-f0-9]{40})$') { throw 'Unexpected runtime resource path.' }
            $destination = [IO.Path]::GetFullPath((Join-Path $runtimeRoot $entry.FullName))
            if (-not $destination.StartsWith($runtimeRoot + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or $entry.Length -gt 100MB) { throw 'Runtime resource exceeds its path/size boundary.' }
            New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
            if ((Test-Path -LiteralPath $destination -PathType Leaf) -and (Get-Item -LiteralPath $destination).Length -eq $entry.Length) {
                $existing = [IO.File]::Open($destination, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
                $packed = $null
                try {
                    $packed = $entry.Open()
                    if ([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($existing)) -ceq [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($packed))) { continue }
                } finally { if ($packed) { $packed.Dispose() }; $existing.Dispose() }
            }
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
    $env:LBA_BUILD_GIT_SHA = $helperSource
    if ($env:GITHUB_ENV -and -not $localRuntime) {
        @("NOVA_PROTECTED_BUILD_ID=$($identity.buildId)","NOVA_PROTECTED_MANIFEST_PUBLIC_KEY=$($identity.manifestPublicKey)","NOVA_PROTECTED_ENDPOINT=$($identity.endpoint)","NOVA_PROTECTED_RUNTIME_COMPATIBILITY=$($env:NOVA_PROTECTED_RUNTIME_COMPATIBILITY)","NOVA_PUBLIC_HELPER_INPUT_ID=$($checked.helperInputId)","CARGO_ENCODED_RUSTFLAGS=$($env:CARGO_ENCODED_RUSTFLAGS)","CARGO_PROFILE_RELEASE_STRIP=symbols","CARGO_PROFILE_RELEASE_DEBUG=0") | Add-Content -LiteralPath $env:GITHUB_ENV -Encoding utf8
    }
    $coreFeatures = if ($localRuntime) { 'public-beta,development-runtime' } else { 'public-beta' }
    & cargo build --locked --release --manifest-path tools/viewer-core/Cargo.toml --target-dir $coreTarget --no-default-features --features $coreFeatures --bin litematica_core --bin litematica_native_viewer --bin stockpile_server
    if ($LASTEXITCODE -ne 0) { throw 'Public Fast/core/stockpile compilation failed.' }
    $loaderArgs = @('build','--locked','--release','--manifest-path','tools/nova-protected-loader/Cargo.toml','--target-dir',$loaderTarget,'--bins')
    if ($localRuntime) { $loaderArgs += @('--features','development-runtime') }
    & cargo @loaderArgs
    if ($LASTEXITCODE -ne 0) { throw 'Public authenticated loader compilation failed.' }
    # The up-to-date shortcut in build.ps1 compares mtimes. A release/CI runner may
    # restore a cached target directory whose timestamps are newer than the freshly
    # checked-out sources, so never reuse the provider DLL there.
    if ($Force -or $env:GITHUB_ACTIONS -eq 'true') { & (Join-Path $repo 'tools/windows-thumbnail-provider/build.ps1') -Force }
    else { & (Join-Path $repo 'tools/windows-thumbnail-provider/build.ps1') }
    if ($LASTEXITCODE -ne 0) { throw 'The normal Windows thumbnail provider resource failed to build.' }
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    foreach ($existing in Get-ChildItem -LiteralPath $stage -Force) {
        if ($existing.PSIsContainer -or $existing.Name -notin ($backendNames + 'backend-deployment.json')) { throw "Unexpected file in generated public backend staging: $($existing.Name)" }
    }
    $sources = [ordered]@{
        'litematica_core.exe' = Join-Path $coreTarget 'release/litematica_core.exe'
        'litematica_native_viewer.exe' = Join-Path $coreTarget 'release/litematica_native_viewer.exe'
        'nova-render-service.exe' = Join-Path $loaderTarget 'release/nova-render-service.exe'
        'nova-build-worker.exe' = Join-Path $loaderTarget 'release/nova-build-worker.exe'
    }
    $artifacts = [ordered]@{}
    $restaged = 0
    foreach ($entry in $sources.GetEnumerator()) {
        $destination = Join-Path $stage $entry.Key
        if (Copy-IfChanged $entry.Value $destination) { $restaged++ }
        $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($hash -ne (Get-FileHash -LiteralPath $entry.Value -Algorithm SHA256).Hash.ToLowerInvariant()) { throw "Public helper staging is incomplete: $($entry.Key)" }
        $artifacts[$entry.Key] = [ordered]@{ bytes=(Get-Item -LiteralPath $destination).Length; sha256=$hash }
    }
    $deployment = [ordered]@{ schema='lba.backend.deployment.v1'; gitSha=$sha; helperSourceSha=$helperSource; helperInputId=$checked.helperInputId; profile='release'; runtimeKind=$(if ($localRuntime) {'development'} else {'production'}); runtimeBuildId=$identity.buildId; targetTriple='x86_64-pc-windows-msvc'; artifacts=$artifacts }
    $manifestWritten = Write-TextIfChanged (Join-Path $stage 'backend-deployment.json') ($deployment | ConvertTo-Json -Depth 5)
    if ($restaged -eq 0 -and -not $manifestWritten) {
        Write-Host "[protected-backend] staging already current; helper timestamps left untouched"
    }
    foreach ($entry in $resources.PSObject.Properties) {
        # build.rs owns generation AND validation of every flat PNG in this map.
        # A fresh checkout has the source archive, not the derived directory yet.
        if ($entry.Name -eq 'gen/block-icons') {
            if (-not (Test-Path -LiteralPath (Join-Path $repo 'pack-in/game-icons/block.zip') -PathType Leaf)) { throw 'Missing source archive for generated block icons.' }
            continue
        }
        if ($Development -and $entry.Name -eq '../../bin/protected') { continue }
        $source = Resolve-ProductResourceSource $tauriPath $entry.Name $entry.Value $stockpileSource
        if (-not (Test-Path -LiteralPath $source)) { throw "Missing required product resource: $($entry.Name)" }
    }
    if ((& git rev-parse HEAD).Trim() -ne $sha) { throw 'Public source commit changed during the candidate build.' }
    Write-Host "Public backend ready source=$sha protected_build=$($identity.buildId) helpers=4 restaged=$restaged staging=$stage"
} finally {
    # Do not make the Host claim the helper's older source SHA. Its own Git
    # identity follows the current UI checkout, independently of helper reuse.
    if ($Development) { Remove-Item Env:LBA_BUILD_GIT_SHA -ErrorAction SilentlyContinue }
    Pop-Location
}
