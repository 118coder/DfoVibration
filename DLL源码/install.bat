@echo off
rem ============================================================
rem DFO Vibration Plugin - installer (v1.1)
rem usage: install.bat [game_dir]
rem default game dir: G:\game\USDOF
rem ============================================================
setlocal

set GAME=%~1
if "%GAME%"=="" set GAME=G:\game\USDOF
set SRC=%~dp0release
set DEST=%GAME%\us_extend_dll

if not exist "%GAME%\DFO.exe" (
    echo ERROR: game not found at "%GAME%"
    exit /b 1
)

if not exist "%DEST%" mkdir "%DEST%"

copy /y "%SRC%\DfoVibration.dll" "%DEST%" >nul
if errorlevel 1 goto :fail
copy /y "%SRC%\DfoVibration.ini" "%DEST%" >nul
if errorlevel 1 goto :fail

rem control panel next to game (for auto-start after DLL mount)
copy /y "%SRC%\DfoVibration.exe" "%GAME%" >nul
if errorlevel 1 goto :fail
copy /y "%SRC%\DfoVibration_debug.exe" "%GAME%" >nul 2>nul

rem mapping/settings (TOML, keep existing if user already has one)
if not exist "%GAME%\DfoVibration.toml" copy /y "%SRC%\DfoVibration.toml" "%GAME%" >nul 2>nul

echo ============================================
echo Installed:
echo   %DEST%\DfoVibration.dll   (mounted by launcher)
echo   %DEST%\DfoVibration.ini   (config)
echo   %GAME%\DfoVibration.exe   (auto-started by DLL)
echo   %GAME%\DfoVibration_debug.exe (debug w/ console)
echo.
echo Usage:
echo   1. start game via launcher - DLL mounts, panel opens automatically
echo   2. Ctrl+Alt+V toggles vibration
echo   3. debug: run DfoVibration_debug.exe (console + DfoVibration.log)
echo ============================================
exit /b 0

:fail
echo INSTALL FAILED
exit /b 1
