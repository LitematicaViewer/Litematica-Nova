@echo off
setlocal

set "SCRIPT_DIR=%~dp0"
set "REPO_ROOT=%SCRIPT_DIR%.."
set "DESKTOP_DIR=%REPO_ROOT%\desktop-nova"
set "VIEWER_BACKEND_DIR=%REPO_ROOT%\bin\viewer-backend"

if not exist "%DESKTOP_DIR%\package.json" (
    echo [dev-win-ts] Missing desktop-nova package.json.
    goto fail
)

if not exist "%VIEWER_BACKEND_DIR%\litematica_core.exe" (
    echo [dev-win-ts] Missing %VIEWER_BACKEND_DIR%\litematica_core.exe
    echo [dev-win-ts] Run bin\build-win-ts.bat first.
    goto fail
)

if not exist "%VIEWER_BACKEND_DIR%\litematica_native_viewer.exe" (
    echo [dev-win-ts] Missing %VIEWER_BACKEND_DIR%\litematica_native_viewer.exe
    echo [dev-win-ts] Run bin\build-win-ts.bat first.
    goto fail
)

pushd "%DESKTOP_DIR%" || goto fail

if not exist "node_modules" (
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
