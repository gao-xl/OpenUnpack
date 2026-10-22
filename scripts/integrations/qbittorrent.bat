@echo off
setlocal enabledelayedexpansion

:: ==============================================================================
:: OpenUnpack - qBittorrent Download Completion Hook (Windows)
:: 
:: Setup in qBittorrent:
::   1. Open Tools -> Options -> Downloads -> "Run external program on torrent finished"
::   2. Command:
::      "C:\path\to\qbittorrent.bat" "%F" "%D" "%N" "%L"
:: 
:: Arguments:
::   %1 (%F): Content path (same as root path for multifile torrent)
::   %2 (%D): Save directory
::   %3 (%N): Torrent name
::   %4 (%L): Category
:: ==============================================================================

set "TARGET_PATH=%~1"
set "SAVE_DIR=%~2"
set "TORRENT_NAME=%~3"
set "CATEGORY=%~4"

echo [OpenUnpack-Hook] Triggered for torrent: %TORRENT_NAME%
echo [OpenUnpack-Hook] Target path: %TARGET_PATH%

if "%TARGET_PATH%"=="" (
    echo [OpenUnpack-Hook] Error: Target path parameter is empty.
    exit /b 1
)

:: Try locating openunpack CLI
set "OPENUNPACK_CLI=openunpack.exe"
where openunpack.exe >nul 2>&1
if %errorlevel% equ 0 (
    goto :RUN_CLI
)

if exist "%LOCALAPPDATA%\Programs\OpenUnpack\openunpack.exe" (
    set "OPENUNPACK_CLI=%LOCALAPPDATA%\Programs\OpenUnpack\openunpack.exe"
    goto :RUN_CLI
)

:: Fallback to curl with Local API
echo [OpenUnpack-Hook] CLI not found in PATH, attempting REST API fallback...
set "API_PORT=47980"
set "API_TOKEN="

:: Read local daemon token if available
set "INFO_FILE=%LOCALAPPDATA%\OpenUnpack\daemon.info"
if exist "%INFO_FILE%" (
    for /f "tokens=2 delims=:, " %%a in ('findstr /i "token" "%INFO_FILE%"') do (
        set "API_TOKEN=%%~a"
    )
)

set "AUTH_HEADER="
if not "%API_TOKEN%"=="" (
    set "AUTH_HEADER=-H "Authorization: Bearer %API_TOKEN%""
)

set "PAYLOAD={\"command\":\"add_archive\",\"args\":{\"path\":\"%TARGET_PATH:\=\\%\",\"autoStart\":true}}"
curl -s -X POST "http://127.0.0.1:%API_PORT%/api/v1/commands" ^
     -H "Content-Type: application/json" ^
     %AUTH_HEADER% ^
     -d "%PAYLOAD%" >nul 2>&1

if %errorlevel% equ 0 (
    echo [OpenUnpack-Hook] Successfully queued via Local API.
    exit /b 0
) else (
    echo [OpenUnpack-Hook] Failed to call Local API. Ensure OpenUnpack daemon is running.
    exit /b 1
)

:RUN_CLI
echo [OpenUnpack-Hook] Dispatching to %OPENUNPACK_CLI%...
"%OPENUNPACK_CLI%" unpack "%TARGET_PATH%"
exit /b %errorlevel%
