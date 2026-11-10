@echo off
rem Build on Windows with the Rust MSVC toolchain (https://rustup.rs). Output: target\release\win10_tweaks.exe
rem Rename it to PrimeCrewTweakinator5000.exe for the portable zip (or use the release asset).
cd /d "%~dp0"
cargo build --release || exit /b 1
echo.
echo Built: %~dp0target\release\win10_tweaks.exe
