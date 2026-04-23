param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$LitematicPath,

    [Parameter(Position = 1)]
    [ValidateSet("normal", "fast_experimental", "full")]
    [string]$DisplayMode = "full",

    [int]$ChunkSize = 32,

    [string]$ViewerPath = ""
)

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $ViewerPath) {
    $ViewerPath = Join-Path $scriptDir "..\bin\viewer-backend\litematica_native_viewer.exe"
}

$resolvedViewer = (Resolve-Path -LiteralPath $ViewerPath).Path
$resolvedLitematic = (Resolve-Path -LiteralPath $LitematicPath).Path

$arguments = @(
    $resolvedLitematic,
    "--chunk-size=$ChunkSize",
    "--display-mode=$DisplayMode"
)

$stdoutRedirect = [System.IO.Path]::GetTempFileName()
$stderrRedirect = [System.IO.Path]::GetTempFileName()

$process = Start-Process `
    -FilePath $resolvedViewer `
    -ArgumentList $arguments `
    -RedirectStandardOutput $stdoutRedirect `
    -RedirectStandardError $stderrRedirect `
    -PassThru

Write-Output "[fixture-preview] popup launched pid=$($process.Id) mode=$DisplayMode file=$resolvedLitematic"
