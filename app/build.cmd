@echo off
rem Build on Windows with the Rust MSVC toolchain (https://rustup.rs).
rem Output: target\release\PrimeCrewTweakinator5000.exe
cd /d "%~dp0"
cargo build --release || exit /b 1
echo.
echo Built: %~dp0target\release\PrimeCrewTweakinator5000.exe
