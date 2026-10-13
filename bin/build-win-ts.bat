@echo off
setlocal

REM ---------------------------------------------------------------------------
REM Litematica-Nova - Windows packaging (new Tauri + public-beta stack)
REM
REM Pipeline (mirrors .github/workflows/beta-public.yml):
REM   1. cargo build --locked --release --features public-beta  into
REM      tools/viewer-core/target/public-beta/release  (shared target dir).
REM      stockpile_server is included because tauri.conf.json declares it as a
REM      required bundle resource.
REM   2. desktop-nova/scripts/prepare-backend-deployment.ps1 -StageOnly writes the
REM      backend-deployment.json manifest and stages the host-visible backend.
REM   3. npm run check:layers   (layer boundary baseline)
REM   4. npm run tauri:build    (NSIS perMachine installer)
REM
REM Signing keys stay outside this script: createUpdaterArtifacts is left at its
REM product default and the installer is signed manually for release.
REM ---------------------------------------------------------------------------

set "SCRIPT_DIR=%~dp0"
set "REPO_ROOT=%SCRIPT_DIR%.."
set "DESKTOP_DIR=%REPO_ROOT%\desktop-nova"
set "VIEWER_CORE_DIR=%REPO_ROOT%\tools\viewer-core"
set "MIN_NODE_MAJOR=20"
set "MIN_NODE_MINOR=19"

echo [build-win-ts] Repo: %REPO_ROOT%

if not exist "%DESKTOP_DIR%\package.json" (
    echo [build-win-ts] Missing desktop-nova package.json.
    goto fail
)

if not exist "%VIEWER_CORE_DIR%\Cargo.toml" (
    echo [build-win-ts] Missing viewer-core Cargo.toml.
    goto fail
)

where cargo >nul 2>nul
if errorlevel 1 (
    echo [build-win-ts] cargo not found on PATH.
    goto fail
)

where node >nul 2>nul
if errorlevel 1 (
    echo [build-win-ts] node not found on PATH. Install Node.js %MIN_NODE_MAJOR%.%MIN_NODE_MINOR% or newer first.
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
    echo [build-win-ts] Node %NODE_MAJOR%.%NODE_MINOR% is too old. Nova needs %MIN_NODE_MAJOR%.%MIN_NODE_MINOR% or newer.
    goto fail
)
echo [build-win-ts] Node %NODE_MAJOR%.%NODE_MINOR% accepted.

REM --- 1. Build the public-beta Rust backend --------------------------------
echo [build-win-ts] Building Rust backend binaries ^(public-beta^)...
pushd "%VIEWER_CORE_DIR%" || goto fail
cargo build --locked --release --features public-beta --target-dir target/public-beta --bin litematica_core --bin litematica_native_viewer --bin stockpile_server
if errorlevel 1 (
    popd
    goto fail
)
popd

REM --- 2. Stage the public backend deployment -------------------------------
echo [build-win-ts] Preparing public backend deployment...
pushd "%REPO_ROOT%" || goto fail
powershell -NoProfile -ExecutionPolicy Bypass -File "%DESKTOP_DIR%\scripts\prepare-backend-deployment.ps1" -RepoRoot "%REPO_ROOT%" -Profile release -StageOnly
if errorlevel 1 (
    popd
    goto fail
)
popd

REM --- 3. Frontend dependencies and layer boundaries ------------------------
pushd "%DESKTOP_DIR%" || goto fail

if not exist "node_modules" (
    echo [build-win-ts] Installing frontend dependencies...
    call npm install
    if errorlevel 1 (
        popd
        goto fail
    )
)

echo [build-win-ts] Checking layer boundaries...
call npm run check:layers
if errorlevel 1 (
    popd
    goto fail
)

REM --- 4. Package the Tauri NSIS installer ----------------------------------
echo [build-win-ts] Building Tauri desktop app ^(NSIS^)...
call npm run tauri:build
if errorlevel 1 (
    popd
    goto fail
)
popd

echo.
echo [build-win-ts] Build completed. Installer is under desktop-nova\src-tauri\target\release\bundle\nsis\
echo [build-win-ts] Run bin\dev-win-ts.bat to launch the development window.
endlocal
exit /b 0

:fail
echo.
echo [build-win-ts] Build failed. See the messages above.
pause
endlocal
exit /b 1
