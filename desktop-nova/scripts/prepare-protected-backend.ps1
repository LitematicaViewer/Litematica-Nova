[CmdletBinding()]
param(
    [string] $RepoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path,
    [switch] $Development,
    [switch] $CheckOnly,
    [switch] $ReuseExisting
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
    # Release builds keep their main-only gate. Public UI feature branches use
    # the same actual source graph/package checks, not a branch-name allowlist.
    $checkArgs = @((Join-Path $PSScriptRoot 'check-public-checkout.mjs'), '--repo', $repo)
    if ($Development) { $checkArgs += '--development' }
    if ($ReuseExisting) { $checkArgs += '--reuse-existing' }
    $checkedJson = & node @checkArgs
    if ($LASTEXITCODE -ne 0) { throw 'Public checkout/runtime preflight failed.' }
    $checked = ($checkedJson -join "`n") | ConvertFrom-Json
    $sha = $checked.sha
    $identity = $checked.identity
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
    foreach ($requiredInput in @('data/blockdb/blocks.dat.z', 'data/blockdb/plains-tints.dat.z', 'pack-in/game-icons/block.zip')) {
        if (-not (Test-Path -LiteralPath (Join-Path $repo $requiredInput) -PathType Leaf)) { throw "Missing required compile input: $requiredInput" }
    }
    if ($CheckOnly -or $ReuseExisting) {
        if ($ReuseExisting) {
            Write-Host "Existing backend verified source=$($checked.backendSource); UI source=$sha. No helper rebuild."
        } else {
            Write-Host "Public development inputs verified source=$sha. No files written; no build started."
        }
        return
    }
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
    $deployment = [ordered]@{ schema='lba.backend.deployment.v1'; gitSha=$sha; profile='release'; runtimeBuildId=$identity.buildId; artifacts=$artifacts }
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
    Pop-Location
}
