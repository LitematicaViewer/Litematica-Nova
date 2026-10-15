[CmdletBinding()]
param([switch]$Smoke, [switch]$Force)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$root = (Resolve-Path -LiteralPath $PSScriptRoot).Path
$target = [IO.Path]::GetFullPath((Join-Path $root "target/release"))
if (-not $target.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Build output leaves provider directory" }
New-Item -ItemType Directory -Path $target -Force | Out-Null

# Reuse an up-to-date DLL instead of paying a full cl.exe compile on every
# development launch. The check also skips the Visual Studio environment setup
# below, which costs about a second by itself. Pass -Force to rebuild anyway.
$dll = Join-Path $target "litematica_thumbnail.dll"
$smokeExe = Join-Path $target "thumbnail_smoke.exe"
$sourcePaths = @("thumbnail_provider.cpp", "thumbnail_smoke.cpp", "provider_abi.h", "litematica_thumbnail.def", "build.ps1") | ForEach-Object { Join-Path $root $_ }
$sourceItems = @(Get-Item -LiteralPath $sourcePaths -ErrorAction SilentlyContinue)
if (-not $Force -and $sourceItems.Count -gt 0 -and (Test-Path -LiteralPath $dll) -and (Test-Path -LiteralPath $smokeExe)) {
    $newestSource = ($sourceItems | Measure-Object -Property LastWriteTimeUtc -Maximum).Maximum
    if ((Get-Item -LiteralPath $dll).LastWriteTimeUtc -ge $newestSource -and (Get-Item -LiteralPath $smokeExe).LastWriteTimeUtc -ge $newestSource) {
        Write-Host "[thumbnail-provider] sources unchanged; reusing litematica_thumbnail.dll"
        Get-FileHash -LiteralPath $dll -Algorithm SHA256 | Format-List
        if ($Smoke) { & $smokeExe $dll; if ($LASTEXITCODE -ne 0) { throw "Thumbnail CLI smoke failed: $LASTEXITCODE" } }
        return
    }
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio/Installer/vswhere.exe"
if (-not (Test-Path -LiteralPath $vswhere)) { throw "vswhere is missing" }
$vs = (& $vswhere -products '*' -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1)
if (-not $vs) { throw "Visual Studio C++ BuildTools are missing" }
$developer = Join-Path $vs "Common7/Tools/VsDevCmd.bat"
$command = 'call "' + $developer + '" -no_logo -arch=x64 -host_arch=x64 && set'
$environment = & $env:ComSpec /d /s /c $command
if ($LASTEXITCODE -ne 0) { throw "C++ developer environment failed: $LASTEXITCODE" }
foreach ($line in $environment) { if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], "Process") } }
$cl = Join-Path $env:VCToolsInstallDir "bin/Hostx64/x64/cl.exe"
$flags = @('/nologo', '/std:c++17', '/O2', '/MT', '/EHsc', '/W4', '/WX', '/utf-8', '/DUNICODE', '/D_UNICODE', '/DWIN32_LEAN_AND_MEAN', '/DNOMINMAX', '/D_WIN32_WINNT=0x0601', '/guard:cf', '/GS', '/sdl')
& $cl @flags /LD (Join-Path $root 'thumbnail_provider.cpp') "/Fo$target/thumbnail_provider.obj" "/Fe$target/litematica_thumbnail.dll" /link "/DEF:$root/litematica_thumbnail.def" "/IMPLIB:$target/litematica_thumbnail.lib" /DYNAMICBASE /NXCOMPAT /guard:cf ole32.lib shell32.lib shlwapi.lib windowscodecs.lib bcrypt.lib gdi32.lib uuid.lib
if ($LASTEXITCODE -ne 0) { throw "Thumbnail DLL compilation failed: $LASTEXITCODE" }
& $cl @flags (Join-Path $root 'thumbnail_smoke.cpp') "/Fo$target/thumbnail_smoke.obj" "/Fe$target/thumbnail_smoke.exe" /link /DYNAMICBASE /NXCOMPAT /guard:cf ole32.lib shell32.lib shlwapi.lib windowscodecs.lib gdi32.lib uuid.lib
if ($LASTEXITCODE -ne 0) { throw "Thumbnail smoke compilation failed: $LASTEXITCODE" }
Get-FileHash -LiteralPath (Join-Path $target 'litematica_thumbnail.dll') -Algorithm SHA256 | Format-List
if ($Smoke) { & (Join-Path $target 'thumbnail_smoke.exe') (Join-Path $target 'litematica_thumbnail.dll'); if ($LASTEXITCODE -ne 0) { throw "Thumbnail CLI smoke failed: $LASTEXITCODE" } }
