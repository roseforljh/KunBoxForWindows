@echo off
chcp 65001 >nul
title KunBox Build

echo.
echo ================================
echo     KunBox Build Script
echo ================================
echo.

set /p choice="Select build mode [0=Release, 1=Debug]: "

if "%choice%"=="1" (
    echo Building DEBUG version...
    set BUILD_MODE=debug
) else (
    echo Building RELEASE version...
    set BUILD_MODE=release
)

echo.
echo [1/2] Building frontend...
cd /d "%~dp0kunbox-electron"
call npm run build
if %errorlevel% neq 0 (
    echo Frontend build failed!
    pause
    exit /b 1
)
echo Frontend build completed.

echo.
echo [2/2] Building backend (Tauri)...
cd /d "%~dp0src-tauri"

if "%BUILD_MODE%"=="debug" (
    cargo tauri build --debug
) else (
    cargo tauri build
)

if %errorlevel% neq 0 (
    echo Backend build failed!
    pause
    exit /b 1
)

cd /d "%~dp0"

echo.
echo ================================
echo     Build completed!
echo ================================
echo.

if "%BUILD_MODE%"=="debug" (
    echo Output: %~dp0src-tauri\target\debug
) else (
    echo Output: %~dp0src-tauri\target\release\bundle
)

echo.
pause
