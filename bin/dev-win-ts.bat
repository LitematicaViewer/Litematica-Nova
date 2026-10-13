@echo off
setlocal

REM ---------------------------------------------------------------------------
REM Litematica-Nova - Windows development launcher (new Tauri + public-beta stack)
REM
REM Constraints that come from the merged public-beta architecture:
REM   1. The Rust backend must be built with the public-beta feature:
REM      viewer-core/src/lib.rs has a compile_error! gate without it.
REM   2. Backend artifacts belong in tools/viewer-core/target/public-beta/release,
REM      the shared target dir used by tauri.conf.json and
REM      prepare-backend-deployment.ps1.
REM   3. tauri.conf.json declares bundle resources that must exist before Tauri
REM      runs, otherwise tauri dev/build aborts with
REM      "resource path ... doesn't exist":
REM         tools/viewer-core/target/public-beta/release/stockpile_server.exe
REM         bin/public-beta
REM   4. The frontend needs Node >= 20.19 (vite 7 / react-router 7). Older Node
REM      is rejected here instead of failing later inside vite.
REM   5. bin/public-beta holds public-beta binaries only. A development build runs
REM      the legacy host path (bin/viewer-backend), so this directory is created
REM      as an empty placeholder and is never committed (see .gitignore).
REM ---------------------------------------------------------------------------

set "SCRIPT_DIR=%~dp0"
set "REPO_ROOT=%SCRIPT_DIR%.."
set "DESKTOP_DIR=%REPO_ROOT%\desktop-nova"
set "VIEWER_CORE_DIR=%REPO_ROOT%\tools\viewer-core"
set "VIEWER_RELEASE_DIR=%VIEWER_CORE_DIR%\target\public-beta\release"
set "VIEWER_BACKEND_DIR=%REPO_ROOT%\bin\viewer-backend"
set "PUBLIC_BETA_DIR=%REPO_ROOT%\bin\public-beta"
set "MIN_NODE_MAJOR=20"
set "MIN_NODE_MINOR=19"

echo [dev-win-ts] Repo: %REPO_ROOT%

if not exist "%DESKTOP_DIR%\package.json" (
    echo [dev-win-ts] Missing desktop-nova package.json.
    goto fail
)

if not exist "%VIEWER_CORE_DIR%\Cargo.toml" (
    echo [dev-win-ts] Missing viewer-core Cargo.toml.
    goto fail
)

REM --- Toolchain checks ------------------------------------------------------
where cargo >nul 2>nul
if errorlevel 1 (
    echo [dev-win-ts] cargo not found on PATH. Install the Rust toolchain first.
    goto fail
)

where node >nul 2>nul
if errorlevel 1 (
    echo [dev-win-ts] node not found on PATH. Install Node.js %MIN_NODE_MAJOR%.%MIN_NODE_MINOR% or newer first.
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
    echo [dev-win-ts] Node %NODE_MAJOR%.%NODE_MINOR% is too old. Nova needs %MIN_NODE_MAJOR%.%MIN_NODE_MINOR% or newer ^(vite 7 / react-router 7^).
    echo [dev-win-ts] Upgrade Node.js, or put a suitable Node earlier on PATH.
    goto fail
)
echo [dev-win-ts] Node %NODE_MAJOR%.%NODE_MINOR% accepted.

REM --- Build Rust backend (public-beta feature, shared target dir) -----------
echo [dev-win-ts] Building Rust backend binaries ^(public-beta^)...
pushd "%VIEWER_CORE_DIR%" || goto fail
cargo build --locked --release --features public-beta --target-dir target/public-beta --bin litematica_core --bin litematica_native_viewer --bin stockpile_server
if errorlevel 1 (
    popd
    goto fail
)
popd

for %%b in (litematica_core litematica_native_viewer stockpile_server) do (
    if not exist "%VIEWER_RELEASE_DIR%\%%b.exe" (
        echo [dev-win-ts] Missing backend artifact: %VIEWER_RELEASE_DIR%\%%b.exe
        goto fail
    )
)

REM --- Stage the public-beta deployment ---------------------------------------
REM Two independent gates require bin/public-beta to hold a COMPLETE staging:
REM   * desktop-nova/src-tauri/build.rs (validate_public_backends) asserts the
REM     directory contains exactly litematica_core.exe, litematica_native_viewer.exe
REM     and backend-deployment.json whenever it exists (an empty dir or a
REM     .gitkeep placeholder fails the build);
REM   * tauri.conf.json declares bin/public-beta as a bundle resource, so Tauri
REM     fails with "resource path ... doesn't exist" when it is missing.
REM Reuse the canonical staging script so the manifest format stays identical to
REM CI. -StageOnly skips the release-only git identity assertion.
echo [dev-win-ts] Staging public-beta deployment...
powershell -NoProfile -ExecutionPolicy Bypass -File "%DESKTOP_DIR%\scripts\prepare-backend-deployment.ps1" -RepoRoot "%REPO_ROOT%" -Profile release -StageOnly
if errorlevel 1 goto fail

if not exist "%PUBLIC_BETA_DIR%\backend-deployment.json" (
    echo [dev-win-ts] Missing %PUBLIC_BETA_DIR%\backend-deployment.json after staging.
    goto fail
)

REM --- Frontend dependencies ------------------------------------------------
pushd "%DESKTOP_DIR%" || goto fail

if not exist "node_modules" (
    echo [dev-win-ts] Installing frontend dependencies...
    call npm install
    if errorlevel 1 (
        popd
        goto fail
    )
)

echo [dev-win-ts] Starting Tauri development window...
call npm run tauri:dev
set "EXIT_CODE=%ERRORLEVEL%"

popd
echo.
echo [dev-win-ts] Tauri dev process exited with code %EXIT_CODE%.
pause
endlocal
exit /b %EXIT_CODE%

:fail
echo.
echo [dev-win-ts] Startup failed. See the messages above.
pause
endlocal
exit /b 1
