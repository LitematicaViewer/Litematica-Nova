@echo off
setlocal EnableExtensions EnableDelayedExpansion

set "SCRIPT_DIR=%~dp0"
set "ROOT=%SCRIPT_DIR%.."
cd /d "%ROOT%" 2>nul
if errorlevel 1 (
  echo [ensure-dev-env] Cannot cd to: %ROOT%
  goto :fail
)

if not defined LBA_DEV_HOME (
  if defined LOCALAPPDATA (
    set "LBA_DEV_HOME=%LOCALAPPDATA%\Litematica-BA\dev-env"
  ) else (
    set "LBA_DEV_HOME=%USERPROFILE%\AppData\Local\Litematica-BA\dev-env"
  )
)
if not defined LBA_DEV_VENV set "LBA_DEV_VENV=%LBA_DEV_HOME%\venv"
if not defined LBA_PIP_CACHE set "LBA_PIP_CACHE=%LBA_DEV_HOME%\pip-cache"

set "VENV_PY=%LBA_DEV_VENV%\Scripts\python.exe"
set "STAMP_FILE=%LBA_DEV_HOME%\requirements.stamp"

if not exist "%LBA_DEV_HOME%" mkdir "%LBA_DEV_HOME%" >nul 2>&1
if not exist "%LBA_PIP_CACHE%" mkdir "%LBA_PIP_CACHE%" >nul 2>&1

if defined PYTHON (
  set "PYEXE=%PYTHON%"
  goto :pyexe_done
)

set "PYEXE="
for /f "delims=" %%i in ('py -3.11 -c "import sys; print(sys.executable)" 2^>nul') do set "PYEXE=%%i"
if defined PYEXE goto :pyexe_done
for /f "delims=" %%i in ('py -3.10 -c "import sys; print(sys.executable)" 2^>nul') do set "PYEXE=%%i"
if defined PYEXE goto :pyexe_done
for /f "delims=" %%i in ('py -3.12 -c "import sys; print(sys.executable)" 2^>nul') do set "PYEXE=%%i"
if defined PYEXE goto :pyexe_done
for /f "delims=" %%i in ('py -3.13 -c "import sys; print(sys.executable)" 2^>nul') do set "PYEXE=%%i"
if defined PYEXE goto :pyexe_done
where python >nul 2>&1
if not errorlevel 1 (
  set "PYEXE=python"
  goto :pyexe_done
)
if exist "%USERPROFILE%\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe" (
  set "PYEXE=%USERPROFILE%\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe"
  goto :pyexe_done
)
set "PYEXE=python"

:pyexe_done
"%PYEXE%" -c "import sys" 2>nul
if errorlevel 1 (
  echo [ensure-dev-env] Python not found. Install Python 3.11 x64 or set PYTHON=path\to\python.exe
  goto :fail
)

if not defined ALLOW_PY314 (
  "%PYEXE%" -c "import sys; sys.exit(0 if sys.version_info < (3,14) else 1)" 2>nul
  if errorlevel 1 (
    echo [ensure-dev-env] Python 3.14 is not supported by the default Windows wheel set yet.
    echo                   Use Python 3.11, or set ALLOW_PY314=1 to try anyway.
    goto :fail
  )
)

if exist "%VENV_PY%" (
  for /f "delims=" %%a in ('"%PYEXE%" -c "import sys; print(str(sys.version_info[0])+chr(46)+str(sys.version_info[1]))" 2^>nul') do set "WANTV=%%a"
  for /f "delims=" %%b in ('"%VENV_PY%" -c "import sys; print(str(sys.version_info[0])+chr(46)+str(sys.version_info[1]))" 2^>nul') do set "HAVEV=%%b"
  if defined WANTV if defined HAVEV if not "!WANTV!"=="!HAVEV!" (
    echo [ensure-dev-env] Python mismatch: selected interpreter is !WANTV! but env is !HAVEV!.
    echo                   Delete "%LBA_DEV_VENV%" and run again.
    goto :fail
  )
)

if not exist "%VENV_PY%" (
  echo [ensure-dev-env] Creating virtual environment at "%LBA_DEV_VENV%" ...
  "%PYEXE%" -m venv "%LBA_DEV_VENV%"
  if errorlevel 1 (
    echo [ensure-dev-env] python -m venv failed.
    goto :fail
  )
)

set "SYNC_KEY="
for %%I in ("%ROOT%\requirements.txt" "%ROOT%\pyproject.toml") do (
  set "SYNC_KEY=!SYNC_KEY!|%%~nxI=%%~tI=%%~zI"
)

set "CURRENT_KEY="
if exist "%STAMP_FILE%" set /p CURRENT_KEY=<"%STAMP_FILE%"

set "NEED_SYNC=1"
if exist "%VENV_PY%" if /i "!CURRENT_KEY!"=="!SYNC_KEY!" set "NEED_SYNC="

if defined NEED_SYNC (
  echo [ensure-dev-env] Syncing dependencies into "%LBA_DEV_VENV%" ...
  set "PIP_CACHE_DIR=%LBA_PIP_CACHE%"
  "%VENV_PY%" -m pip install --disable-pip-version-check --upgrade pip setuptools wheel
  if errorlevel 1 (
    echo [ensure-dev-env] pip bootstrap failed.
    goto :fail
  )
  "%VENV_PY%" -m pip install --disable-pip-version-check -r "%ROOT%\requirements.txt"
  if errorlevel 1 (
    echo [ensure-dev-env] pip install requirements failed.
    goto :fail
  )
  "%VENV_PY%" -m pip install --disable-pip-version-check -e "%ROOT%"
  if errorlevel 1 (
    echo [ensure-dev-env] pip install -e . failed.
    goto :fail
  )
  > "%STAMP_FILE%" echo(!SYNC_KEY!
) else (
  echo [ensure-dev-env] Reusing existing environment at "%LBA_DEV_VENV%".
)

endlocal & (
  set "LBA_DEV_HOME=%LBA_DEV_HOME%"
  set "LBA_DEV_VENV=%LBA_DEV_VENV%"
  set "LBA_PIP_CACHE=%LBA_PIP_CACHE%"
  set "VENV_PY=%VENV_PY%"
)
exit /b 0

:fail
endlocal
exit /b 1
