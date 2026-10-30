@echo off
setlocal DisableDelayedExpansion
:: qBittorrent completion hook: "%F" "%D" "%N" "%L".
:: OPENUNPACK_CLI may name the actual standalone CLI executable.
set "TARGET_PATH=%~1"
if not defined TARGET_PATH goto INVALID_INPUT
if not exist "%TARGET_PATH%" goto INVALID_INPUT
if not defined OPENUNPACK_CLI goto FIND_CLI
if not exist "%OPENUNPACK_CLI%" goto MISSING_CLI
goto RUN_CLI

:FIND_CLI
where openunpack.exe >nul 2>&1
if errorlevel 1 goto MISSING_CLI
set "OPENUNPACK_CLI=openunpack.exe"

:RUN_CLI
"%OPENUNPACK_CLI%" unpack "%TARGET_PATH%"
exit /b %errorlevel%

:INVALID_INPUT
echo [OpenUnpack-Hook] A completed local file or directory is required. >&2
exit /b 1

:MISSING_CLI
echo [OpenUnpack-Hook] Configure the standalone CLI through OPENUNPACK_CLI or PATH. >&2
exit /b 127
