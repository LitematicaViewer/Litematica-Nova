@echo off
setlocal EnableExtensions
REM PowerShell 7 is the canonical development entry; CMD remains a thin wrapper.
set "PWSH="
for %%I in (pwsh.exe) do if not defined PWSH set "PWSH=%%~$PATH:I"
if not defined PWSH if exist "%ProgramFiles%\PowerShell\7\pwsh.exe" set "PWSH=%ProgramFiles%\PowerShell\7\pwsh.exe"
if not defined PWSH (
    echo [dev-win] PowerShell 7 is required. Run: winget install --id Microsoft.PowerShell
    exit /b 1
)
"%PWSH%" -NoProfile -File "%~dp0dev-win.ps1" %*
exit /b %ERRORLEVEL%
