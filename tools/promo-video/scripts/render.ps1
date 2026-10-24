[CmdletBinding()]
param(
    [string]$ReleaseManifest = (Join-Path $PSScriptRoot '../output/release-manifest.json'),
    [ValidateSet('both', 'zh-CN', 'en-US')][string]$Language = 'both'
)
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
if (-not (Test-Path -LiteralPath $ReleaseManifest -PathType Leaf)) {
    throw '尚未登记正式发布结果；R_RELEASED=true 后再进入 V 阶段。'
}
$release = Get-Content -LiteralPath $ReleaseManifest -Raw -Encoding UTF8 | ConvertFrom-Json
if ($release.R_RELEASED -ne $true -or $release.version -ne '0.4.0' -or
    $release.release_url -notmatch '^https://github\.com/[^/]+/[^/]+/releases/tag/beta-0\.4\.0$' -or
    $release.video_content_ready -ne $true) {
    throw '需要已核实的 0.4.0 正式发布记录及 V 阶段完成的视频内容；S 阶段骨架不渲染。'
}
Push-Location $workspace
try {
    $targets = @(
        @{Language='zh-CN'; Composition='ZhIntro'; Filename='Nova-0.4-zh-CN.mp4'},
        @{Language='en-US'; Composition='EnIntro'; Filename='Nova-0.4-en-US.mp4'}
    )
    foreach ($target in $targets) {
        if ($Language -ne 'both' -and $Language -ne $target.Language) { continue }
        & npm exec --no -- remotion render src/index.ts $target.Composition (Join-Path 'output' $target.Filename) --codec=h264 --pixel-format=yuv420p
        if ($LASTEXITCODE -ne 0) { throw "渲染失败：$($target.Composition)（$LASTEXITCODE）" }
    }
} finally { Pop-Location }
