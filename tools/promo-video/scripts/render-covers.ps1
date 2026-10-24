[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$workspace=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$node=(Get-Command node -ErrorAction Stop).Source
Push-Location $workspace
try {
  & $node (Join-Path $workspace 'src/covers/generate-covers.mjs')
  if($LASTEXITCODE -ne 0){throw 'Official bilingual SVG cover rendering failed.'}
} finally {Pop-Location}
