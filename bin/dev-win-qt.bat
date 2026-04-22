@echo off
setlocal EnableExtensions

set "ROOT=%~dp0.."
cd /d "%ROOT%" 2>nul
if errorlevel 1 (
  echo [dev-win-qt] Cannot cd to: %ROOT%
  goto :pause_fail
)

call "%ROOT%\bin\ensure-dev-env.bat"
if errorlevel 1 (
  echo [dev-win-qt] Failed to prepare the external development environment.
  goto :pause_fail
)

echo [dev-win-qt] Starting LitematicaBA Qt ...
set "PYTHONPATH=%ROOT%\src"
"%VENV_PY%" -m litematicaba
if errorlevel 1 (
  echo.
  echo [dev-win-qt] App exited with error. See traceback above.
  goto :pause_fail
)

exit /b 0

:pause_fail
echo.
pause
exit /b 1
