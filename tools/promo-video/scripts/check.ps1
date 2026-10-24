[CmdletBinding()]
param([switch]$Media)
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
Push-Location $workspace
try {
    foreach ($path in @('package-lock.json', 'src/index.ts', 'src/Root.tsx')) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "缺少文件：$path" }
    }
    & npm run typecheck
    if ($LASTEXITCODE -ne 0) { throw "TypeScript 检查失败（$LASTEXITCODE）" }
    foreach ($name in @('remotion-best-practices', 'remotion-create', 'remotion-markup', 'remotion-render', 'remotion-multimedia', 'remotion-captions', 'playwright-cli')) {
        $skill = Join-Path '.agents/skills' "$name/SKILL.md"
        if (-not (Test-Path -LiteralPath $skill -PathType Leaf)) { throw "缺少 Skill：$name" }
    }
    if ($Media) {
        foreach ($language in @('zh-CN', 'en-US')) {
            $video = Join-Path 'output' "Nova-0.4-$language.mp4"
            $subtitle = Join-Path 'output' "Nova-0.4-$language.srt"
            if (-not (Test-Path -LiteralPath $video -PathType Leaf)) { throw "缺少视频：$video" }
            if (-not (Test-Path -LiteralPath $subtitle -PathType Leaf)) { throw "缺少字幕：$subtitle" }
            & ffprobe -v error -show_entries 'format=duration:stream=codec_name,width,height,r_frame_rate' -of json $video
            if ($LASTEXITCODE -ne 0) { throw "媒体元数据读取失败：$video" }
        }
        Write-Output '媒体元数据和字幕文件已读取；画面、素材身份与关键帧需在 V 阶段另行验收。'
    } else {
        Write-Output 'S_TOOLING_OK：独立依赖、TypeScript 与七项官方 Skill 已就绪；未渲染视频。'
    }
} finally { Pop-Location }
