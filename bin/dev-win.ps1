#Requires -Version 7.0
[CmdletBinding()]
param([Parameter(ValueFromRemainingArguments = $true)][string[]] $Arguments)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Arguments = @($Arguments | Where-Object { $null -ne $_ })
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$desktop = Join-Path $repo 'desktop-nova'
$prepare = Join-Path $desktop 'scripts/prepare-protected-backend.ps1'
$mode = 'dev'
$force = $false
$runtimeSource = if ($env:NOVA_RUNTIME_SOURCE) { $env:NOVA_RUNTIME_SOURCE } else { 'community' }
$privateSource = $env:NOVA_PRIVATE_SOURCE_DIR
$tauriArguments = [Collections.Generic.List[string]]::new()
for ($i = 0; $i -lt $Arguments.Count; $i++) {
    $argument = $Arguments[$i]
    switch ($argument) {
        '--check' { $mode = 'check' }
        '--prepare' { $mode = 'prepare' }
        '--skip-prepare' { $mode = 'reuse' }
        '--force-prepare' { $force = $true }
        '--runtime-source' { if (++$i -ge $Arguments.Count) { throw '--runtime-source requires community, prebuilt or local.' }; $runtimeSource = $Arguments[$i] }
        '--private-source' { if (++$i -ge $Arguments.Count) { throw '--private-source requires an authorized private source directory.' }; $privateSource = $Arguments[$i] }
        { $_ -in '--help', '-h' } {
            Write-Host 'Usage: pwsh -NoProfile -File bin/dev-win.ps1 [--runtime-source community|prebuilt|local] [--private-source directory] [--check|--prepare|--skip-prepare] [tauri dev arguments]'
            exit 0
        }
        default { $tauriArguments.Add($argument) }
    }
}
if ($runtimeSource -notin @('community','prebuilt','local')) { throw '--runtime-source must be community, prebuilt or local.' }
function Copy-DevelopmentFile([string] $Source, [string] $Destination) {
    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) { throw "Required development component is missing: $([IO.Path]::GetFileName($Source))" }
    if ((Test-Path -LiteralPath $Destination -PathType Leaf) -and (Get-Item -LiteralPath $Source).Length -eq (Get-Item -LiteralPath $Destination).Length -and (Get-FileHash -LiteralPath $Source).Hash -eq (Get-FileHash -LiteralPath $Destination).Hash) { return }
    New-Item -ItemType Directory -Path (Split-Path -Parent $Destination) -Force | Out-Null
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
}
$tokenFile = Join-Path $env:LOCALAPPDATA 'Litematica-Nova/development-runtime-token.dpapi'
$originalToken = $env:NOVA_DEVELOPMENT_READ_TOKEN
Push-Location -LiteralPath $repo
try {
    $activation = $env:NOVA_TOOLCHAIN_ACTIVATE
    if ($activation) { . $activation }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Rust MSVC cargo is missing from PATH.' }
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw 'Node.js 20.19 or newer is required.' }
    & node -e 'const [a,b]=process.versions.node.split(".").map(Number);if(a<20||(a===20&&b<19))process.exit(1)'
    if ($LASTEXITCODE -ne 0) { throw 'Node.js 20.19 or newer is required.' }
    if ($runtimeSource -eq 'prebuilt' -and -not $env:NOVA_DEVELOPMENT_READ_TOKEN -and (Test-Path -LiteralPath $tokenFile -PathType Leaf)) {
        $saved = (Get-Content -LiteralPath $tokenFile -Raw).Trim() | ConvertTo-SecureString
        $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($saved)
        try { $env:NOVA_DEVELOPMENT_READ_TOKEN = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer) }
        finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer); $saved.Dispose() }
    }
    $options = @{ RepoRoot = $repo; Development = $true; Force = $force; RuntimeSource = $runtimeSource }
    if ($privateSource) { $options.PrivateSourceDir = $privateSource }
    if ($mode -eq 'check') { $options.CheckOnly = $true }
    if ($mode -eq 'reuse') { $options.ReuseExisting = $true }
    try { & $prepare @options }
    catch {
        if ($runtimeSource -ne 'prebuilt' -or $mode -eq 'check' -or $_.Exception.Message -notmatch 'NOVA_RUNTIME_DOWNLOAD_AUTH_REQUIRED') { throw }
        $secret = Read-Host 'Development runtime read-only token (not a decryption/signing key)' -AsSecureString
        if ($secret.Length -eq 0) { $secret.Dispose(); throw 'A read-only runtime credential is required for the configured channel.' }
        $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secret)
        try {
            $env:NOVA_DEVELOPMENT_READ_TOKEN = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)
            & $prepare @options
            New-Item -ItemType Directory -Path (Split-Path -Parent $tokenFile) -Force | Out-Null
            $secret | ConvertFrom-SecureString | Set-Content -LiteralPath $tokenFile -Encoding utf8
        } finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer); $secret.Dispose() }
    }
    if ($mode -eq 'check') { Write-Host "[dev-win] $runtimeSource runtime, ABI, capabilities and source inputs verified. No files written; no GUI started."; exit 0 }
    $stage = Join-Path $repo 'bin/public-beta'
    $env:NOVA_PUBLIC_BACKEND_DIR = $stage
    $backend = Join-Path $repo 'bin/viewer-backend'
    $names = @('litematica_core.exe','litematica_native_viewer.exe','backend-deployment.json')
    if ($runtimeSource -ne 'community') { $names += @('nova-render-service.exe','nova-build-worker.exe') }
    foreach ($name in $names) {
        Copy-DevelopmentFile (Join-Path $stage $name) (Join-Path $backend $name)
    }
    $runtime = $env:NOVA_RUNTIME_PACKAGE_DIR
    if ($runtimeSource -ne 'community' -and -not $runtime) { throw 'The verified development runtime cache was not selected.' }
    if ($runtimeSource -eq 'prebuilt') {
        foreach ($file in Get-ChildItem -LiteralPath $runtime -File) { Copy-DevelopmentFile $file.FullName (Join-Path $backend "protected/$($file.Name)") }
    }
    $helperRoot = if ($env:NOVA_PUBLIC_CARGO_CACHE_ROOT) { [IO.Path]::GetFullPath($env:NOVA_PUBLIC_CARGO_CACHE_ROOT) } else { $repo }
    $stockpile = Join-Path $helperRoot 'tools/viewer-core/target/public-beta/release/stockpile_server.exe'
    $stockpileResource = Join-Path $repo 'bin/stockpile-server/windows-x64/stockpile_server.exe'
    Copy-DevelopmentFile $stockpile (Join-Path $backend 'stockpile_server.exe')
    Copy-DevelopmentFile $stockpile $stockpileResource
    # Both get_root() choices now read these exact cached bytes: the workspace
    # backend fallback, and Tauri's copied debug resources. Release config is
    # unchanged; the signed release package in bin/protected stays untouched.
    $overlayDirectory = Join-Path $repo '.tmp/development-runtime/windows-x64'
    New-Item -ItemType Directory -Path $overlayDirectory -Force | Out-Null
    $overlay = Join-Path $overlayDirectory 'tauri.json'
    $resourceMap = [ordered]@{ '../../bin/protected' = $null }
    if ($runtimeSource -eq 'prebuilt') {
        $resourceMap[(Join-Path $repo 'licenses/AMICE-APACHE-2.0.txt')] = 'licenses/AMICE-APACHE-2.0.txt'
        $resourceMap[(Join-Path $repo 'licenses/PROTECTED-MODULE.txt')] = 'licenses/PROTECTED-MODULE.txt'
    }
    if ($runtimeSource -eq 'prebuilt') { $resourceMap[(Join-Path $backend 'protected')] = 'bin/viewer-backend/protected' }
    $resourceMap['../../tools/viewer-core/target/public-beta/release/stockpile_server.exe'] = $null
    $resourceMap[$stockpileResource] = 'bin/stockpile-server/windows-x64/stockpile_server.exe'
    $configuration = @{ bundle = @{ resources = $resourceMap; createUpdaterArtifacts = $false } } | ConvertTo-Json -Depth 5
    if (-not (Test-Path -LiteralPath $overlay -PathType Leaf) -or [IO.File]::ReadAllText($overlay) -cne $configuration) {
        [IO.File]::WriteAllText($overlay, $configuration, [Text.UTF8Encoding]::new($false))
    }
    if ($mode -eq 'prepare') { Write-Host '[dev-win] Development runtime prepared. The UI was not started.'; exit 0 }
    if (-not $env:LBA_DATA_ROOT) { $env:LBA_DATA_ROOT = Join-Path $repo 'data' }
    Push-Location -LiteralPath $desktop
    try {
        if (-not (Test-Path -LiteralPath 'node_modules' -PathType Container)) {
            & npm.cmd ci
            if ($LASTEXITCODE -ne 0) { throw 'Frontend dependency installation failed.' }
        }
        Write-Host '[dev-win] Starting UI hot reload with the verified compatible runtime.'
        $features = if ($runtimeSource -eq 'local') { @('--features','development-runtime') } else { @() }
        & npm.cmd run tauri -- dev --config $overlay @features @tauriArguments
        exit $LASTEXITCODE
    } finally { Pop-Location }
} catch { Write-Error $_; exit 1 }
finally { $env:NOVA_DEVELOPMENT_READ_TOKEN = $originalToken; Pop-Location }
