@echo off
setlocal EnableExtensions

call "%~dp0bin\ensure-dev-env.bat"
if errorlevel 1 (
  echo.
  echo [install] Failed to prepare the external development environment.
  pause
  exit /b 1
)

echo.
echo [install] Ready.
echo [install] Dev home: %LBA_DEV_HOME%
echo [install] Virtual env: %LBA_DEV_VENV%
exit /b 0
