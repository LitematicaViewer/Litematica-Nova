param(
    [string]$Repo,
    [string]$Workflow = "stockpile-server.yml",
    [string]$RunId,
    [string]$ReleaseTag,
    [string]$Out = "bin/stockpile-server",
    [string[]]$Target = @("all")
)

$ErrorActionPreference = "Stop"

function Resolve-RepoRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}

function Require-Gh {
    $gh = Get-Command gh -ErrorAction SilentlyContinue
    if (-not $gh) {
        throw "GitHub CLI 'gh' is required to download artifacts automatically. Install gh, authenticate with 'gh auth login', or download artifacts manually into bin/stockpile-server/<platform>/."
    }
}

function Infer-Repo {
    $remote = git remote get-url origin 2>$null
    if (-not $remote) {
        throw "Cannot infer GitHub repository from git remote. Pass -Repo owner/name."
    }
    if ($remote -match "github.com[:/](?<repo>[^/]+/[^/.]+)(\.git)?$") {
        return $Matches.repo
    }
    throw "Cannot infer GitHub repository from remote '$remote'. Pass -Repo owner/name."
}

function Copy-ArtifactFile {
    param(
        [string]$TempRoot,
        [string]$Platform,
        [string]$FileName,
        [string]$OutRoot
    )

    $expected = Join-Path (Join-Path (Join-Path $TempRoot "bin") "stockpile-server") (Join-Path $Platform $FileName)
    if (-not (Test-Path -LiteralPath $expected -PathType Leaf)) {
        $matches = @(Get-ChildItem -LiteralPath $TempRoot -Recurse -File -Filter $FileName | Where-Object {
            $_.FullName -match [regex]::Escape($Platform)
        })
        if ($matches.Count -eq 0) {
            $matches = @(Get-ChildItem -LiteralPath $TempRoot -Recurse -File -Filter $FileName)
        }
        if ($matches.Count -eq 1) {
            $expected = $matches[0].FullName
        } else {
            throw "Downloaded artifact for $Platform did not contain $FileName in an expected path."
        }
    }

    $targetDir = Join-Path $OutRoot $Platform
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
    Copy-Item -LiteralPath $expected -Destination (Join-Path $targetDir $FileName) -Force
}

$repoRoot = Resolve-RepoRoot
Push-Location $repoRoot
try {
    Require-Gh
    if (-not $Repo) {
        $Repo = Infer-Repo
    }

    $outRoot = if ([System.IO.Path]::IsPathRooted($Out)) {
        $Out
    } else {
        Join-Path $repoRoot $Out
    }
    New-Item -ItemType Directory -Force -Path $outRoot | Out-Null

    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("stockpile-server-artifacts-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null

    $artifacts = @(
        @{ Name = "stockpile-server-windows-x64"; Platform = "windows-x64"; File = "stockpile_server.exe" },
        @{ Name = "stockpile-server-linux-x64"; Platform = "linux-x64"; File = "stockpile_server" },
        @{ Name = "stockpile-server-macos-x64"; Platform = "macos-x64"; File = "stockpile_server" },
        @{ Name = "stockpile-server-macos-arm64"; Platform = "macos-arm64"; File = "stockpile_server" }
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
    $artifacts = @($artifacts | Where-Object { $targetNames -contains $_.Platform })

    if ($ReleaseTag) {
        foreach ($artifact in $artifacts) {
            gh release download $ReleaseTag --repo $Repo --pattern "$($artifact.Name)*" --dir $tempRoot
        }
        foreach ($zip in Get-ChildItem -LiteralPath $tempRoot -File -Filter "*.zip") {
            $expanded = Join-Path $tempRoot ([System.IO.Path]::GetFileNameWithoutExtension($zip.Name))
            New-Item -ItemType Directory -Force -Path $expanded | Out-Null
            Expand-Archive -LiteralPath $zip.FullName -DestinationPath $expanded -Force
        }
    } else {
        if (-not $RunId) {
            $RunId = (gh run list --repo $Repo --workflow $Workflow --status success --limit 1 --json databaseId --jq ".[0].databaseId")
            if (-not $RunId) {
                throw "No successful '$Workflow' run found for $Repo. Run the workflow first or pass -RunId."
            }
        }
        foreach ($artifact in $artifacts) {
            gh run download $RunId --repo $Repo --name $artifact.Name --dir $tempRoot
        }
    }

    foreach ($artifact in $artifacts) {
        Copy-ArtifactFile -TempRoot $tempRoot -Platform $artifact.Platform -FileName $artifact.File -OutRoot $outRoot
    }

    & (Join-Path $PSScriptRoot "verify_stockpile_server_bins.ps1") -Root $outRoot -Target $targetNames
} finally {
    Pop-Location
}
