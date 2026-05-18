param(
    [string]$Root = "bin/stockpile-server",
    [string[]]$Target = @("all"),
    [switch]$Json
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$rootPath = if ([System.IO.Path]::IsPathRooted($Root)) {
    $Root
} else {
    Join-Path $repoRoot $Root
}

$allTargets = @(
    @{ Platform = "windows-x64"; File = "stockpile_server.exe" },
    @{ Platform = "linux-x64"; File = "stockpile_server" },
    @{ Platform = "macos-x64"; File = "stockpile_server" },
    @{ Platform = "macos-arm64"; File = "stockpile_server" }
)

$targetNames = @()
foreach ($entry in $Target) {
    foreach ($name in ($entry -split ",")) {
        $trimmed = $name.Trim()
        if ($trimmed.Length -gt 0) {
            $targetNames += $trimmed
        }
    }
}
if ($targetNames.Count -eq 0 -or $targetNames -contains "all") {
    $targetNames = @("windows-x64", "linux-x64", "macos-x64", "macos-arm64")
}
$validNames = @("windows-x64", "linux-x64", "macos-x64", "macos-arm64")
foreach ($name in $targetNames) {
    if ($validNames -notcontains $name) {
        throw "Invalid target '$name'. Use windows-x64, linux-x64, macos-x64, macos-arm64, or all."
    }
}

$required = @($allTargets | Where-Object { $targetNames -contains $_.Platform })

$results = foreach ($item in $required) {
    $path = Join-Path (Join-Path $rootPath $item.Platform) $item.File
    $exists = Test-Path -LiteralPath $path -PathType Leaf
    $length = if ($exists) { (Get-Item -LiteralPath $path).Length } else { 0 }
    [pscustomobject]@{
        platform = $item.Platform
        path = $path
        exists = $exists
        non_empty = ($exists -and $length -gt 0)
        length = $length
    }
}

$missing = @($results | Where-Object { -not $_.non_empty })

if ($Json) {
    [pscustomobject]@{
        root = $rootPath
        ok = ($missing.Count -eq 0)
        missing = @($missing | ForEach-Object { "$($_.platform)/$([System.IO.Path]::GetFileName($_.path))" })
        binaries = $results
    } | ConvertTo-Json -Depth 4
} else {
    Write-Host "stockpile_server binary root: $rootPath"
    foreach ($result in $results) {
        if ($result.non_empty) {
            Write-Host "[ok]      $($result.platform) -> $($result.path) ($($result.length) bytes)"
        } else {
            Write-Host "[missing] $($result.platform) -> $($result.path)"
        }
    }
}

if ($missing.Count -gt 0) {
    exit 1
}
