@echo off
setlocal EnableExtensions EnableDelayedExpansion

set "APP_EXIT=1"
set "EXIT_REASON=Unknown failure"

set "ROOT=%~dp0.."
cd /d "%ROOT%" 2>nul
if errorlevel 1 (
  set "EXIT_REASON=Cannot cd to: %ROOT%"
  goto :pause_fail
)

set "UI_ROOT=%ROOT%\src\litematicanova\ui"
set "UI_DEV_HOME=%ROOT%\.tmp\ui-dev-env"
set "NPM_CONFIG_CACHE=%UI_DEV_HOME%\npm-cache"
set "STAMP_FILE=%UI_DEV_HOME%\package.stamp"

if not exist "%UI_ROOT%\package.json" (
  set "EXIT_REASON=Missing UI package: %UI_ROOT%\package.json"
  goto :pause_fail
)

if not exist "%UI_DEV_HOME%" mkdir "%UI_DEV_HOME%" >nul 2>&1
if not exist "%NPM_CONFIG_CACHE%" mkdir "%NPM_CONFIG_CACHE%" >nul 2>&1

where node >nul 2>&1
if errorlevel 1 (
  set "EXIT_REASON=node not found. Install Node.js 18+ and try again."
  goto :pause_fail
)

where npm >nul 2>&1
if errorlevel 1 (
  set "EXIT_REASON=npm not found. Install Node.js 18+ and try again."
  goto :pause_fail
)

where cargo >nul 2>&1
if errorlevel 1 (
  if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
  )
)

where cargo >nul 2>&1
if errorlevel 1 (
  echo [dev-win-ui] cargo not found. Rust is required for the Tauri desktop UI.
  echo [dev-win-ui] If Rust is already installed, reopen this window or add:
  echo              %USERPROFILE%\.cargo\bin
  echo              to your PATH.
  set "EXIT_REASON=cargo not found."
  goto :pause_fail
)

set "SYNC_KEY="
for %%I in ("%UI_ROOT%\package.json" "%UI_ROOT%\package-lock.json") do (
  if exist "%%~fI" call set "SYNC_KEY=%%SYNC_KEY%%|%%~nxI=%%~tI=%%~zI"
)

set "CURRENT_KEY="
if exist "%STAMP_FILE%" set /p CURRENT_KEY=<"%STAMP_FILE%"

set "NEED_SYNC=1"
if exist "%UI_ROOT%\node_modules\.bin\tauri.cmd" if exist "%UI_ROOT%\node_modules\.bin\vite.cmd" if /i "%CURRENT_KEY%"=="%SYNC_KEY%" set "NEED_SYNC="

pushd "%UI_ROOT%" >nul
if errorlevel 1 (
  set "EXIT_REASON=Cannot enter UI root: %UI_ROOT%"
  goto :pause_fail
)

if defined NEED_SYNC (
  echo [dev-win-ui] Syncing npm dependencies into "%UI_ROOT%\node_modules" ...
  call npm install --no-audit --no-fund
  if errorlevel 1 (
    set "APP_EXIT=!ERRORLEVEL!"
    set "EXIT_REASON=npm install failed."
    popd >nul
    goto :pause_fail
  )
  > "%STAMP_FILE%" echo(%SYNC_KEY%
) else (
  echo [dev-win-ui] Reusing existing npm dependencies in "%UI_ROOT%\node_modules".
)

set "PATH=%UI_ROOT%\node_modules\.bin;%PATH%"
set "TAURI_DEV_HOST=127.0.0.1"
set "TAURI_ROOT=%ROOT%\src\litematicanova\platform\tauri"
set "CARGO_TARGET_DIR=%TAURI_ROOT%\target"
set "TAURI_TARGET_MIGRATION_STAMP=%UI_DEV_HOME%\tauri-target-migrated-v1.stamp"
if not exist "%TAURI_TARGET_MIGRATION_STAMP%" (
  if exist "%CARGO_TARGET_DIR%" (
    echo [dev-win-ui] Clearing stale Rust target cache in "%CARGO_TARGET_DIR%" ...
    rmdir /s /q "%CARGO_TARGET_DIR%" >nul 2>&1
  )
  > "%TAURI_TARGET_MIGRATION_STAMP%" echo migrated
)

echo [dev-win-ui] Starting Tauri + React development shell ...
echo [dev-win-ui] UI root: %UI_ROOT%
call npm run tauri:dev
set "APP_EXIT=%ERRORLEVEL%"
popd >nul

if "%APP_EXIT%"=="0" (
  set "EXIT_REASON=UI dev command exited with code 0."
  goto :pause_success
)

set "EXIT_REASON=UI dev command exited with error %APP_EXIT%."
goto :pause_fail

:pause_success
echo.
echo [dev-win-ui] %EXIT_REASON%
echo [dev-win-ui] To close automatically, set DEV_WIN_UI_NO_PAUSE=1.
if /i not "%DEV_WIN_UI_NO_PAUSE%"=="1" pause
exit /b 0

:pause_fail
if "%APP_EXIT%"=="" set "APP_EXIT=1"
echo.
echo [dev-win-ui] %EXIT_REASON%
echo [dev-win-ui] Exit code: %APP_EXIT%
echo [dev-win-ui] To close automatically, set DEV_WIN_UI_NO_PAUSE=1.
if /i not "%DEV_WIN_UI_NO_PAUSE%"=="1" pause
exit /b %APP_EXIT%
