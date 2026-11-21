#!/usr/bin/env bash
# Build a Windows release: portable zip + NSIS installer, cross-compiled on Linux
# with llvm-mingw (x86_64-pc-windows-gnullvm).
# Needs: rustup target x86_64-pc-windows-gnullvm, llvm-mingw on PATH, makensis (NSIS 3).
# Usage: scripts/build-release.sh            (outputs into $OUT_DIR, default dist/)
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="PrimeCrew Tweakinator 5000"
APP_ID="PrimeCrewTweakinator5000"
FILE_STEM="PrimeCrewTweakinator5000"
PUBLISHER="PrimeCrew Software"
OUT_DIR="${OUT_DIR:-dist}"
TARGET=x86_64-pc-windows-gnullvm

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' app/Cargo.toml | head -1)
# 1.2.0 -> 1.2.0.0 (Windows file version) plus major/minor for ARP
read -r VERSION4 VERSION_MAJOR VERSION_MINOR < <(python3 -c "
import re,sys
v=sys.argv[1]
m=re.match(r'(\d+)\.(\d+)\.(\d+)',v)
if not m:
    raise SystemExit(f'bad version {v}')
print(f'{m.group(1)}.{m.group(2)}.{m.group(3)}.0', m.group(1), m.group(2))
" "$VERSION")
EXE="$FILE_STEM.exe"

echo "== $APP_NAME $VERSION ($VERSION4)"

if ! command -v x86_64-w64-mingw32-clang >/dev/null; then
  echo "x86_64-w64-mingw32-clang not on PATH (add llvm-mingw/bin)" >&2
  exit 1
fi
command -v makensis >/dev/null || { echo "makensis not found (apt install nsis)" >&2; exit 1; }

export WINDRES="${WINDRES:-x86_64-w64-mingw32-windres}"
export AR="${AR:-x86_64-w64-mingw32-ar}"
export AR_x86_64_pc_windows_gnullvm="${AR_x86_64_pc_windows_gnullvm:-$AR}"

( cd app && cargo build --release --target "$TARGET" )
BIN="app/target/$TARGET/release/$EXE"
[ -f "$BIN" ] || { echo "missing $BIN" >&2; exit 1; }

mkdir -p "$OUT_DIR"
cp "$BIN" "$OUT_DIR/$EXE"

# ---- portable zip (folder + exe + README)
ZIP_NAME="$FILE_STEM-v$VERSION-win64.zip"
ZIP="$OUT_DIR/$ZIP_NAME"
rm -f "$ZIP"
python3 - "$ZIP" "$BIN" "$EXE" "$FILE_STEM" <<'PY'
import sys, zipfile, pathlib
z, exe, name, folder = sys.argv[1:5]
with zipfile.ZipFile(z, "w", zipfile.ZIP_DEFLATED) as f:
    f.write(exe, f"{folder}/{name}")
    f.write("README.md", f"{folder}/README.md")
PY

# ---- installer
SETUP_NAME="$FILE_STEM-v$VERSION-setup.exe"
SETUP="$OUT_DIR/$SETUP_NAME"
makensis -V2 -INPUTCHARSET UTF8 \
  -DAPP_NAME="$APP_NAME" \
  -DAPP_ID="$APP_ID" \
  -DAPP_EXE="$EXE" \
  -DAPP_VERSION="$VERSION" \
  -DVERSION4="$VERSION4" \
  -DVERSION_MAJOR="$VERSION_MAJOR" \
  -DVERSION_MINOR="$VERSION_MINOR" \
  -DPUBLISHER="$PUBLISHER" \
  -DICON="$PWD/app/assets/app.ico" \
  -DEXE_PATH="$PWD/$BIN" \
  -DSRC_DIR="$PWD" \
  -DOUTFILE="$PWD/$SETUP" \
  installer/installer.nsi

( cd "$OUT_DIR" && sha256sum "$EXE" "$ZIP_NAME" "$SETUP_NAME" | tee "$FILE_STEM-v$VERSION-SHA256SUMS.txt" )
ls -la "$OUT_DIR"
