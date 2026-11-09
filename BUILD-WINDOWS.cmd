@echo off
setlocal
cd /d "%~dp0"

rem The key that signs version.json for the self-updater. Keep it in
rem signing-key.txt next to this file; never in the repository.
if exist "%~dp0signing-key.txt" (
    set /p UPDATE_SIGNING_KEY=<"%~dp0signing-key.txt"
)

echo Ezoo Download - Windows build
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-windows.ps1"
echo.
pause
