@echo off
setlocal enabledelayedexpansion

:: ==============================================================================
:: OpenUnpack - aria2 on-download-complete hook (Windows)
:: 
:: Setup in aria2.conf:
::   on-download-complete=C:\path\to\aria2-on-complete.bat
:: 
:: Arguments:
::   %1: GID
::   %2: File count
::   %3: File path
:: ==============================================================================

set "GID=%~1"
set "FILE_COUNT=%~2"
set "FILE_PATH=%~3"

if "%FILE_PATH%"=="" exit /b 0
if not exist "%FILE_PATH%" exit /b 0

echo [OpenUnpack-aria2] Triggered for GID %GID%, file: %FILE_PATH%

set "TARGET=%FILE_PATH%"
if "%FILE_COUNT%" gtr "1" (
    for %%F in ("%FILE_PATH%") do set "TARGET=%%~dpF"
)

where openunpack.exe >nul 2>&1
if %errorlevel% equ 0 (
    openunpack.exe unpack "%TARGET%"
    exit /b %errorlevel%
)

if exist "%LOCALAPPDATA%\Programs\OpenUnpack\openunpack.exe" (
    "%LOCALAPPDATA%\Programs\OpenUnpack\openunpack.exe" unpack "%TARGET%"
    exit /b %errorlevel%
)

set "PAYLOAD={\"command\":\"add_archive\",\"args\":{\"path\":\"%TARGET:\=\\%\",\"autoStart\":true}}"
curl -s -X POST "http://127.0.0.1:47980/api/v1/commands" ^
     -H "Content-Type: application/json" ^
     -d "%PAYLOAD%" >nul 2>&1
exit /b 0
