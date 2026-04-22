@echo off
setlocal EnableExtensions

set "ROOT=%~dp0"
if "%ROOT:~-1%"=="\" set "ROOT=%ROOT:~0,-1%"

call "%ROOT%\bin\ensure-dev-env.bat"
if not errorlevel 1 (
  set "PYTHONPATH=%ROOT%\src"
  "%VENV_PY%" -m litematicaba
  exit /b %ERRORLEVEL%
)

set "LAUNCHER=%ROOT%\bin\launcher\Litematica-BA.exe"
if exist "%LAUNCHER%" (
  start "" "%LAUNCHER%"
  exit /b 0
)

set "PYTHONPATH=%ROOT%\src"
python -m litematicaba
exit /b %ERRORLEVEL%
