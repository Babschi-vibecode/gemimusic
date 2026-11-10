@echo off
echo ========================================================
echo        gemimusic - Android APK Build Script
echo ========================================================

:: Verify cargo-apk is installed
where cargo-apk >nul 2>nul
if %ERRORLEVEL% neq 0 (
    echo [!] cargo-apk is not installed. Installing cargo-apk...
    cargo install cargo-apk
)

:: Ensure Android target is added to rust toolchain
echo [*] Checking Android Rust target (aarch64-linux-android)...
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

:: Check environment variables
if "%ANDROID_HOME%"=="" (
    if "%ANDROID_SDK_ROOT%"=="" (
        echo [!] Warning: ANDROID_HOME or ANDROID_SDK_ROOT is not set.
        echo Please ensure Android SDK is installed and configured in your PATH.
    )
)

if "%NDK_HOME%"=="" (
    echo [*] Checking default NDK paths...
)

echo [*] Building Release APK for Android...
cargo apk build --release

if %ERRORLEVEL% equ 0 (
    echo [✓] Build Succeeded!
    echo APK located in: target\release\apk\gemimusic.apk
    echo.
    echo To install on a connected Android phone/emulator:
    echo   cargo apk run --release
    echo   or
    echo   adb install -r target\release\apk\gemimusic.apk
) else (
    echo [X] Build failed. Please verify your Android NDK and SDK installation.
)
