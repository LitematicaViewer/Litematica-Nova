[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $RepoRoot,
    [ValidateSet("debug", "release")][string] $Profile = "release",
    [string] $GitSha,
    [string] $ViewerTargetDir,
    [string] $DesktopTargetDir,
    [switch] $CleanOnly
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Get-WorkspacePath([string] $Root, [string] $RelativePath) {
    $candidate = [IO.Path]::GetFullPath((Join-Path $Root $RelativePath))
    $prefix = $Root.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (-not $candidate.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw "Path outside repository: $candidate" }
    return $candidate
}
function Get-ArtifactRecord([string] $Path) {
    return [ordered]@{ bytes = [uint64](Get-Item -LiteralPath $Path).Length; sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
}
function Assert-PublicBinary([string] $Path) {
    $lines = @(& $Path --public-build-info)
    if ($LASTEXITCODE -ne 0) { throw "Public build identity rejected: $Path" }
    $info = ($lines -join "`n") | ConvertFrom-Json
    if ($info.schema -ne "nova.public.beta.build.v1" -or $info.flavor -ne "fast-only" -or $info.fullMode -ne $false -or $info.publicBeta -ne $true) {
        throw "Not an isolated public fast-only backend: $Path"
    }
}

$repo = (Resolve-Path -LiteralPath $RepoRoot).Path
$backendDir = Get-WorkspacePath $repo "bin/public-beta"
if ([string]::IsNullOrWhiteSpace($ViewerTargetDir)) { $ViewerTargetDir = "tools/viewer-core/target/public-beta" }
if ([string]::IsNullOrWhiteSpace($DesktopTargetDir)) { $DesktopTargetDir = "desktop-nova/src-tauri/target/public-beta" }
$viewerTarget = Join-Path (Get-WorkspacePath $repo $ViewerTargetDir) $Profile
$stagingDir = Get-WorkspacePath $repo "$DesktopTargetDir/$Profile/bin/public-beta"
if ($CleanOnly) {
    foreach ($path in @($backendDir, $stagingDir)) { if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force } }
    Write-Host "[public-backend] build-owned public staging removed"
    exit 0
}
if ([string]::IsNullOrWhiteSpace($GitSha)) { $GitSha = (& git -C $repo rev-parse --verify HEAD).Trim(); if ($LASTEXITCODE -ne 0) { throw "Git HEAD unavailable" } }
$GitSha = $GitSha.Trim().ToLowerInvariant()
if ($GitSha -notmatch '^(?:[0-9a-f]{40}|[0-9a-f]{64})$') { throw "Invalid Git SHA" }
$suffix = if ([Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([Runtime.InteropServices.OSPlatform]::Windows)) { ".exe" } else { "" }
$sources = [ordered]@{}
foreach ($name in @("litematica_core", "litematica_native_viewer")) {
    $binary = "$name$suffix"
    $source = Join-Path $viewerTarget $binary
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Missing isolated public backend: $source" }
    Assert-PublicBinary $source
    $sources[$binary] = $source
}
# Only clear exact public build-owned directories after both source identities pass.
# Neither bin/viewer-backend nor legacy target/release is read, modified or deployed.
foreach ($path in @($backendDir, $stagingDir)) {
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
    New-Item -ItemType Directory -Path $path -Force | Out-Null
}
$artifacts = [ordered]@{}
foreach ($entry in $sources.GetEnumerator()) {
    Copy-Item -LiteralPath $entry.Value -Destination (Join-Path $backendDir $entry.Key)
    $artifacts[$entry.Key] = Get-ArtifactRecord (Join-Path $backendDir $entry.Key)
}
$manifest = [ordered]@{ schema = "nova.public.beta.backends.v1"; publicBeta = $true; flavor = "fast-only"; gitSha = $GitSha; profile = $Profile; artifacts = $artifacts }
$manifestPath = Join-Path $backendDir "backend-deployment.json"
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
foreach ($name in @($sources.Keys) + @("backend-deployment.json")) { Copy-Item -LiteralPath (Join-Path $backendDir $name) -Destination (Join-Path $stagingDir $name) }
foreach ($name in $sources.Keys) {
    if ((Get-ArtifactRecord (Join-Path $stagingDir $name)).sha256 -ne $artifacts[$name].sha256) { throw "Staged public backend hash mismatch: $name" }
}
Write-Host "[public-backend] Ready flavor=fast-only gitSha=$GitSha profile=$Profile files=$($sources.Count + 1)"
