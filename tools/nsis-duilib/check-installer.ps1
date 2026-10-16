[CmdletBinding()]
param([switch]$Compile, [switch]$RunAbi, [string]$TauriConfigOverlay, [string]$GeneratedInstaller)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = $PSScriptRoot
$repo = [IO.Path]::GetFullPath((Join-Path $root '../..'))
$installer = Join-Path $repo 'desktop-nova/src-tauri/installer'
$template = Get-Content -LiteralPath (Join-Path $installer 'installer.nsi') -Raw
$hooks = Get-Content -LiteralPath (Join-Path $installer 'nsis-hooks.nsh') -Raw
$config = Get-Content -LiteralPath (Join-Path $repo 'desktop-nova/src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$nsis = @{}
foreach ($property in $config.bundle.windows.nsis.PSObject.Properties) { $nsis[$property.Name] = $property.Value }
if ($TauriConfigOverlay) {
    $overlay = Get-Content -LiteralPath $TauriConfigOverlay -Raw | ConvertFrom-Json
    if ($overlay.PSObject.Properties['identifier'] -and $overlay.identifier -ne $config.identifier) { throw 'The Windows overlay changes the stable application identifier' }
    if ($overlay.PSObject.Properties['bundle'] -and $overlay.bundle -and $overlay.bundle.PSObject.Properties['windows'] -and $overlay.bundle.windows -and $overlay.bundle.windows.PSObject.Properties['nsis']) {
        if (-not $overlay.bundle.windows.nsis) { throw 'The Windows overlay removes the production NSIS configuration' }
        foreach ($property in $overlay.bundle.windows.nsis.PSObject.Properties) { $nsis[$property.Name] = $property.Value }
    }
}
if ($config.identifier -ne 'com.litematica.nova' -or $nsis.template -ne 'installer/installer.nsi' -or $nsis.installerHooks -ne 'installer/nsis-hooks.nsh' -or $nsis.installMode -ne 'perMachine') { throw 'Windows production does not select the stable Nova installer' }
foreach ($pattern in @(
    '!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${BUNDLEID}"',
    '!define MANUPRODUCTKEY "${MANUKEY}\${BUNDLEID}"',
    '!define MUI_CUSTOMFUNCTION_GUIINIT NovaDuiGuiInit', '!define MUI_CUSTOMFUNCTION_UNGUIINIT un.NovaDuiGuiInit',
    'Function NovaDuiGuiInit', 'Function un.NovaDuiGuiInit', 'Function .onGUIEnd', 'Function un.onGUIEnd',
    '{{#each resources}}', '{{#each resources_ancestors}}', '!uninstfinalize', 'Section WebView2',
    'WriteUninstaller "$INSTDIR\uninstall.exe"', 'DeleteAppDataCheckboxState = 1', '$UpdateMode <> 1',
    'NovaDui::FindInstallLocation /NOUNLOAD'
)) { if (-not $template.Contains($pattern)) { throw "Missing installer contract: $pattern" } }
# Page macros generate the actual NSIS callbacks; direct definitions collide.
if ($template -match '(?m)^Function (?:un)?\.onGUIInit\s*$') { throw 'Duilib must chain through the official MUI2 GUIINIT hooks' }
if ($template.IndexOf('!define MUI_CUSTOMFUNCTION_GUIINIT') -gt $template.IndexOf('!insertmacro MUI_PAGE_WELCOME') -or
    $template.IndexOf('!define MUI_CUSTOMFUNCTION_UNGUIINIT') -gt $template.IndexOf('!insertmacro MUI_UNPAGE_CONFIRM')) { throw 'MUI2 GUIINIT hooks must be selected before page macros' }
if ($template.Contains('!insertmacro CheckIfAppIsRunning') -or $template.Contains('APP_ASSOCIATE') -or $template.Contains('APP_UNASSOCIATE')) { throw 'Generic force-kill / competing ProgID remains reachable' }
if ($template.IndexOf('!insertmacro NOVA_MIGRATE_EXISTING') -gt $template.IndexOf('Section WebView2')) { throw 'Legacy removal must precede every new application file write' }
foreach ($pattern in @('"MultiSelectModel" "Player"', '$\"%1$\" %*', 'NovaInstallerIdentity', 'NovaDui::RepairShell', 'NovaDui::Migrate', 'NSIS_PTR_SIZE', 'NSIS_CHAR_SIZE')) {
    if (-not $hooks.Contains($pattern)) { throw "Missing hook contract: $pattern" }
}
foreach ($script in @('build.ps1','check-installer.ps1')) {
    $tokens = $null; $errors = $null
    [Management.Automation.Language.Parser]::ParseFile((Join-Path $root $script), [ref]$tokens, [ref]$errors) | Out-Null
    if ($errors.Count) { throw ($errors | Out-String) }
}
Write-Host 'NOVA_INSTALLER_CONTRACT_OK'
if (-not $Compile -and -not $RunAbi -and -not $GeneratedInstaller) { return }

$compiler = Join-Path $env:LOCALAPPDATA 'tauri/NSIS/makensis.exe'
if (-not (Test-Path -LiteralPath $compiler)) { $compiler = Join-Path $root '.cache/NSIS/makensis.exe' }
$out = Join-Path $root 'target/check'
New-Item -ItemType Directory -Path $out -Force | Out-Null
if ($GeneratedInstaller) {
    # Compile the full Tauri-rendered script with its actual file/resource list.
    # Only OutFile is redirected; no installer executable is ever launched here.
    $generatedPath = (Resolve-Path -LiteralPath $GeneratedInstaller).Path
    $generated = Get-Content -LiteralPath $generatedPath -Raw -Encoding UTF8
    foreach ($pattern in @('!insertmacro MUI_PAGE_WELCOME', '!insertmacro MUI_UNPAGE_CONFIRM',
        '!define MUI_CUSTOMFUNCTION_GUIINIT NovaDuiGuiInit', '!define MUI_CUSTOMFUNCTION_UNGUIINIT un.NovaDuiGuiInit',
        'Function NovaDuiGuiInit', 'Function un.NovaDuiGuiInit', 'Section WebView2', 'WriteUninstaller')) {
        if (-not $generated.Contains($pattern)) { throw "Incomplete or stale production NSIS script: $pattern" }
    }
    if ($generated -match '(?m)^Function (?:un)?\.onGUIInit\s*$' -or $generated.Contains('{{')) { throw 'Production NSIS still contains a direct GUIINIT or unrendered template' }
    if ($generated -notmatch '(?m)^!define UNINSTALLERSIGNCOMMAND ""\r?$') { throw 'Compile-only validation requires the unsigned candidate, not a signing command' }
    $outputPath = Join-Path $out 'production-compile-only.exe'
    $outputDefine = '!define OUTFILE "' + $outputPath.Replace('$','$$') + '"'
    $outputLine = [regex]'(?m)^!define OUTFILE "[^"\r\n]*"\r?$'
    if ($outputLine.Matches($generated).Count -ne 1) { throw 'Production NSIS output definition is ambiguous' }
    $generated = $outputLine.Replace($generated, [Text.RegularExpressions.MatchEvaluator]{ param($match) $outputDefine })
    $compilePath = Join-Path $out 'production-compile-only.nsi'
    [IO.File]::WriteAllText($compilePath, $generated, [Text.UTF8Encoding]::new($true))
    Push-Location -LiteralPath (Split-Path -Parent $generatedPath)
    try { $compilerOutput = @(& $compiler /V2 /NOCD $compilePath 2>&1); $compilerExit = $LASTEXITCODE }
    finally { Pop-Location }
    $compilerOutput | ForEach-Object { Write-Host $_ }
    if ($compilerExit -ne 0 -or ($compilerOutput -join "`n") -match '(?im)warning\s+6000:') { throw "Full production NSIS compilation failed or contains unresolved variables: $compilerExit" }
    Write-Host 'NOVA_DUILIB_FULL_PRODUCTION_NSIS_COMPILE_OK'
    if (-not $RunAbi) { return }
}
$escapedInstaller = $installer.Replace('$','$$')
$escapedOut = $out.Replace('$','$$')
$fixture = @'
Unicode true
RequestExecutionLevel user
!include MUI2.nsh
!include LogicLib.nsh
!define PRODUCTNAME "Nova adapter compile fixture"
!define BUNDLEID "com.litematica.nova"
!define MAINBINARYNAME "Litematica-Nova"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${BUNDLEID}"
!include "__INSTALLER__\nsis-hooks.nsh"
Name "Nova adapter compile fixture - DO NOT INSTALL"
OutFile "__OUTPUT__\adapter-compile-only.exe"
InstallDir "$TEMP\Nova-adapter-compile-only"
!define MUI_CUSTOMFUNCTION_GUIINIT NovaDuiGuiInit
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.NovaDuiGuiInit
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Section
  !insertmacro NOVA_MIGRATE_EXISTING
  !insertmacro NOVA_CHECK_RUNNING
  WriteUninstaller "$INSTDIR\uninstall.exe"
  !insertmacro NSIS_HOOK_POSTINSTALL
SectionEnd
Section Uninstall
  !insertmacro NSIS_HOOK_PREUNINSTALL
  !insertmacro NSIS_HOOK_POSTUNINSTALL
SectionEnd
Function NovaDuiGuiInit
  NovaDui::Attach /NOUNLOAD "Nova adapter compile fixture"
  Pop $0
FunctionEnd
Function un.NovaDuiGuiInit
  NovaDui::Attach /NOUNLOAD "Nova adapter compile fixture"
  Pop $0
FunctionEnd
Function .onGUIEnd
  NovaDui::Detach
FunctionEnd
Function un.onGUIEnd
  NovaDui::Detach
FunctionEnd
'@
$fixture = $fixture.Replace('__INSTALLER__', $escapedInstaller).Replace('__OUTPUT__', $escapedOut)
$fixturePath = Join-Path $out 'adapter-compile-only.nsi'
[IO.File]::WriteAllText($fixturePath, $fixture, [Text.UTF8Encoding]::new($true))
$compilerOutput = @(& $compiler /V2 /NOCD $fixturePath 2>&1)
$compilerExit = $LASTEXITCODE
$compilerOutput | ForEach-Object { Write-Host $_ }
if ($compilerExit -ne 0 -or ($compilerOutput -join "`n") -match '(?im)warning\s+6000:') { throw "NSIS adapter compilation failed or contains unresolved variables: $compilerExit" }
Write-Host 'NOVA_DUILIB_NSIS_ADAPTER_COMPILE_OK'
# adapter-compile-only.exe is deliberately NEVER executed. It contains real
# migration hooks. The independent ABI host below has no registry/install code.
if (-not $RunAbi) { return }
$abiHost = @'
Unicode true
RequestExecutionLevel user
SilentInstall silent
!addplugindir /x86-unicode "__PLUGIN__"
Name "Nova ABI only"
OutFile "__OUTPUT__\abi-only.exe"
Section
  NovaDui::Abi
  Pop $0
  FileOpen $1 "__OUTPUT__\abi-result.txt" w
  FileWriteUTF16LE $1 $0
  FileClose $1
SectionEnd
'@
$abiHost = $abiHost.Replace('__PLUGIN__', (Join-Path $root 'target/x86-unicode')).Replace('__OUTPUT__', $escapedOut)
$abiPath = Join-Path $out 'abi-only.nsi'
[IO.File]::WriteAllText($abiPath, $abiHost, [Text.UTF8Encoding]::new($true))
$compilerOutput = @(& $compiler /V2 /NOCD $abiPath 2>&1)
$compilerExit = $LASTEXITCODE
$compilerOutput | ForEach-Object { Write-Host $_ }
if ($compilerExit -ne 0 -or ($compilerOutput -join "`n") -match '(?im)warning\s+6000:') { throw "ABI-only host compilation failed or contains unresolved variables: $compilerExit" }
$process = Start-Process -FilePath (Join-Path $out 'abi-only.exe') -ArgumentList '/S' -WindowStyle Hidden -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "ABI-only host failed: $($process.ExitCode)" }
$result = Get-Content -LiteralPath (Join-Path $out 'abi-result.txt') -Raw -Encoding Unicode
if ($result -ne 'NovaDui/1;x86;Unicode;pointer=4;wchar=2;static-crt;duilib=85658d47036445626f5a24d5287f348fff871604') { throw "Unexpected real NSIS plugin ABI result: $result" }
Write-Host $result
Write-Host 'NOVA_DUILIB_ABI_HOST_OK'
