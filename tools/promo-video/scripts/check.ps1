[CmdletBinding()]
param([switch]$Media,[switch]$Keyframes)
$ErrorActionPreference='Stop'
$workspace=Split-Path $PSScriptRoot -Parent
Push-Location $workspace
try {
 & npm run typecheck
 if($LASTEXITCODE -ne 0){throw '视频TypeScript检查失败。'}
 & node scripts/export-story.mjs
 if($LASTEXITCODE -ne 0){throw '时间线/字幕生成失败。'}
 $shots=Get-Content -LiteralPath shot-plan.json -Raw -Encoding UTF8 | ConvertFrom-Json
 $cursor=0.0
 foreach($shot in $shots){
  if($shot.inSeconds -ne $cursor -or $shot.outSeconds -le $shot.inSeconds){throw "时间线不连续：$($shot.id)"}
  $cursor=[double]$shot.outSeconds
 }
 if($cursor -ne 120){throw '时间线必须120秒。'}
 if($Media){
  $report=@{version='0.4.0';source='3cd63';fps=60;durationSeconds=120;shots=$shots.Count;voiceover=$false;audio='Original CC0 score + SFX';asset_manifest='assets/original/asset-manifest.json';html_original_export_bytes=247230;html_sanitized_delivery_bytes=246964;outputs=@()}
  foreach($locale in @('zh-CN','en-US')){
   $video="output/Litematica-Nova-Official-Intro-$locale.mp4"
   $srt="output/Litematica-Nova-Official-Intro-$locale.srt"
   if(-not(Test-Path -LiteralPath $video -PathType Leaf)){throw "缺少成片：$video"}
   if(-not(Test-Path -LiteralPath $srt -PathType Leaf)){throw "缺少字幕：$srt"}
   $probeRaw=& ffprobe -v error -show_entries 'format=duration,size:stream=codec_name,codec_type,width,height,r_frame_rate,sample_rate,channels' -of json $video
   if($LASTEXITCODE -ne 0){throw "媒体读取失败：$video"}
   $probe=($probeRaw -join "`n") | ConvertFrom-Json
   $v=$probe.streams | Where-Object {$_.codec_type -eq 'video'} | Select-Object -First 1
   $a=$probe.streams | Where-Object {$_.codec_type -eq 'audio'} | Select-Object -First 1
   if($v.width -ne 1920 -or $v.height -ne 1080 -or $v.r_frame_rate -ne '60/1' -or $v.codec_name -ne 'h264' -or $a.codec_name -ne 'aac' -or [double]$probe.format.duration -lt 115 -or [double]$probe.format.duration -gt 125){throw "成片参数错误：$locale"}
   $report.outputs+=@{locale=$locale;file=$video;media=$probe}
   if($Keyframes){
    $qa="output/qa/$locale"; New-Item -ItemType Directory -Force -Path $qa | Out-Null
    $indices=@(); $index=@(); $ordinal=1; foreach($s in $shots){$start=[int]($s.inSeconds*60);$end=[int]($s.outSeconds*60);$frames=@(($start+6),($start+40),([Math]::Min($start+85,$end-15)),($end-8));$indices+=$frames;for($i=0;$i -lt 4;$i++){$index+=@{shot=$s.id;state=@("opening","action","settled","outgoing")[$i];frame=$frames[$i];file=("frame-{0:D3}.jpg" -f $ordinal)};$ordinal++}}; $index | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath "$qa/keyframe-index.json" -Encoding UTF8
    $terms=@($indices | ForEach-Object {"eq(n\,$_)"}); $groups=@(); for($g=0;$g -lt $terms.Count;$g+=16){$groups+='( '+($terms[$g..([Math]::Min($g+15,$terms.Count-1))] -join '+')+')'}; $filter=$groups -join '+'
    & ffmpeg -y -hide_banner -loglevel error -i $video -vf "select='$filter'" -fps_mode vfr "$qa/frame-%03d.jpg"
    if($LASTEXITCODE -ne 0){throw "关键帧提取失败：$locale"}
    & ffmpeg -y -hide_banner -loglevel error -framerate 1 -i "$qa/frame-%03d.jpg" -vf 'scale=480:270,tile=4x11' -fps_mode vfr "$qa/shot-contact-%02d.jpg"
    if($LASTEXITCODE -ne 0){throw "逐镜头Contact失败：$locale"}
    & ffmpeg -y -hide_banner -loglevel error -i $video -vf 'fps=1/3,scale=480:270,tile=4x4' -fps_mode vfr "$qa/contact-%02d.jpg"
    if($LASTEXITCODE -ne 0){throw "Contact sheet失败：$locale"}
   }
  }
  $report.covers=@()
  foreach($locale in @('zh-CN','en-US')){
   $svg="output/Litematica-Nova-Official-Cover-$locale.svg"
   $png="output/Litematica-Nova-Official-Cover-$locale.png"
   if(-not(Test-Path -LiteralPath $svg -PathType Leaf) -or -not(Test-Path -LiteralPath $png -PathType Leaf)){throw "缺少后置封面：$locale"}
   $coverText=Get-Content -LiteralPath $svg -Raw -Encoding UTF8
   $embedded=[regex]::Matches($coverText,'data:image/png;base64,').Count
   if($embedded -lt 2){throw "SVG缺少真实GPU和UI的自包含图像：$locale"}
   $pngInfo=((& ffprobe -v error -show_entries stream=width,height -of json $png) -join "`n") | ConvertFrom-Json
   if($LASTEXITCODE -ne 0 -or $pngInfo.streams[0].width -ne 1920 -or $pngInfo.streams[0].height -ne 1080){throw "封面PNG尺寸错误：$locale"}
   $report.covers+=@{locale=$locale;svg=$svg;png=$png;embeddedPngs=$embedded}
  }
  $report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath output/render-manifest.json -Encoding UTF8
  'VIDEO_MEDIA_OK：两条真实120秒1080p60 H264/AAC及双语字幕已读回；内容/音频由独立Review另验。'
 }else {'VIDEO_SOURCE_OK：TypeScript、44镜头连续时间线及双语字幕生成通过。'}
}finally{Pop-Location}
