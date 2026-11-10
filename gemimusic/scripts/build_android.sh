#!/bin/bash
set -e

echo "========================================================"
echo "       gemimusic - Android APK Build Script"
echo "========================================================"

if ! command -v cargo-apk &> /dev/null; then
    echo "[!] cargo-apk could not be found. Installing..."
    cargo install cargo-apk
fi

echo "[*] Adding Android targets to Rust toolchain..."
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

echo "[*] Compiling Android Release APK..."
cargo apk build --release

echo "[✓] APK successfully built at: target/release/apk/gemimusic.apk"
echo "To run on connected Android device: cargo apk run --release"
