@echo off
setlocal EnableExtensions EnableDelayedExpansion

set "ROOT=%~dp0.."
cd /d "%ROOT%" 2>nul
if errorlevel 1 (
  echo [pac-win] Cannot cd to: %ROOT%
  goto :pause_fail
)

if /i "%~1"=="help" goto :show_help
if "%~1"=="/?" goto :show_help

set "MODE="
if /i "%~1"=="full" set "MODE=full"
if /i "%~1"=="py" set "MODE=py"
if /i "%~1"=="python" set "MODE=py"
if defined MODE goto :mode_ok

echo.
echo Pack mode:
echo   1  Full  - include ui/resources + repo logo/ ^(for release^)
echo   2  Py-only - no --add-data; smaller/faster; assets and logos may be missing
set /p MODECHOICE=Choose [1/2], default 1: 
if "!MODECHOICE!"=="" set "MODECHOICE=1"
if "!MODECHOICE!"=="1" set "MODE=full"
if "!MODECHOICE!"=="2" set "MODE=py"
if not defined MODE (
  echo [pac-win] Invalid choice.
  goto :pause_fail
)

:mode_ok
if "!MODE!"=="full" (
  echo [pac-win] Mode: full ^(resources + logo^)
) else (
  echo [pac-win] Mode: Python only ^(no --add-data^)
)

call "%ROOT%\bin\ensure-dev-env.bat"
if errorlevel 1 (
  echo [pac-win] Failed to prepare the external development environment.
  goto :pause_fail
)

echo [pac-win] Ensuring deps and PyInstaller...
"%VENV_PY%" -m pip install -q -U pip
"%VENV_PY%" -m pip install -q -e .
"%VENV_PY%" -m pip install -q "pyinstaller>=6.0"

set "PREVIEW=%TEMP%\litematicaba_pack_preview.txt"
"%VENV_PY%" "%ROOT%\bin\pack_resolve.py" preview "%ROOT%" > "%PREVIEW%"
if errorlevel 1 (
  echo [pac-win] pack_resolve.py preview failed.
  goto :pause_fail
)

for /f "usebackq tokens=1,2,3 delims=|" %%a in ("%PREVIEW%") do (
  set "EXENAME=%%a"
  set "PKVER=%%b"
  set "PKCNT=%%c"
)

if not defined EXENAME (
  echo [pac-win] Could not parse preview line.
  goto :pause_fail
)

echo [pac-win] Version from src: !PKVER!  pack# !PKCNT!  exe base: !EXENAME!
echo [pac-win] Building one-file GUI exe - may take a few minutes.

if "!MODE!"=="full" (
  REM 含 deepslate_viewer.html、vendor/、nbt-viewer/；否则冻结环境下仅能打开展平后的 .py，HTML 与脚本不在 _MEIPASS
  "%VENV_PY%" -m PyInstaller --noconfirm --clean --onefile --windowed ^
    --name "!EXENAME!" --paths "%ROOT%\src" ^
    --hidden-import PySide6.QtSvg ^
    --add-data "%ROOT%\src\litematicaba\ui\resources;litematicaba\ui\resources" ^
    --add-data "%ROOT%\src\litematicaba\resources\web;litematicaba\resources\web" ^
    --add-data "%ROOT%\pack-in;pack-in" ^
    --add-data "%ROOT%\lang;lang" ^
    --add-data "%ROOT%\logo;logo" ^
    --splash "%ROOT%\logo\logo_full.png" ^
    --noupx ^
    "%ROOT%\src\litematicaba\__main__.py"
) else (
  "%VENV_PY%" -m PyInstaller --noconfirm --clean --onefile --windowed ^
    --name "!EXENAME!" --paths "%ROOT%\src" ^
    --hidden-import PySide6.QtSvg ^
    --splash "%ROOT%\logo\logo_full.png" ^
    --noupx ^
    "%ROOT%\src\litematicaba\__main__.py"
)

if errorlevel 1 (
  echo [pac-win] PyInstaller failed. pack_state.json not updated.
  goto :pause_fail
)

"%VENV_PY%" "%ROOT%\bin\pack_resolve.py" finalize "%ROOT%" "!PKVER!" "!PKCNT!"

echo.
echo [pac-win] Output: %ROOT%\dist\!EXENAME!.exe
goto :pause_ok

:show_help
echo Usage: %~nx0 [full ^| py ^| help]
echo   full   Full pack with ui/resources and logo/
echo   py     Python-only ^(no --add-data^)
echo   ^(no arg^)  Interactive choice
exit /b 0

:pause_fail
echo.
pause
exit /b 1

:pause_ok
pause
exit /b 0
