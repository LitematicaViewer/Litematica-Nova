#Requires -Version 7.0
[CmdletBinding()]
param([Parameter(ValueFromRemainingArguments = $true)][string[]] $Arguments)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$desktop = Join-Path $repo 'desktop-nova'
$prepare = Join-Path $desktop 'scripts/prepare-protected-backend.ps1'
$mode = 'dev'
$force = $false
$tauriArguments = [Collections.Generic.List[string]]::new()
foreach ($argument in $Arguments) {
    switch ($argument) {
        '--check' { $mode = 'check' }
        '--prepare' { $mode = 'prepare' }
        '--skip-prepare' { $mode = 'reuse' }
        '--force-prepare' { $force = $true }
        { $_ -in '--help', '-h' } {
            Write-Host 'Usage: pwsh -NoProfile -File bin/dev-win.ps1 [--check|--prepare|--skip-prepare] [--force-prepare] [tauri dev arguments]'
            exit 0
        }
        default { $tauriArguments.Add($argument) }
    }
}
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
    $activation = if ($env:NOVA_TOOLCHAIN_ACTIVATE) { $env:NOVA_TOOLCHAIN_ACTIVATE } elseif (Test-Path -LiteralPath 'D:\NovaCI\toolchains\windows\Activate.ps1') { 'D:\NovaCI\toolchains\windows\Activate.ps1' } else { $null }
    if ($activation) { . $activation }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Rust MSVC cargo is missing from PATH.' }
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw 'Node.js 20.19 or newer is required.' }
    & node -e 'const [a,b]=process.versions.node.split(".").map(Number);if(a<20||(a===20&&b<19))process.exit(1)'
    if ($LASTEXITCODE -ne 0) { throw 'Node.js 20.19 or newer is required.' }
    if (-not $env:NOVA_DEVELOPMENT_READ_TOKEN -and (Test-Path -LiteralPath $tokenFile -PathType Leaf)) {
        $saved = (Get-Content -LiteralPath $tokenFile -Raw).Trim() | ConvertTo-SecureString
        $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($saved)
        try { $env:NOVA_DEVELOPMENT_READ_TOKEN = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer) }
        finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer); $saved.Dispose() }
    }
    $options = @{ RepoRoot = $repo; Development = $true; Force = $force }
    if ($mode -eq 'check') { $options.CheckOnly = $true }
    if ($mode -eq 'reuse') { $options.ReuseExisting = $true }
    try { & $prepare @options }
    catch {
        if ($mode -eq 'check' -or $_.Exception.Message -notmatch 'NOVA_RUNTIME_DOWNLOAD_AUTH_REQUIRED') { throw }
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
    if ($mode -eq 'check') { Write-Host '[dev-win] Signed runtime, ABI, capabilities and source inputs verified. No files written; no GUI started.'; exit 0 }
    $stage = Join-Path $repo 'bin/public-beta'
    $backend = Join-Path $repo 'bin/viewer-backend'
    foreach ($name in @('litematica_core.exe','litematica_native_viewer.exe','nova-render-service.exe','nova-build-worker.exe','backend-deployment.json')) {
        Copy-DevelopmentFile (Join-Path $stage $name) (Join-Path $backend $name)
    }
    $runtime = $env:NOVA_RUNTIME_PACKAGE_DIR
    if (-not $runtime) { throw 'The verified development runtime cache was not selected.' }
    foreach ($file in Get-ChildItem -LiteralPath $runtime -File) { Copy-DevelopmentFile $file.FullName (Join-Path $backend "protected/$($file.Name)") }
    # Both get_root() choices now read these exact cached bytes: the workspace
    # backend fallback, and Tauri's copied debug resources. Release config is
    # unchanged; the signed release package in bin/protected stays untouched.
    $overlayDirectory = Join-Path $repo '.tmp/development-runtime/windows-x64'
    New-Item -ItemType Directory -Path $overlayDirectory -Force | Out-Null
    $overlay = Join-Path $overlayDirectory 'tauri.json'
    $resourceMap = [ordered]@{ '../../bin/protected' = $null }
    $resourceMap[(Join-Path $backend 'protected')] = 'bin/viewer-backend/protected'
    $configuration = @{ bundle = @{ resources = $resourceMap } } | ConvertTo-Json -Depth 5
    if (-not (Test-Path -LiteralPath $overlay -PathType Leaf) -or [IO.File]::ReadAllText($overlay) -cne $configuration) {
        [IO.File]::WriteAllText($overlay, $configuration, [Text.UTF8Encoding]::new($false))
    }
    $stockpile = Join-Path $repo 'tools/viewer-core/target/public-beta/release/stockpile_server.exe'
    Copy-DevelopmentFile $stockpile (Join-Path $backend 'stockpile_server.exe')
    Copy-DevelopmentFile $stockpile (Join-Path $repo 'bin/stockpile-server/windows-x64/stockpile_server.exe')
    if ($mode -eq 'prepare') { Write-Host '[dev-win] Development runtime prepared. The UI was not started.'; exit 0 }
    if (-not $env:LBA_DATA_ROOT) { $env:LBA_DATA_ROOT = Join-Path $repo 'data' }
    Push-Location -LiteralPath $desktop
    try {
        if (-not (Test-Path -LiteralPath 'node_modules' -PathType Container)) {
            & npm.cmd ci
            if ($LASTEXITCODE -ne 0) { throw 'Frontend dependency installation failed.' }
        }
        Write-Host '[dev-win] Starting UI hot reload with the verified compatible runtime.'
        & npm.cmd run tauri -- dev --config $overlay @tauriArguments
        exit $LASTEXITCODE
    } finally { Pop-Location }
} catch { Write-Error $_; exit 1 }
finally { $env:NOVA_DEVELOPMENT_READ_TOKEN = $originalToken; Pop-Location }
