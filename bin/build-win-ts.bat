@echo off
setlocal

set "SCRIPT_DIR=%~dp0"
set "REPO_ROOT=%SCRIPT_DIR%.."
set "DESKTOP_DIR=%REPO_ROOT%\desktop-nova"
set "VIEWER_CORE_DIR=%REPO_ROOT%\tools\viewer-core"
set "VIEWER_BACKEND_DIR=%REPO_ROOT%\bin\viewer-backend"

echo [build-win-ts] Repo: %REPO_ROOT%

if not exist "%DESKTOP_DIR%\package.json" (
    echo [build-win-ts] Missing desktop-nova package.json.
    goto fail
)

if not exist "%VIEWER_CORE_DIR%\Cargo.toml" (
    echo [build-win-ts] Missing viewer-core Cargo.toml.
    goto fail
)

echo [build-win-ts] Building Rust backend binaries...
pushd "%VIEWER_CORE_DIR%" || goto fail
cargo build --release --bin litematica_core
if errorlevel 1 (
    popd
    goto fail
)
cargo build --release --bin litematica_native_viewer
if errorlevel 1 (
    popd
    goto fail
)
popd

echo [build-win-ts] Syncing backend binaries...
if not exist "%VIEWER_BACKEND_DIR%" mkdir "%VIEWER_BACKEND_DIR%"
copy /Y "%VIEWER_CORE_DIR%\target\release\litematica_core.exe" "%VIEWER_BACKEND_DIR%\litematica_core.exe" >nul
if errorlevel 1 goto fail
copy /Y "%VIEWER_CORE_DIR%\target\release\litematica_native_viewer.exe" "%VIEWER_BACKEND_DIR%\litematica_native_viewer.exe" >nul
if errorlevel 1 goto fail

echo [build-win-ts] Preparing desktop-nova dependencies...
pushd "%DESKTOP_DIR%" || goto fail
if not exist "node_modules" (
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

echo [build-win-ts] Building Tauri desktop app...
call npm run tauri:build
if errorlevel 1 (
    popd
    goto fail
)
popd

echo [build-win-ts] Build completed. Opening development window...
call "%SCRIPT_DIR%dev-win-ts.bat"
set "EXIT_CODE=%ERRORLEVEL%"

endlocal
exit /b %EXIT_CODE%

:fail
echo.
echo [build-win-ts] Build failed. See the messages above.
pause
endlocal
exit /b 1
