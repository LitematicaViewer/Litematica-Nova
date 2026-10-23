#Requires -Version 7.0
[CmdletBinding()]
param([string]$Jar, [string]$GameVersion = '26.3', [string]$AssetsDir, [string]$Output, [string]$Core, [switch]$Force)
$ErrorActionPreference = 'Stop'
$arguments = @((Join-Path $PSScriptRoot 'prepare-default-game-data.mjs'))
foreach ($entry in @(@('--jar', $Jar), @('--game-version', $GameVersion), @('--assets-dir', $AssetsDir), @('--output', $Output), @('--core', $Core))) {
    if ($entry[1]) { $arguments += $entry }
}
if ($Force) { $arguments += '--force' }
& node @arguments
if ($LASTEXITCODE -ne 0) { throw 'Default game data generation failed.' }
