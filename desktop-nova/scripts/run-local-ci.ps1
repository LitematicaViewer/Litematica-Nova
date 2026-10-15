[CmdletBinding()]
param(
    [Parameter(Mandatory)] [ValidatePattern('^[a-f0-9]{40}$')] [string] $ExpectedSha,
    [Parameter(Mandatory)] [ValidateSet('Windows', 'Linux')] [string] $Platform,
    [string] $RepoRoot = $env:NOVA_LOCAL_REPOSITORY
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $RepoRoot) { throw 'The existing local repository path is not configured.' }
$repo = (Resolve-Path -LiteralPath $RepoRoot).Path
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:GITHUB_EVENT_NAME -ne 'workflow_dispatch' -or
    $env:GITHUB_REF -ne 'refs/heads/main' -or $env:GITHUB_REPOSITORY -ne 'LitematicaViewer/Litematica-Nova') {
    throw 'Local CI accepts a trusted manual public-main workflow only.'
}
$lock = $null
try {
    $lock = [IO.File]::Open('D:\NovaCI\runners\nova-build.lock', [IO.FileMode]::OpenOrCreate,
        [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    Push-Location -LiteralPath $repo
    try {
        $branch = (& git branch --show-current).Trim()
        if ($LASTEXITCODE -ne 0 -or $branch -ne 'main') { throw 'The existing workspace must already be on public main; CI will not switch it.' }
        $sha = (& git rev-parse --verify HEAD).Trim()
        if ($LASTEXITCODE -ne 0 -or $sha -ne $ExpectedSha) { throw 'The local source SHA does not match the requested workflow SHA.' }
        if (-not (& git rev-parse --show-toplevel).Trim().Equals($repo.Replace('\','/'), [StringComparison]::OrdinalIgnoreCase)) { throw 'The configured path must be the existing repository root.' }
        & git diff --quiet HEAD --
        if ($LASTEXITCODE -ne 0) { throw 'Tracked source changes must be committed before local CI.' }
        if ($Platform -eq 'Windows') {
            . 'D:\NovaCI\toolchains\windows\Activate.ps1'
            $env:CARGO_TARGET_DIR = Join-Path $repo 'desktop-nova/src-tauri/target/ci-windows-x64'
            if ($env:NOVA_WINDOWS_RUNTIME_PACKAGE_DIR) { $env:NOVA_RUNTIME_PACKAGE_DIR = $env:NOVA_WINDOWS_RUNTIME_PACKAGE_DIR }
            & node desktop-nova/scripts/build-public-platform.mjs --target x86_64-pc-windows-msvc --candidate
        } else {
            $linuxRepo = (& wsl.exe -d NovaCI-Debian -u nova-ci -- wslpath -a -u $repo).Trim()
            if ($LASTEXITCODE -ne 0 -or -not $linuxRepo) { throw 'Unable to resolve the existing repository in WSL.' }
            $script = @'
set -euo pipefail
cd "$1"
export GIT_CONFIG_COUNT=1
export GIT_CONFIG_KEY_0=core.autocrlf
export GIT_CONFIG_VALUE_0="$2"
export CARGO_TARGET_DIR="$PWD/desktop-nova/src-tauri/target/ci-linux-x64"
export NOVA_RUNTIME_PACKAGE_DIR=/mnt/d/NovaCI/runtime/x86_64-unknown-linux-gnu
node desktop-nova/scripts/build-public-platform.mjs --target x86_64-unknown-linux-gnu --candidate
'@
            $linuxFailure = $null
            $restoreFailure = $null
            $shellScript = Join-Path $env:RUNNER_TEMP ('nova-linux-' + [guid]::NewGuid().ToString('N') + '.sh')
            try {
                $gitAutocrlf = (& git config --get core.autocrlf | Out-String).Trim()
                if (-not $gitAutocrlf) { $gitAutocrlf = 'false' }
                if ($gitAutocrlf -notin @('true','false','input')) { throw 'The Windows checkout line-ending setting is invalid.' }
                [IO.File]::WriteAllText($shellScript, $script.Replace([string][char]13, ''), [Text.UTF8Encoding]::new($false))
                $linuxScript = (& wsl.exe -d NovaCI-Debian -u nova-ci -- wslpath -a -u $shellScript).Trim()
                if ($LASTEXITCODE -ne 0 -or -not $linuxScript) { throw 'Unable to resolve the CI shell script in WSL.' }
                & wsl.exe -d NovaCI-Debian -u nova-ci -- bash -l $linuxScript $linuxRepo $gitAutocrlf
                if ($LASTEXITCODE -ne 0) { throw "Linux candidate build failed with exit $LASTEXITCODE." }
            } catch { $linuxFailure = $_ }
            finally {
                if (Test-Path -LiteralPath $shellScript -PathType Leaf) { Remove-Item -LiteralPath $shellScript }
                try {
                    # WSL installs its own native Node dependencies. Restore the
                    # Windows developer environment in this same checkout.
                    . 'D:\NovaCI\toolchains\windows\Activate.ps1'
                    $node = (Get-Command node).Source
                    $npm = Join-Path (Split-Path -Parent $node) 'node_modules/npm/bin/npm-cli.js'
                    & $node $npm ci --prefix desktop-nova
                    if ($LASTEXITCODE -ne 0) { throw "Windows npm dependency restoration failed with exit $LASTEXITCODE." }
                } catch { $restoreFailure = $_ }
            }
            if ($linuxFailure) {
                if ($restoreFailure) { Write-Warning $restoreFailure.Exception.Message }
                throw $linuxFailure
            }
            if ($restoreFailure) { throw $restoreFailure }
        }
        if ($LASTEXITCODE -ne 0) { throw "$Platform candidate build failed with exit $LASTEXITCODE." }
        if ((& git rev-parse HEAD).Trim() -ne $ExpectedSha) { throw 'The source commit changed during the candidate build.' }
    } finally { Pop-Location }
} finally { if ($lock) { $lock.Dispose() } }
