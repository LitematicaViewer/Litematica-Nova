[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
& node (Join-Path $PSScriptRoot 'generate-audio.mjs')
if ($LASTEXITCODE -ne 0) { throw 'Original audio synthesis failed.' }
$audio = Join-Path $workspace 'assets/audio'
& ffmpeg -hide_banner -loglevel error -y -i (Join-Path $audio 'nova-original-score.wav') -af 'afade=t=in:d=0.35,afade=t=out:st=117:d=3,loudnorm=I=-20:TP=-3:LRA=9' -ar 48000 -c:a pcm_s16le (Join-Path $audio 'nova-original-score-master.wav')
if ($LASTEXITCODE -ne 0) { throw 'Original score mastering failed.' }
