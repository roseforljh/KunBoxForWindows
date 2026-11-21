@echo off
chcp 65001 >nul
title KunBox Build

echo.
echo ================================
echo     KunBox Build Script (Electron)
echo ================================
echo.

set /p choice="Select build mode [0=Installer (Release), 1=Portable]: "

if "%choice%"=="1" (
    echo Building Portable version...
    set BUILD_CMD=build:portable
) else (
    echo Building Installer version...
    set BUILD_CMD=build:win
)

echo.
echo Building KunBox with Electron...
cd /d "%~dp0kunbox-electron"
call npm run %BUILD_CMD%
if %errorlevel% neq 0 (
    echo Build failed!
    pause
    exit /b 1
)
echo Build completed successfully!
cd /d "%~dp0"
pause
