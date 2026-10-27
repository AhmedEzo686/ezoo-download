#!/usr/bin/env bash
# Ezoo Download — Linux build (deb + AppImage + rpm)
# Usage:  ./packaging/linux/build-linux.sh [deb|appimage|rpm|all]
# Result: dist/EzooDownload_1.0.7_amd64.deb
#         dist/EzooDownload-1.0.7-x86_64.AppImage
#         dist/ezoo-download-1.0.7-1.x86_64.rpm
#
# Environment:
#   SKIP_CARGO_BUILD=1  reuse target/release/EzooDownload, do not rebuild
#                       (CI builds it once first, with caching)
#   SKIP_FFMPEG=1       package without the bundled ffmpeg (development
#                       only: merges then need a system ffmpeg on PATH)
#   CARGO_TARGET        cargo target dir (default: $ROOT/target)
#
# ffmpeg is BUNDLED inside every package (see fetch-ffmpeg.sh): the static
# build is downloaded at packaging time, checksum-checked, and staged
# into the package only — it never enters the source tree.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
DIST="$ROOT/dist"
TARGET="${CARGO_TARGET:-$ROOT/target}"
WHAT="${1:-all}"

if [ -z "${SKIP_CARGO_BUILD:-}" ]; then
    command -v cargo >/dev/null 2>&1 || { echo "cargo not found — install Rust: https://rustup.rs"; exit 1; }
    echo "==> Building EzooDownload $VERSION (release)…"
    (cd "$ROOT" && cargo build --release)
fi
BIN="$TARGET/release/EzooDownload"
[ -x "$BIN" ] || { echo "binary not found at $BIN"; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

# ---------------------------------------------------------------- ffmpeg stage
# One download for all formats; each format stages from this directory.
FFBIN=""
if [ -z "${SKIP_FFMPEG:-}" ]; then
    "$HERE/fetch-ffmpeg.sh" "$STAGE/ffmpeg-bin"
    FFBIN="$STAGE/ffmpeg-bin/ffmpeg"
else
    echo "==> SKIP_FFMPEG=1: packaging WITHOUT the bundled ffmpeg" >&2
fi

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
mkdir -p "$STAGE/root/usr/share/metainfo"
cp "$HERE/io.github.ahmedezo686.ezoo-download.metainfo.xml" "$STAGE/root/usr/share/metainfo/"

# The bundled ffmpeg, for the .deb/.rpm layout: /usr/lib/ezoo-download.
# The AppImage stages its own copy next to the program (see build_appimage).
stage_ffmpeg_libdir() {
    local dest="$1"
    [ -n "$FFBIN" ] || return 0
    mkdir -p "$dest/usr/lib/ezoo-download"
    install -m 755 "$FFBIN" "$dest/usr/lib/ezoo-download/ffmpeg"
    cp "$STAGE/ffmpeg-bin/ffmpeg-version.txt" "$dest/usr/lib/ezoo-download/ffmpeg-version.txt"
    cp "$STAGE/ffmpeg-bin/ffmpeg-LICENSE.txt" "$dest/usr/share/doc/ezoo-download/ffmpeg-LICENSE.txt"
}

# ------------------------------------------------------------------------ deb
build_deb() {
    echo "==> Building .deb…"
    local debdir="$STAGE/deb"
    cp -r "$STAGE/root" "$debdir"
    stage_ffmpeg_libdir "$debdir"
    mkdir -p "$debdir/DEBIAN"
    sed "s/^Installed-Size: 1/Installed-Size: $(du -sk --exclude=DEBIAN "$debdir" | cut -f1)/" \
        "$HERE/debian/control" > "$debdir/DEBIAN/control"
    install -m 755 "$HERE/debian/postinst" "$debdir/DEBIAN/postinst"
    mkdir -p "$DIST"
    dpkg-deb --root-owner-group --build "$debdir" "$DIST/EzooDownload_${VERSION}_amd64.deb"
    echo "    $DIST/EzooDownload_${VERSION}_amd64.deb"
}

# ------------------------------------------------------------------------ rpm
build_rpm() {
    echo "==> Building .rpm…"
    command -v rpmbuild >/dev/null 2>&1 || { echo "rpmbuild not found — install the 'rpm' package (Debian/Ubuntu: sudo apt-get install rpm)"; exit 1; }
    local rpmdir="$STAGE/rpm" root="$STAGE/rpm-root"
    cp -r "$STAGE/root" "$root"
    stage_ffmpeg_libdir "$root"
    mkdir -p "$rpmdir"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
    (cd "$root" && tar -czf "$rpmdir/SOURCES/root.tar.gz" .)
    sed "s/^Version: .*/Version: $VERSION/" "$HERE/rpm/ezoo-download.spec" > "$rpmdir/SPECS/ezoo-download.spec"
    rpmbuild --define "_topdir $rpmdir" --define "_rpmdir $rpmdir/RPMS" -bb "$rpmdir/SPECS/ezoo-download.spec" >"$STAGE/rpmbuild.log" 2>&1 \
        || { echo "rpmbuild failed:" >&2; tail -30 "$STAGE/rpmbuild.log" >&2; exit 1; }
    mkdir -p "$DIST"
    cp "$rpmdir"/RPMS/x86_64/*.rpm "$DIST/"
    echo "    $DIST/$(ls "$rpmdir/RPMS/x86_64")"
}

# ------------------------------------------------------------------- appimage
build_appimage() {
    echo "==> Building .AppImage…"
    local appdir="$STAGE/EzooDownload.AppDir"
    mkdir -p "$appdir"
    cp -r "$STAGE/root/usr" "$appdir/usr"
    # the bundled ffmpeg next to the program inside the AppImage
    if [ -n "$FFBIN" ]; then
        install -m 755 "$FFBIN" "$appdir/usr/bin/ffmpeg"
        cp "$STAGE/ffmpeg-bin/ffmpeg-version.txt" "$appdir/usr/share/doc/ezoo-download/ffmpeg-version.txt"
        cp "$STAGE/ffmpeg-bin/ffmpeg-LICENSE.txt" "$appdir/usr/share/doc/ezoo-download/ffmpeg-LICENSE.txt"
    fi
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
    rpm)      build_rpm ;;
    all)      build_deb; build_appimage; build_rpm ;;
    *)        echo "usage: $0 [deb|appimage|rpm|all]"; exit 1 ;;
esac
echo "==> Done."
