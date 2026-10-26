#!/usr/bin/env bash
# Ezoo Download — Linux build (deb + AppImage)
# Usage:  ./packaging/linux/build-linux.sh [deb|appimage|all]
# Result: dist/EzooDownload_1.0.6_amd64.deb  and  dist/EzooDownload-1.0.6-x86_64.AppImage
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
DIST="$ROOT/dist"
TARGET="${CARGO_TARGET:-$ROOT/target}"
WHAT="${1:-all}"

command -v cargo >/dev/null 2>&1 || { echo "cargo not found — install Rust: https://rustup.rs"; exit 1; }

echo "==> Building EzooDownload $VERSION (release)…"
(cd "$ROOT" && cargo build --release)
BIN="$TARGET/release/EzooDownload"
[ -x "$BIN" ] || { echo "binary not found at $BIN"; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

# ---------------------------------------------------------------- shared tree
mkdir -p "$STAGE/root/usr/bin" \
         "$STAGE/root/usr/share/ezoo-download" \
         "$STAGE/root/usr/share/applications" \
         "$STAGE/root/usr/share/doc/ezoo-download"
install -m 755 "$BIN" "$STAGE/root/usr/bin/EzooDownload"
cp -r "$ROOT/extension" "$STAGE/root/usr/share/ezoo-download/extension"
rm -rf "$STAGE/root/usr/share/ezoo-download/extension/test"
cp -r "$ROOT/extension" "$STAGE/root/usr/share/ezoo-download/extension-firefox"
mv "$STAGE/root/usr/share/ezoo-download/extension-firefox/manifest.firefox.json" \
   "$STAGE/root/usr/share/ezoo-download/extension-firefox/manifest.json"
rm -rf "$STAGE/root/usr/share/ezoo-download/extension-firefox/test"
cp "$HERE/ezoo-download.desktop" "$STAGE/root/usr/share/applications/"
cp -r "$HERE/hicolor" "$STAGE/root/usr/share/icons/hicolor" 2>/dev/null || {
    mkdir -p "$STAGE/root/usr/share/icons"; cp -r "$HERE/hicolor" "$STAGE/root/usr/share/icons/hicolor"; }
cp "$ROOT/README.md" "$STAGE/root/usr/share/doc/ezoo-download/"
cp "$ROOT/THIRD-PARTY-NOTICES.txt" "$STAGE/root/usr/share/doc/ezoo-download/" 2>/dev/null || true

# ------------------------------------------------------------------------ deb
build_deb() {
    echo "==> Building .deb…"
    local debdir="$STAGE/deb"
    cp -r "$STAGE/root" "$debdir"
    mkdir -p "$debdir/DEBIAN"
    sed "s/^Installed-Size: 1/Installed-Size: $(du -sk --exclude=DEBIAN "$debdir" | cut -f1)/" \
        "$HERE/debian/control" > "$debdir/DEBIAN/control"
    install -m 755 "$HERE/debian/postinst" "$debdir/DEBIAN/postinst"
    mkdir -p "$DIST"
    dpkg-deb --root-owner-group --build "$debdir" "$DIST/EzooDownload_${VERSION}_amd64.deb"
    echo "    $DIST/EzooDownload_${VERSION}_amd64.deb"
}

# ------------------------------------------------------------------- appimage
build_appimage() {
    echo "==> Building .AppImage…"
    local appdir="$STAGE/EzooDownload.AppDir"
    mkdir -p "$appdir"
    cp -r "$STAGE/root/usr" "$appdir/usr"
    cp "$HERE/ezoo-download.desktop" "$appdir/"
    cp "$HERE/hicolor/512x512/apps/ezoo-download.png" "$appdir/ezoo-download.png"
    cp "$HERE/hicolor/scalable/apps/ezoo-download.svg" "$appdir/ezoo-download.svg"
    ln -sf ezoo-download.svg "$appdir/.DirIcon"
    cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
# relative paths so the AppImage works from anywhere
HERE="$(dirname "$(readlink -f "$0")")"
export PATH="$HERE/usr/bin:$PATH"
exec "$HERE/usr/bin/EzooDownload" "$@"
EOF
    chmod +x "$appdir/AppRun"
    mkdir -p "$DIST"

    # linuxdeploy bundles the shared libraries the binary needs
    local ldep="$STAGE/linuxdeploy.AppImage"
    if [ ! -x "$ldep" ]; then
        curl -sL -o "$ldep" https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
        chmod +x "$ldep"
    fi
    if "$ldep" --appimage-extract-and-run \
        --appdir "$appdir" \
        --executable "$appdir/usr/bin/EzooDownload" \
        --desktop-file "$appdir/ezoo-download.desktop" \
        --icon-file "$appdir/ezoo-download.png" \
        >"$STAGE/linuxdeploy.log" 2>&1; then
        :
    else
        echo "    linuxdeploy failed — falling back to a plain packaging:" >&2
        tail -20 "$STAGE/linuxdeploy.log" >&2
    fi
    # linuxdeploy may rewrite the desktop file name; keep ours as the entry
    # and pack with appimagetool
    local tool="$STAGE/appimagetool.AppImage"
    if [ ! -x "$tool" ]; then
        curl -sL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
        chmod +x "$tool"
    fi
    "$tool" --appimage-extract-and-run --no-appstream "$appdir" "$DIST/EzooDownload-${VERSION}-x86_64.AppImage"
    echo "    $DIST/EzooDownload-${VERSION}-x86_64.AppImage"
}

case "$WHAT" in
    deb)      build_deb ;;
    appimage) build_appimage ;;
    all)      build_deb; build_appimage ;;
    *)        echo "usage: $0 [deb|appimage|all]"; exit 1 ;;
esac
echo "==> Done."
