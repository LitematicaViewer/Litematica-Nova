[CmdletBinding()]
param(
 [ValidateSet('final','proof','style')][string]$Mode='final',
 [ValidateSet('both','zh-CN','en-US')][string]$Language='both',
 [int]$Concurrency=8
)
$ErrorActionPreference='Stop'
$workspace=Split-Path $PSScriptRoot -Parent
$releaseFile=Join-Path $workspace 'output/release-manifest.json'
if(-not(Test-Path -LiteralPath $releaseFile -PathType Leaf)){throw '先完成正式 R 发布。'}
$release=Get-Content -LiteralPath $releaseFile -Raw -Encoding UTF8 | ConvertFrom-Json
if($release.R_RELEASED -ne $true -or $release.version -ne '0.4.0'){throw '只有 R_RELEASED=true 的正式0.4.0才能进入V。'}
if($Mode -eq 'final' -and $release.video_content_ready -ne $true){throw '先完成 styleframe / Proof-of-Style 内部放行。'}
Push-Location $workspace
try {
 if(-not(Test-Path -LiteralPath 'assets/audio/nova-original-score-master.wav')){& (Join-Path $PSScriptRoot 'generate-audio.ps1')}
 & node scripts/export-story.mjs
 if($LASTEXITCODE -ne 0){throw '时间线或字幕生成失败。'}
 if($Mode -eq 'style'){
  foreach($id in @('StyleHero','StyleFeature','StyleOutro')){
   & npm exec --no -- remotion still src/index.ts $id "output/$id.png" --image-format=png
   if($LASTEXITCODE -ne 0){throw "Styleframe失败：$id"}
  }
 } elseif($Mode -eq 'proof'){
  & npm exec --no -- remotion render src/index.ts NovaProof output/Proof-of-Style-10s.mp4 --codec=h264 --pixel-format=yuv420p --audio-codec=aac --audio-bitrate=192k --crf=18 --x264-preset=fast "--concurrency=$Concurrency"
  if($LASTEXITCODE -ne 0){throw 'Proof-of-Style失败。'}
 } else {
  foreach($locale in @('zh-CN','en-US')){
   if($Language -ne 'both' -and $Language -ne $locale){continue}
   $id=if($locale -eq 'zh-CN'){'NovaOfficialZh'}else{'NovaOfficialEn'}
   & npm exec --no -- remotion render src/index.ts $id "output/Litematica-Nova-Official-Intro-$locale.mp4" --codec=h264 --pixel-format=yuv420p --audio-codec=aac --audio-bitrate=192k --crf=18 --x264-preset=fast "--concurrency=$Concurrency"
   if($LASTEXITCODE -ne 0){throw "正式视频失败：$locale"}
  }
  if($Language -eq 'both'){
   $coverScript=Join-Path $PSScriptRoot 'render-covers.ps1'
   if(-not(Test-Path -LiteralPath $coverScript -PathType Leaf)){throw '两片已完成，缺少后置封面入口。'}
   & pwsh -NoProfile -File $coverScript
   if($LASTEXITCODE -ne 0){throw '后置封面生成失败。'}
   & pwsh -NoProfile -File (Join-Path $PSScriptRoot 'check.ps1') -Media -Keyframes
   if($LASTEXITCODE -ne 0){throw '最终媒体/封面检查失败。'}
  }
 }
} finally {Pop-Location}
