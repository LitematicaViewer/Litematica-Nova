@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM ===========================================================================
REM  Litematica-Nova - Windows development launcher (public tree)
REM
REM  This replaces the upstream scripts bin\dev-win-ts.bat / bin\build-win-ts.bat
REM  that no longer exist on main. The old launcher cannot work any more:
REM
REM    1. desktop-nova\src-tauri\build.rs validate_public_backends() asserts
REM       bin\public-beta contains EXACTLY these five entries:
REM         litematica_core.exe, litematica_native_viewer.exe,
REM         nova-render-service.exe, nova-build-worker.exe,
REM         backend-deployment.json
REM       and that the manifest lists exactly those four executables.
REM       (The old launcher staged only two of them, so it now fails the gate.)
REM    2. desktop-nova\src-tauri\src\main.rs initialize_application_host()
REM       initializes the Application Host with the same four executables,
REM       resolved from get_root()\bin\viewer-backend.
REM    3. tauri.conf.json maps  ../../bin/public-beta -> bin/viewer-backend and
REM       ../../bin/protected -> bin/viewer-backend/protected, and declares
REM       data\mc26\26.3 and litematica_thumbnail.dll as bundle resources.
REM    4. Only desktop-nova\scripts\prepare-protected-backend.ps1 builds the two
REM       authenticated loader backends and the thumbnail provider DLL, and
REM       unpacks pack-in\minecraft-runtime-26.3.zip into data\mc26\26.3.
REM       It also verifies the signed ciphertext package in bin\protected with
REM       its Ed25519 manifest before anything is staged.
REM
REM  This launcher therefore drives that reviewed script instead of duplicating
REM  its identity checks, then mirrors the resulting staging into
REM  bin\viewer-backend so a development build also finds the backends when the
REM  application resource root does not already provide bin\viewer-backend.
REM
REM  Usage:
REM    dev-win.cmd                 prepare, then start the Tauri dev window
REM    dev-win.cmd --prepare       stage everything, do not start the GUI
REM    dev-win.cmd --skip-prepare  start the GUI with the existing staging
REM    dev-win.cmd --check         read-only preflight report (no writes/builds)
REM    dev-win.cmd <tauri args>    forward extra arguments to "tauri dev",
REM                                e.g. dev-win.cmd --no-watch
REM
REM  Environment:
REM    LBA_DATA_ROOT  Defaults to <repo>\data so a development build keeps using
REM                   this checkout's data directory (enumerator collections,
REM                   projection library, user config) instead of a freshly
REM                   seeded one under src-tauri\target\debug\data.
REM                   Clear or override it to change that behaviour.
REM ===========================================================================

set "SCRIPT_DIR=%~dp0"
set "REPO_ROOT=%SCRIPT_DIR%.."
for %%I in ("%REPO_ROOT%") do set "REPO_ROOT=%%~fI"
set "DESKTOP_DIR=%REPO_ROOT%\desktop-nova"
set "TAURI_DIR=%DESKTOP_DIR%\src-tauri"
set "VIEWER_CORE_DIR=%REPO_ROOT%\tools\viewer-core"
set "LOADER_DIR=%REPO_ROOT%\tools\nova-protected-loader"
set "VIEWER_RELEASE_DIR=%VIEWER_CORE_DIR%\target\public-beta\release"
set "LOADER_RELEASE_DIR=%LOADER_DIR%\target\public-beta\release"
set "VIEWER_BACKEND_DIR=%REPO_ROOT%\bin\viewer-backend"
set "PUBLIC_BETA_DIR=%REPO_ROOT%\bin\public-beta"
set "PROTECTED_DIR=%REPO_ROOT%\bin\protected"
set "RUNTIME_ZIP=%REPO_ROOT%\pack-in\minecraft-runtime-26.3.zip"
set "STOCKPILE_DEST_DIR=%REPO_ROOT%\bin\stockpile-server\windows-x64"
set "PREPARE_SCRIPT=%DESKTOP_DIR%\scripts\prepare-protected-backend.ps1"
set "MIN_NODE_MAJOR=20"
set "MIN_NODE_MINOR=19"

set "MODE=dev"
set "TAURI_ARGS="
if /i "%~1"=="--check" set "MODE=check"
if /i "%~1"=="--prepare" set "MODE=prepare"
if /i "%~1"=="--skip-prepare" set "MODE=dev-noprepare"
if /i "%~1"=="--help" goto usage
if /i "%~1"=="-h" goto usage
REM Anything that is not a launcher switch is passed through to "tauri dev".
if "%MODE%"=="dev" if not "%~1"=="" set "TAURI_ARGS=%*"

echo [dev-win] repo: %REPO_ROOT%
echo [dev-win] mode: %MODE%
echo.

REM --- Repository shape ------------------------------------------------------
if not exist "%DESKTOP_DIR%\package.json" (
    echo [dev-win] FAIL desktop-nova\package.json is missing; run this from a full checkout.
    goto fail
)
if not exist "%VIEWER_CORE_DIR%\Cargo.toml" (
    echo [dev-win] FAIL tools\viewer-core\Cargo.toml is missing.
    goto fail
)
if not exist "%LOADER_DIR%\Cargo.toml" (
    echo [dev-win] FAIL tools\nova-protected-loader\Cargo.toml is missing.
    goto fail
)
if not exist "%PREPARE_SCRIPT%" (
    echo [dev-win] FAIL %PREPARE_SCRIPT% is missing.
    goto fail
)

REM --- Toolchain -------------------------------------------------------------
where cargo >nul 2>nul
if errorlevel 1 (
    echo [dev-win] FAIL cargo not found on PATH. Install the Rust MSVC toolchain first.
    goto fail
)
where node >nul 2>nul
if errorlevel 1 (
    echo [dev-win] FAIL node not found on PATH. Install Node.js %MIN_NODE_MAJOR%.%MIN_NODE_MINOR% or newer.
    goto fail
)
for /f "tokens=1,2 delims=." %%a in ('node --version') do (
    set "NODE_MAJOR=%%a"
    set "NODE_MINOR=%%b"
)
set "NODE_MAJOR=%NODE_MAJOR:v=%"
set /a NODE_OK=0
if %NODE_MAJOR% GTR %MIN_NODE_MAJOR% set /a NODE_OK=1
if %NODE_MAJOR% EQU %MIN_NODE_MAJOR% if %NODE_MINOR% GEQ %MIN_NODE_MINOR% set /a NODE_OK=1
if %NODE_OK% NEQ 1 (
    echo [dev-win] FAIL Node %NODE_MAJOR%.%NODE_MINOR% is too old; Nova needs %MIN_NODE_MAJOR%.%MIN_NODE_MINOR% or newer ^(vite 7 / react-router 7^).
    goto fail
)

REM --- PowerShell 7 ----------------------------------------------------------
REM prepare-protected-backend.ps1 needs PowerShell 7: it reads $IsWindows under
REM Set-StrictMode -Version Latest and uses [IO.Compression.ZipFile] without an
REM Add-Type, both of which fail on Windows PowerShell 5.1.
set "PWSH="
for %%I in (pwsh.exe) do if not defined PWSH set "PWSH=%%~$PATH:I"
if not defined PWSH if exist "%ProgramFiles%\PowerShell\7\pwsh.exe" set "PWSH=%ProgramFiles%\PowerShell\7\pwsh.exe"
if not defined PWSH if exist "%ProgramFiles%\PowerShell\7-preview\pwsh.exe" set "PWSH=%ProgramFiles%\PowerShell\7-preview\pwsh.exe"
if not defined PWSH for /d %%D in ("%USERPROFILE%\.dsh\profiles\*") do if not defined PWSH if exist "%%D\node_modules\@dsh-ops\pwsh-win32-x64\bin\pwsh.exe" set "PWSH=%%D\node_modules\@dsh-ops\pwsh-win32-x64\bin\pwsh.exe"
if not defined PWSH (
    echo [dev-win] FAIL PowerShell 7 ^(pwsh^) not found; prepare-protected-backend.ps1 cannot run on 5.1.
    echo [dev-win]      Install it with:  winget install --id Microsoft.PowerShell
    goto fail
)
echo [dev-win] pwsh: %PWSH%

REM --- Branch gate -----------------------------------------------------------
REM prepare-protected-backend.ps1 refuses any branch other than main, so report
REM that here instead of failing halfway through a long build.
for /f "delims=" %%B in ('git -C "%REPO_ROOT%" branch --show-current 2^>nul') do set "BRANCH=%%B"
if /i not "%BRANCH%"=="main" (
    echo [dev-win] FAIL current branch is "%BRANCH%"; the prepared backend refuses anything but main.
    echo [dev-win]      Run:  git -C "%REPO_ROOT%" checkout main
    goto fail
)

REM --- Inputs the preparation consumes --------------------------------------
call :need "%RUNTIME_ZIP%" "pack-in\minecraft-runtime-26.3.zip (unpacked into data\mc26\26.3)"
call :need "%PROTECTED_DIR%\protected-build.json" "bin\protected\protected-build.json (signed build identity)"
call :need "%PROTECTED_DIR%\nova_protected_module.dll.enc" "bin\protected\nova_protected_module.dll.enc (ciphertext module)"
call :need "%REPO_ROOT%\data\blockdb\blocks.dat.z" "data\blockdb\blocks.dat.z (embedded block database)"
call :need "%REPO_ROOT%\data\blockdb\plains-tints.dat.z" "data\blockdb\plains-tints.dat.z (tint sidecar)"
call :need "%REPO_ROOT%\pack-in\game-icons\block.zip" "pack-in\game-icons\block.zip (source for gen\block-icons)"
if defined MISSING goto fail

REM --- Preflight report ------------------------------------------------------
echo.
echo [dev-win] preflight OK
for /f "delims=" %%V in ('node --version') do echo [dev-win]   node       : %%V
for /f "delims=" %%V in ('cargo --version') do echo [dev-win]   cargo      : %%V
echo [dev-win]   backends   : litematica_core.exe, litematica_native_viewer.exe, nova-render-service.exe, nova-build-worker.exe
if defined LBA_DATA_ROOT (
    echo [dev-win]   data root  : %LBA_DATA_ROOT%
) else (
    echo [dev-win]   data root  : ^(default^) %REPO_ROOT%\data
)
echo [dev-win]   staging    : %PUBLIC_BETA_DIR% then mirrored to %VIEWER_BACKEND_DIR%
echo.

if /i "%MODE%"=="check" (
    echo [dev-win] --check only; nothing was written and nothing was built.
    echo [dev-win] Commands a real run would execute:
    echo [dev-win]   1. "%PWSH%" -NoProfile -File "%PREPARE_SCRIPT%" -RepoRoot "%REPO_ROOT%"
    echo [dev-win]   2. copy %PUBLIC_BETA_DIR%\ ^(5 entries^)  ^-^>  %VIEWER_BACKEND_DIR%
    echo [dev-win]   3. copy %PROTECTED_DIR%\*  ^-^>  %VIEWER_BACKEND_DIR%\protected\
    echo [dev-win]   4. copy %VIEWER_RELEASE_DIR%\stockpile_server.exe  ^-^>  %VIEWER_BACKEND_DIR%\ and %STOCKPILE_DEST_DIR%\
    echo [dev-win]   5. set LBA_DATA_ROOT=%REPO_ROOT%\data
    echo [dev-win]   6. cd /d "%DESKTOP_DIR%" ^&^& npm ci ^(only when node_modules is absent^) ^&^& npm run tauri:dev
    endlocal
    exit /b 0
)

REM --- Stage the reviewed public backend ------------------------------------
if /i "%MODE%"=="dev-noprepare" (
    echo [dev-win] --skip-prepare: reusing the existing staging.
    goto after_prepare
)

echo [dev-win] Preparing the reviewed public backend ^(release build; this takes a while^)...
echo [dev-win]   signature check + data\mc26 unpack + 4 helper builds + thumbnail provider
"%PWSH%" -NoProfile -File "%PREPARE_SCRIPT%" -RepoRoot "%REPO_ROOT%"
if errorlevel 1 (
    echo [dev-win] FAIL prepare-protected-backend.ps1 failed; see its output above.
    goto fail
)

REM --- Mirror into the development backend directory -------------------------
REM main.rs backend_exe_path() reads get_root()\bin\viewer-backend. In a bundled
REM build tauri.conf.json creates that directory from bin\public-beta; a
REM development build that resolves get_root() to this checkout needs the same
REM files here, plus the protected package the resource map expects.
echo [dev-win] Mirroring staging into bin\viewer-backend...
if not exist "%VIEWER_BACKEND_DIR%" mkdir "%VIEWER_BACKEND_DIR%"
copy /Y "%PUBLIC_BETA_DIR%\litematica_core.exe" "%VIEWER_BACKEND_DIR%\" >nul
if errorlevel 1 goto mirror_fail
copy /Y "%PUBLIC_BETA_DIR%\litematica_native_viewer.exe" "%VIEWER_BACKEND_DIR%\" >nul
if errorlevel 1 goto mirror_fail
copy /Y "%PUBLIC_BETA_DIR%\nova-render-service.exe" "%VIEWER_BACKEND_DIR%\" >nul
if errorlevel 1 goto mirror_fail
copy /Y "%PUBLIC_BETA_DIR%\nova-build-worker.exe" "%VIEWER_BACKEND_DIR%\" >nul
if errorlevel 1 goto mirror_fail
copy /Y "%PUBLIC_BETA_DIR%\backend-deployment.json" "%VIEWER_BACKEND_DIR%\" >nul
if errorlevel 1 goto mirror_fail
if exist "%VIEWER_RELEASE_DIR%\stockpile_server.exe" (
    copy /Y "%VIEWER_RELEASE_DIR%\stockpile_server.exe" "%VIEWER_BACKEND_DIR%\" >nul
    if not exist "%STOCKPILE_DEST_DIR%" mkdir "%STOCKPILE_DEST_DIR%"
    copy /Y "%VIEWER_RELEASE_DIR%\stockpile_server.exe" "%STOCKPILE_DEST_DIR%\" >nul
)
robocopy "%PROTECTED_DIR%" "%VIEWER_BACKEND_DIR%\protected" /MIR /NFL /NDL /NJH /NJS /NP >nul
if errorlevel 8 goto mirror_fail
echo [dev-win] Backend staging ready.
if /i "%MODE%"=="prepare" (
    echo.
    echo [dev-win] --prepare done; the GUI was not started.
    endlocal
    exit /b 0
)
goto after_prepare

:mirror_fail
echo [dev-win] FAIL could not mirror the staging into %VIEWER_BACKEND_DIR%.
goto fail

:after_prepare
REM --- Development data root -------------------------------------------------
REM data_root() uses LBA_DATA_ROOT in debug builds, so an absolute value pins the
REM enumerator collections, user config and projection library to this checkout
REM instead of a freshly seeded copy under the Tauri target directory.
if not defined LBA_DATA_ROOT set "LBA_DATA_ROOT=%REPO_ROOT%\data"
echo [dev-win] LBA_DATA_ROOT=%LBA_DATA_ROOT%

REM --- Frontend dependencies -------------------------------------------------
pushd "%DESKTOP_DIR%" || goto fail
if not exist "node_modules" (
    echo [dev-win] Installing frontend dependencies with npm ci...
    call npm ci
    if errorlevel 1 (
        popd
        echo [dev-win] FAIL npm ci failed.
        goto fail
    )
)

REM --- Pre-build the debug host ----------------------------------------------
REM desktop-nova\vite.config.ts ignores only "**/tauri/**" while the real
REM directory is "src-tauri", so Vite's watcher also watches
REM src-tauri\target\debug\deps\*.exe. Linking that file while the watcher is
REM attached fails with EBUSY and kills the whole dev session. Building the host
REM once here means the watcher sees a finished binary instead of a moving one.
echo [dev-win] Pre-building the debug host ^(avoids the Vite watcher/linker race^)...
cargo build --manifest-path "%TAURI_DIR%\Cargo.toml"
if errorlevel 1 (
    popd
    echo [dev-win] FAIL debug host build failed.
    goto fail
)

echo [dev-win] Starting the Tauri development window ^(vite on 127.0.0.1:1430^)...
if defined TAURI_ARGS (
    echo [dev-win] Extra tauri dev arguments: %TAURI_ARGS%
    call npm run tauri:dev -- %TAURI_ARGS%
) else (
    call npm run tauri:dev
)
set "EXIT_CODE=%ERRORLEVEL%"
popd
echo.
echo [dev-win] Tauri dev exited with code %EXIT_CODE%.
endlocal & exit /b %EXIT_CODE%

:usage
echo Usage:
echo   dev-win.cmd                 prepare, then start the Tauri dev window
echo   dev-win.cmd --prepare       stage everything, do not start the GUI
echo   dev-win.cmd --skip-prepare  start the GUI with the existing staging
echo   dev-win.cmd --check         read-only preflight report
echo   dev-win.cmd ^<tauri args^>     forward extra arguments to "tauri dev"
echo                               e.g. dev-win.cmd --no-watch
endlocal
exit /b 0

:need
if exist %1 goto need_ok
echo [dev-win] MISS %~2
set "MISSING=1"
exit /b 0

:need_ok
echo [dev-win] OK   %~2
exit /b 0

:fail
echo.
echo [dev-win] Startup aborted. See the messages above.
endlocal
exit /b 1
