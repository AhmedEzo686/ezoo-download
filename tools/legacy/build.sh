#!/usr/bin/env bash
# Ezoo Download for Windows 7 / 8 / 8.1 (32- and 64-bit): one 32-bit program
# built from the same code as the Windows 10 / 11 build, on Linux (Ubuntu
# 24.04, as GitHub's runners), with MinGW.
#
#   tools/legacy/build.sh <stage dir> [ffmpeg dir]
#
# <stage dir> gets what the setup holds (installer/ezoo.iss /DLegacy):
#   EzooDownload.exe, ezoo-host.exe, extension\, extension-firefox\,
#   tools\python\ (Python 3.13 for Windows 7+ with yt-dlp's helpers),
#   tools\yt-dlp.pyz (yt-dlp's Python build) and, when [ffmpeg dir] is
#   given, the 32-bit ffmpeg built by tools/ffmpeg/build.sh with LEGACY=1.
#
# Rust: the target i686-win7-windows-gnu (Rust's own target for Windows 7)
# has no ready standard library: it is built here from the sources of the
# very Rust in use (-Zbuild-std). Every file fetched is checked by SHA-256.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
STAGE=$(realpath -m "$1")
FFMPEG_DIR=${2:-}
TARGET=i686-win7-windows-gnu
WORK=${WORK:-$ROOT/build-cache/legacy}
mkdir -p "$WORK" "$STAGE"

# Python 3.13 for Windows 7 and newer (embeddable, 32-bit), built from
# python.org's sources by github.com/adang1345/PythonVista
PY_VERSION=3.13.18
PY_URL=https://github.com/adang1345/PythonVista/releases/download/v$PY_VERSION/python-$PY_VERSION-embed-win32.zip
PY_SHA256=2d8886d68c369ec0efc76dfbadb1878a8de3db9b459974ec0cb48baea1851a01

fetch() { # url file sha256
  if [ ! -f "$2" ] || ! echo "$3  $2" | sha256sum -c --status -; then
    curl -sSfL --retry 3 -o "$2.part" "$1"
    echo "$3  $2.part" | sha256sum -c --status - || { echo "SHA-256 of $1 is not the expected one"; exit 1; }
    mv "$2.part" "$2"
  fi
}

echo "== tools"
command -v i686-w64-mingw32-gcc >/dev/null || { sudo apt-get update -q; sudo apt-get install -y -q --no-install-recommends mingw-w64; }
command -v llvm-objdump >/dev/null || { sudo apt-get install -y -q --no-install-recommends llvm; }

echo "== Rust's standard library sources (for $TARGET)"
SYSROOT=$(rustc --print sysroot)
COMMIT=$(rustc -vV | sed -n 's/^commit-hash: //p')
if [ ! -f "$SYSROOT/lib/rustlib/src/rust/library/std/Cargo.toml" ]; then
  if rustup component add rust-src 2>/dev/null; then :; else
    # no rustup (or no access to its server): the sources of this very
    # commit from Rust's repository
    SRC="$WORK/rust-src-$COMMIT"
    if [ ! -d "$SRC/library/std" ]; then
      rm -rf "$SRC"; mkdir -p "$SRC"
      git -C "$SRC" init -q
      git -C "$SRC" remote add origin https://github.com/rust-lang/rust
      git -C "$SRC" fetch -q --depth 1 --filter=blob:none origin "$COMMIT"
      git -C "$SRC" sparse-checkout set --no-cone /library /Cargo.lock
      git -C "$SRC" checkout -q FETCH_HEAD
      BT=$(git -C "$SRC" ls-tree HEAD library/backtrace | awk '{print $3}')
      git -C "$SRC/library/backtrace" init -q
      git -C "$SRC/library/backtrace" remote add origin https://github.com/rust-lang/backtrace-rs.git
      git -C "$SRC/library/backtrace" fetch -q --depth 1 origin "$BT"
      git -C "$SRC/library/backtrace" checkout -q FETCH_HEAD
    fi
    mkdir -p "$SYSROOT/lib/rustlib/src/rust"
    ln -sfn "$SRC/library" "$SYSROOT/lib/rustlib/src/rust/library"
    cp "$SRC/Cargo.lock" "$SYSROOT/lib/rustlib/src/rust/"
  fi
fi

echo "== import library of windows-sys 0.48 for $TARGET"
# windows-sys 0.48 names its MinGW library only for i686-pc-windows-gnu:
# the same crate, from crates.io, checked against Cargo.lock
LIBS="$WORK/winlibs"; mkdir -p "$LIBS"
SUM=$(awk '/^name = "windows_i686_gnu"/{f=1} f&&/^version = "0.48.5"/{g=1} g&&/^checksum/{print $3; exit}' "$ROOT/Cargo.lock" | tr -d '"')
if [ -n "$SUM" ] && [ ! -f "$LIBS/libwindows.0.48.5.a" ]; then
  fetch https://static.crates.io/crates/windows_i686_gnu/windows_i686_gnu-0.48.5.crate "$WORK/w48.crate" "$SUM"
  tar xzf "$WORK/w48.crate" -C "$WORK"
  cp "$WORK/windows_i686_gnu-0.48.5/lib/libwindows.0.48.5.a" "$LIBS/"
fi

echo "== Ezoo Download ($TARGET)"
# the program fetches this very ffmpeg again (ffmpeg-legacy.exe of the
# release) only when its SHA-256 is this one
if [ -n "$FFMPEG_DIR" ]; then
  export EZOO_FFMPEG_SHA256=$(sha256sum "$FFMPEG_DIR/ffmpeg.exe" | cut -d' ' -f1)
fi
export CROSS_COMPILE=i686-w64-mingw32-
export CC_i686_win7_windows_gnu=i686-w64-mingw32-gcc CXX_i686_win7_windows_gnu=i686-w64-mingw32-g++ AR_i686_win7_windows_gnu=i686-w64-mingw32-ar
# the few Windows 8 / 10 functions some libraries name, answered by us
# (win7thunks.c): linked first, so the program imports none of them
i686-w64-mingw32-gcc -O2 -Wall -D_WIN32_WINNT=0x0601 -c "$ROOT/tools/legacy/win7thunks.c" -o "$WORK/win7thunks.o"
export CARGO_TARGET_I686_WIN7_WINDOWS_GNU_RUSTFLAGS="-C link-args=-static-libgcc -C link-args=-static -L native=$LIBS -Z pre-link-arg=$WORK/win7thunks.o"
(cd "$ROOT" && RUSTC_BOOTSTRAP=1 cargo build --release --locked -Zbuild-std --target $TARGET \
  -p swiftget -p ezoo-host --features swiftget/legacy)
OUT="$ROOT/target/$TARGET/release"
cp "$OUT/EzooDownload.exe" "$OUT/ezoo-host.exe" "$STAGE/"

echo "== Windows 7 check (nothing newer than Windows 7 is asked of Windows)"
python3 "$ROOT/tools/legacy/win7check.py" "$STAGE/EzooDownload.exe" "$STAGE/ezoo-host.exe"

echo "== Python and yt-dlp"
mkdir -p "$STAGE/tools"
fetch "$PY_URL" "$WORK/python.zip" "$PY_SHA256"
rm -rf "$STAGE/tools/python"; mkdir -p "$STAGE/tools/python/Lib/site-packages"
(cd "$STAGE/tools/python" && unzip -q "$WORK/python.zip")
rm -rf "$WORK/wheels"; mkdir -p "$WORK/wheels"
pip download -q --no-deps --only-binary=:all: --platform win32 --python-version 3.13 --implementation cp \
  --abi cp313 --abi abi3 --abi none --require-hashes -r "$ROOT/tools/legacy/requirements.txt" -d "$WORK/wheels"
for w in "$WORK"/wheels/*.whl; do unzip -qo "$w" -d "$STAGE/tools/python/Lib/site-packages"; done
rm -rf "$STAGE/tools/python/Lib/site-packages/bin"
PTH=$(ls "$STAGE"/tools/python/python3*._pth)
ZIP=$(basename "$(ls "$STAGE"/tools/python/python3*.zip)")
printf '%s\r\n.\r\nLib\\site-packages\r\n' "$ZIP" > "$PTH"
python3 "$ROOT/tools/legacy/win7check.py" --ucrt-ok "$STAGE"/tools/python/*.exe "$STAGE"/tools/python/*.dll "$STAGE"/tools/python/*.pyd
# yt-dlp's Python build of its latest release, checked by yt-dlp's own SHA-256 list
curl -sSfL --retry 3 -o "$WORK/SHA2-256SUMS" https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS
YSUM=$(awk '$2=="yt-dlp"{print $1}' "$WORK/SHA2-256SUMS")
curl -sSfL --retry 3 -o "$STAGE/tools/yt-dlp.pyz" https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp
echo "$YSUM  $STAGE/tools/yt-dlp.pyz" | sha256sum -c --status - || { echo "yt-dlp: SHA-256 mismatch"; exit 1; }

if [ -n "$FFMPEG_DIR" ]; then
  echo "== ffmpeg (32-bit, Windows 7)"
  python3 "$ROOT/tools/legacy/win7check.py" "$FFMPEG_DIR/ffmpeg.exe"
  cp "$FFMPEG_DIR"/* "$STAGE/tools/"
fi

echo "== extension"
rm -rf "$STAGE/extension" "$STAGE/extension-firefox"
cp -r "$ROOT/extension" "$STAGE/extension"; rm -rf "$STAGE/extension/test" "$STAGE/extension/manifest.firefox.json"
cp -r "$ROOT/extension" "$STAGE/extension-firefox"; rm -rf "$STAGE/extension-firefox/test"
mv "$STAGE/extension-firefox/manifest.firefox.json" "$STAGE/extension-firefox/manifest.json"
cp "$ROOT/THIRD-PARTY-NOTICES.txt" "$STAGE/" 2>/dev/null || true
cp "$ROOT/LICENSE" "$STAGE/LICENSE.txt" 2>/dev/null || true
echo "done: $STAGE"
