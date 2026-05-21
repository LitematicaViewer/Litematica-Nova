param(
    [Parameter(Mandatory = $true)]
    [string]$Input,
    [Parameter(Mandatory = $true)]
    [string]$Output,
    [string]$MinecraftVersion = "1.21.10",
    [string]$CoreExe = "tools/viewer-core/target/release/litematica_core.exe",
    [switch]$AllowGuestReadonly,
    [switch]$AdminPageEnabled
)

$ErrorActionPreference = "Stop"

function Resolve-RepoRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}

$repoRoot = Resolve-RepoRoot
Push-Location $repoRoot
try {
    $targets = @(
        @{ Name = "windows-x64"; File = "stockpile_server.exe" },
        @{ Name = "linux-x64"; File = "stockpile_server" },
        @{ Name = "macos-x64"; File = "stockpile_server" },
        @{ Name = "macos-arm64"; File = "stockpile_server" }
    )
    $missing = @()
    foreach ($target in $targets) {
        $path = Join-Path "bin/stockpile-server" (Join-Path $target.Name $target.File)
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            $missing += "$($target.Name)/$($target.File)"
        }
    }
    if ($missing.Count -gt 0) {
        throw "Missing stockpile server artifacts: $($missing -join ', '). Place real binaries under bin/stockpile-server/<target>/ before offline target=all packaging."
    }
    if (-not (Test-Path -LiteralPath $CoreExe -PathType Leaf)) {
        throw "Missing local litematica_core executable: $CoreExe. Build it before going offline."
    }
    $access = Read-Host "Access password"
    $admin = Read-Host "Admin password"
    $readonly = if ($AllowGuestReadonly) { "true" } else { "false" }
    $adminPage = if ($AdminPageEnabled) { "true" } else { "false" }
    "$access`n$admin`n" | & $CoreExe stockpile export-zip `
        --input $Input `
        --output $Output `
        --minecraft-version $MinecraftVersion `
        --mode multi `
        --target all `
        --access-password-stdin `
        --admin-password-stdin `
        --allow-guest-readonly $readonly `
        --admin-page-enabled $adminPage
} finally {
    Pop-Location
}
