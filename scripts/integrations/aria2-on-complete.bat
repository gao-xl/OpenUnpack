@echo off
setlocal DisableDelayedExpansion
:: aria2 arguments: GID, file count, first completed file path.
set "FILE_COUNT=%~2"
set "FILE_PATH=%~3"
if not defined FILE_PATH goto INVALID_INPUT
if not exist "%FILE_PATH%" goto INVALID_INPUT
if not defined FILE_COUNT goto INVALID_INPUT
for /f "delims=0123456789" %%N in ("%FILE_COUNT%") do goto INVALID_INPUT
if "%FILE_COUNT%"=="0" goto INVALID_INPUT
set "TARGET_PATH=%FILE_PATH%"
if "%FILE_COUNT%"=="1" goto RESOLVE_CLI
:: Append a dot so a quoted directory argument never ends in a backslash.
for %%F in ("%FILE_PATH%") do set "TARGET_PATH=%%~dpF."

:RESOLVE_CLI
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
echo [OpenUnpack-aria2] A positive file count and completed local file are required. >&2
exit /b 1

:MISSING_CLI
echo [OpenUnpack-aria2] Configure the standalone CLI through OPENUNPACK_CLI or PATH. >&2
exit /b 127
