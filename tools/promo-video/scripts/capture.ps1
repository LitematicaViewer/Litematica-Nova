[CmdletBinding()]
param([string]$Repository=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path,[string]$SourceName='nova-observatory.litematic',[string]$OutputName='hero-standard.png')
# Standard-only fixed media capture through the published Service/Worker/Viewer.
# No desktop main program, user instance or frozen output is changed.
# For an authorized recapture choose a new OutputName; existing files are kept.
& (Join-Path $PSScriptRoot 'capture-standard.ps1') -Repository $Repository -SourceName $SourceName -OutputName $OutputName
if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
