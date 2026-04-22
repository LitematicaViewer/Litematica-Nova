@echo off
setlocal EnableExtensions

set "ROOT=%~dp0.."
cd /d "%ROOT%" 2>nul
if errorlevel 1 (
  echo [dev-win] ERROR: cannot change to project directory.
  goto :error
)

call "%ROOT%\bin\ensure-dev-env.bat"
if errorlevel 1 (
  echo [dev-win] ERROR: failed to prepare the external development environment.
  goto :error
)

echo [dev-win] Starting legacy viewer entry ...
"%VENV_PY%" "%ROOT%\script\LitematicaViewer.py"
set "EXITCODE=%ERRORLEVEL%"
if not "%EXITCODE%"=="0" (
  echo.
  echo [dev-win] Process exited with code: %EXITCODE%
  pause
)
exit /b %EXITCODE%

:error
echo.
pause
exit /b 1
