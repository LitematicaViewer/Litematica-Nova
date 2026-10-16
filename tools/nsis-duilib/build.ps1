[CmdletBinding()]
param([switch]$Force, [switch]$CheckOnly, [string]$NsisDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = $PSScriptRoot
$lock = Get-Content -LiteralPath (Join-Path $root 'dependencies.lock.json') -Raw | ConvertFrom-Json
$cache = Join-Path $root '.cache'
$output = Join-Path $root 'target/x86-unicode'
New-Item -ItemType Directory -Path $cache,$output -Force | Out-Null

if (-not $NsisDirectory) { $NsisDirectory = Join-Path $env:LOCALAPPDATA 'tauri/NSIS' }
$compiler = Join-Path $NsisDirectory 'makensis.exe'
if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) {
    # This is the exact dependency and digest used by the locked Tauri bundler.
    # Keep it project-local; do not modify the user's NSIS installation.
    $zip = Join-Path $cache 'nsis-3.11.zip'
    if (-not (Test-Path -LiteralPath $zip)) { Invoke-WebRequest 'https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip' -OutFile $zip }
    if ((Get-FileHash -LiteralPath $zip -Algorithm SHA1).Hash -ne 'EF7FF767E5CBD9EDD22ADD3A32C9B8F4500BB10D') { throw 'NSIS dependency digest mismatch' }
    Expand-Archive -LiteralPath $zip -DestinationPath $cache -Force
    $compiler = Join-Path $cache 'NSIS/makensis.exe'
}
$abi = (& $compiler /HDRINFO 2>&1) -join "`n"
if ($LASTEXITCODE -ne 0 -or $abi -notmatch 'NSIS_CPU=x86(?:,|\s)' -or $abi -notmatch 'NSIS_PTR_SIZE=4(?:,|\s)' -or $abi -notmatch 'NSIS_CHAR_SIZE=2(?:,|\s)') { throw 'The real NSIS host is not the required x86 Unicode ABI' }
Write-Host '[NovaDui] verified NSIS host: x86, pointer=4, Unicode char=2'

$archive = Join-Path $cache "duilib-$($lock.duilib.revision).zip"
if (-not (Test-Path -LiteralPath $archive)) { Invoke-WebRequest $lock.duilib.archive -OutFile $archive }
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $lock.duilib.sha256) { throw 'Duilib source digest mismatch' }
$source = Join-Path $cache "duilib-$($lock.duilib.revision)"
if (-not (Test-Path -LiteralPath (Join-Path $source 'DuiLib/UIlib.h'))) { Expand-Archive -LiteralPath $archive -DestinationPath $cache -Force }
$licenses = Join-Path $root '../../desktop-nova/src-tauri/installer/licenses'
foreach ($name in $lock.duilib.licenses) {
    $sourceLicense = Join-Path $source $name
    $bundledLicense = Join-Path $licenses ($name -replace ' ','-')
    if ((Get-FileHash -LiteralPath $sourceLicense).Hash -ne (Get-FileHash -LiteralPath $bundledLicense).Hash) { throw "Bundled license differs from locked Duilib source: $name" }
}
$template = Join-Path $root '../../desktop-nova/src-tauri/installer/upstream/installer.nsi'
if ((Get-FileHash -LiteralPath $template).Hash.ToLowerInvariant() -ne $lock.tauriTemplate.sha256) { throw 'Tauri template baseline changed without updating its lock' }
if ($CheckOnly) { Write-Host 'NOVA_DUILIB_INPUTS_OK'; return }

$dll = Join-Path $output 'NovaDui.dll'
$inputs = @(Get-ChildItem -LiteralPath $root -File | Where-Object { $_.Extension -in '.cpp','.h','.def','.json' -or $_.Name -eq 'build.ps1' })
$identityText = ($inputs | Sort-Object Name | ForEach-Object { "$($_.Name):$((Get-FileHash -LiteralPath $_.FullName).Hash)" }) -join "`n"
$receipt = Join-Path $output 'build-inputs.txt'
$outputReceipt = Join-Path $output 'build-output.sha256'
if (-not $Force -and (Test-Path -LiteralPath $dll) -and (Test-Path -LiteralPath $receipt) -and (Test-Path -LiteralPath $outputReceipt) -and
    (Get-Content -LiteralPath $receipt -Raw) -eq $identityText -and (Get-Content -LiteralPath $outputReceipt -Raw).Trim() -eq (Get-FileHash -LiteralPath $dll).Hash) {
    Write-Host '[NovaDui] identity-matched x86 Unicode plugin reused'
    return
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = (& $vswhere -products '*' -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1)
if (-not $vs) { throw 'Visual Studio C++ BuildTools are missing' }
$developer = Join-Path $vs 'Common7/Tools/VsDevCmd.bat'
$environment = & $env:ComSpec /d /s /c ('call "' + $developer + '" -no_logo -arch=x86 -host_arch=x64 && set')
if ($LASTEXITCODE -ne 0) { throw 'x86 C++ developer environment failed' }
foreach ($line in $environment) { if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') } }
$cl = Join-Path $env:VCToolsInstallDir 'bin/Hostx64/x86/cl.exe'
$dui = Join-Path $source 'DuiLib'
$thirdParty = @(Get-ChildItem -LiteralPath $dui -Recurse -Filter '*.cpp' -File | Where-Object Name -notin 'UIlib.cpp','UIFlash.cpp','UIWebBrowser.cpp','UIDlgBuilder.cpp')
# This adapter has no HTML/ActiveX browser and does not require the optional ATL
# component. Omit only its factory branch; keep the locked upstream tree intact.
$builder = Join-Path $cache 'UIDlgBuilder.cpp'
$gbk = [Text.Encoding]::GetEncoding(936)
$builderText = $gbk.GetString([IO.File]::ReadAllBytes((Join-Path $dui 'Core/UIDlgBuilder.cpp')))
$builderText = [regex]::Replace($builderText, '(?m)^.*DUI_CTR_WEBBROWSER.*(?:\r?\n|$)', '')
[IO.File]::WriteAllText($builder, $builderText, $gbk)
$flags = @('/nologo','/c','/O2','/MT','/EHsc','/W3','/DUNICODE','/D_UNICODE','/DUILIB_STATIC','/DWIN32_LEAN_AND_MEAN','/D_WIN32_WINNT=0x0601','/D_CRT_SECURE_NO_WARNINGS',"/I$dui",'/guard:cf','/GS')
Push-Location -LiteralPath $output
try {
    # The locked upstream sources use GBK, while our adapter is UTF-8. Compile
    # each with its real source encoding rather than silently rewriting it.
    & $cl @flags /std:c++14 /source-charset:.936 @($thirdParty | ForEach-Object FullName) $builder
    if ($LASTEXITCODE -ne 0) { throw "Duilib compilation failed: $LASTEXITCODE" }
    # The real upstream .vcxproj also compiles this C translation unit. It owns
    # stbi_load_from_memory/stbi_image_free used by the Duilib render engine.
    & $cl @flags /TC /source-charset:.936 (Join-Path $dui 'Utils/stb_image.c')
    if ($LASTEXITCODE -ne 0) { throw "Duilib image decoder compilation failed: $LASTEXITCODE" }
    & $cl @flags /std:c++17 /D_HAS_STD_BYTE=0 /DNOMINMAX /wd4828 /utf-8 (Join-Path $root 'ui.cpp') (Join-Path $root 'migration.cpp')
    if ($LASTEXITCODE -ne 0) { throw "NovaDui adapter compilation failed: $LASTEXITCODE" }
    $objects = @(Get-ChildItem -LiteralPath $output -Filter '*.obj' | ForEach-Object FullName)
    & (Join-Path $env:VCToolsInstallDir 'bin/Hostx64/x86/link.exe') /NOLOGO /DLL /MACHINE:X86 /DYNAMICBASE /NXCOMPAT /guard:cf "/DEF:$root/NovaDui.def" "/OUT:$dll" @objects user32.lib gdi32.lib comctl32.lib ole32.lib oleaut32.lib uuid.lib shell32.lib shlwapi.lib imm32.lib msimg32.lib winmm.lib gdiplus.lib version.lib msi.lib advapi32.lib psapi.lib
    if ($LASTEXITCODE -ne 0) { throw "NovaDui link failed: $LASTEXITCODE" }
    $dumpbin = Join-Path $env:VCToolsInstallDir 'bin/Hostx64/x86/dumpbin.exe'
    $headers = (& $dumpbin /headers $dll) -join "`n"
    if ($LASTEXITCODE -ne 0 -or $headers -notmatch '14C machine \(x86\)') { throw 'NovaDui PE ABI mismatch' }
    $dependencies = (& $dumpbin /dependents $dll) -join "`n"
    if ($LASTEXITCODE -ne 0 -or $dependencies -match '(?i)VCRUNTIME|MSVCP|ucrtbase') { throw 'NovaDui unexpectedly requires a redistributable C/C++ runtime' }
    [IO.File]::WriteAllText($receipt, $identityText, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText($outputReceipt, (Get-FileHash -LiteralPath $dll).Hash, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Join-Path $output 'abi.txt'), $abi + "`n" + $headers + "`n" + $dependencies, [Text.UTF8Encoding]::new($false))
    Write-Host 'NOVA_DUILIB_BUILD_OK'
    Get-FileHash -LiteralPath $dll -Algorithm SHA256 | Format-List
} finally { Pop-Location }
