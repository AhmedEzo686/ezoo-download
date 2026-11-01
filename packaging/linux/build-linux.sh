#!/usr/bin/env bash
# Ezoo Download for Linux: builds the .deb and a portable folder with an
# install script that needs no root.
#
#   ./packaging/linux/build-linux.sh            # build everything into dist/
#
# Needs: Rust (cargo), dpkg-deb (for the .deb), ImageMagick for the icons
# (convert), and the usual -dev packages of X11/Wayland (Slint links them).
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$here/../.." && pwd)
version=$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
[ -n "$version" ] || { echo "no version in Cargo.toml"; exit 1; }
arch=$(dpkg --print-architecture 2>/dev/null || echo amd64)
dist="$root/dist"
stage="$root/build-linux"
bin="$root/target/release/EzooDownload"
appdir="$stage/EzooDownload-$version-linux-x86_64"

say() { printf '\n==> %s\n' "$*"; }

say "Building EzooDownload $version (release)"
cargo build --release --manifest-path "$root/Cargo.toml"
[ -x "$bin" ] || { echo "the build did not make $bin"; exit 1; }

say "Collecting the files"
rm -rf "$stage"
mkdir -p "$dist" "$stage"
install -Dm755 "$bin" "$stage/EzooDownload"
# Chrome/Edge/Opera/Brave get the folder as it is; Firefox gets the same files
# with its own manifest.json (the source has one copy of each file)
cp -a "$root/extension" "$stage/extension"
cp -a "$root/extension" "$stage/extension-firefox"
rm -rf "$stage/extension/test" "$stage/extension-firefox/test"
if [ -f "$stage/extension-firefox/manifest.firefox.json" ]; then
    mv -f "$stage/extension-firefox/manifest.firefox.json" "$stage/extension-firefox/manifest.json"
fi
rm -f "$stage/extension/manifest.firefox.json"
cp -f "$root/extension-signed/"*firefox-*.xpi "$stage/ezoo-download-firefox.xpi" 2>/dev/null || true
cp -f "$root/THIRD-PARTY-NOTICES.txt" "$stage/"
cp -f "$here/ezoodownload.desktop" "$stage/"

say "The browser extension as a download (Chrome: unpack it, Load unpacked)"
extver=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$stage/extension/manifest.json" | head -1)
[ -n "$extver" ] || extver="$version"
python3 - "$stage/extension" "$dist/ezoo-download-chrome-$extver.zip" <<'PY'
import os, sys, zipfile
src, out = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for root, dirs, files in os.walk(src):
        dirs.sort()
        for name in sorted(files):
            path = os.path.join(root, name)
            z.write(path, os.path.relpath(path, src))
PY

# a .crx as well, for the browsers and enterprise policies that still accept
# one. Chrome itself only installs its Web Store or "Load unpacked" (a local
# .crx is refused), so this is the archive form of the same extension.
chrome_bin=""
for c in google-chrome google-chrome-stable chromium chromium-browser; do
    if command -v "$c" >/dev/null; then chrome_bin="$c"; break; fi
done
if [ -n "$chrome_bin" ]; then
    rm -rf "$stage/packext" "$stage/packext.crx" "$stage/packext.pem"
    cp -a "$stage/extension" "$stage/packext"
    if "$chrome_bin" --headless=new --pack-extension="$stage/packext" --no-message-box \
        >"$stage/pack.log" 2>&1 && [ -f "$stage/packext.crx" ]; then
        mv -f "$stage/packext.crx" "$dist/ezoo-download-chrome-$extver.crx"
        echo "   .crx: dist/ezoo-download-chrome-$extver.crx"
    else
        echo "   could not pack the .crx (see $stage/pack.log)"
    fi
    rm -rf "$stage/packext" "$stage/packext.pem"
fi

say "Making the icons"
mkdir -p "$stage/icons"
for size in 16 24 32 48 64 128 256; do
    convert "$root/crates/app/ui/icon.png" -resize "${size}x${size}" "$stage/icons/ezoodownload-$size.png"
done
# the .desktop file and every file manager want one with the plain name
cp "$stage/icons/ezoodownload-256.png" "$stage/icons/ezoodownload.png"

say "The portable folder (no root needed)"
mkdir -p "$appdir/icons"
install -m755 "$stage/EzooDownload" "$appdir/EzooDownload"
cp -a "$stage/extension" "$stage/extension-firefox" "$stage/ezoo-download-firefox.xpi" \
      "$stage/THIRD-PARTY-NOTICES.txt" "$stage/ezoodownload.desktop" "$appdir/" 2>/dev/null || true
cp "$stage/icons/"*.png "$appdir/icons/"
cp -f "$here/install-local.sh" "$appdir/install.sh"
chmod +x "$appdir/install.sh"
(cd "$stage" && tar czf "$dist/EzooDownload-$version-linux-x86_64.tar.gz" "$(basename "$appdir")")

say "The AppImage (one file, no install)"
tool="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
if [ -z "$tool" ] && [ -x "$HOME/.local/bin/appimagetool" ]; then
    tool="$HOME/.local/bin/appimagetool"
fi
if [ -n "$tool" ]; then
    imgdir="$stage/EzooDownload.AppDir"
    rm -rf "$imgdir"
    mkdir -p "$imgdir/usr/bin" "$imgdir/usr/lib" "$imgdir/usr/share/ezoodownload"
    install -m755 "$stage/EzooDownload" "$imgdir/usr/bin/EzooDownload"
    # where the program looks for them when it runs from inside the image
    for name in extension extension-firefox ezoo-download-firefox.xpi THIRD-PARTY-NOTICES.txt; do
        [ -e "$stage/$name" ] || continue
        cp -a "$stage/$name" "$imgdir/usr/share/ezoodownload/"
        cp -a "$stage/$name" "$imgdir/usr/bin/"
    done
    # the libraries the program links itself (the ones every desktop has are
    # left to the system: libc, X11, Wayland, OpenGL)
    for lib in libfontconfig.so.1 libfreetype.so.6 libexpat.so.1 libz.so.1 libzstd.so.1 \
               libbz2.so.1.0 libpng16.so.16 libbrotlidec.so.1 libbrotlicommon.so.1; do
        for dir in /usr/lib/x86_64-linux-gnu /usr/lib64 /usr/lib /lib/x86_64-linux-gnu /lib64 /lib; do
            if [ -e "$dir/$lib" ]; then
                cp -L "$dir/$lib" "$imgdir/usr/lib/"
                break
            fi
        done
    done
    install -m644 "$stage/ezoodownload.desktop" "$imgdir/ezoodownload.desktop"
    cp "$stage/icons/ezoodownload-256.png" "$imgdir/ezoodownload.png"
    cat > "$imgdir/AppRun" <<'EOF'
#!/bin/sh
# AppImage entry point: find our own folder, then start the program.
here="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="$here/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export PATH="$here/usr/bin:$PATH"
exec "$here/usr/bin/EzooDownload" "$@"
EOF
    chmod 755 "$imgdir/AppRun"
    # extract-and-run: appimagetool itself is an AppImage, and FUSE is not
    # there in every container / build machine
    if APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" --no-appstream \
        "$imgdir" "$dist/EzooDownload-$version-x86_64.AppImage" >"$stage/appimage.log" 2>&1; then
        chmod 755 "$dist/EzooDownload-$version-x86_64.AppImage"
    else
        echo "appimagetool failed (see $stage/appimage.log); the .deb and the folder are still built"
    fi
else
    echo "appimagetool not found: put it in ~/.local/bin or set APPIMAGETOOL=... to also make the AppImage"
fi

say "The .deb package"
deb="$stage/deb"
mkdir -p "$deb/DEBIAN" \
         "$deb/usr/bin" \
         "$deb/usr/share/applications" \
         "$deb/usr/share/ezoodownload" \
         "$deb/usr/share/doc/ezoodownload" \
         "$deb/usr/share/icons/hicolor"
install -Dm755 "$stage/EzooDownload" "$deb/usr/bin/EzooDownload"
install -Dm644 "$stage/ezoodownload.desktop" "$deb/usr/share/applications/ezoodownload.desktop"
install -Dm644 "$stage/THIRD-PARTY-NOTICES.txt" "$deb/usr/share/doc/ezoodownload/THIRD-PARTY-NOTICES.txt"
for name in extension extension-firefox; do
    cp -a "$stage/$name" "$deb/usr/share/ezoodownload/$name"
done
cp -f "$stage/ezoo-download-firefox.xpi" "$deb/usr/share/ezoodownload/" 2>/dev/null || true
for size in 16 24 32 48 64 128 256; do
    install -Dm644 "$stage/icons/ezoodownload-$size.png" \
        "$deb/usr/share/icons/hicolor/${size}x${size}/apps/ezoodownload.png"
done

# what the program links, in the names this distribution uses
# OpenSSL is built into the program; the rest is what it links, plus the
# X11 / Wayland / OpenGL libraries the toolkit loads when the window is made
deps="libc6, libgcc-s1, libfontconfig1, libfreetype6, libxkbcommon0, libx11-6, libxcb1, libxi6, libxcursor1, libxrandr2, libegl1, libgl1, libwayland-client0, libwayland-cursor0, libdbus-1-3"
if command -v dpkg-shlibdeps >/dev/null; then
    shl="$stage/shlibs"
    mkdir -p "$shl/debian"
    printf 'Source: ezoodownload\n\nPackage: ezoodownload\nArchitecture: any\n' > "$shl/debian/control"
    if found=$(cd "$shl" && dpkg-shlibdeps -O "$deb/usr/bin/EzooDownload" 2>/dev/null); then
        found=${found#shlibs:Depends=}
        [ -n "$found" ] && deps="$found"
    fi
fi

installed_size=$(du -sk "$deb/usr" | cut -f1)
cat > "$deb/DEBIAN/control" <<EOF
Package: ezoodownload
Version: $version
Section: net
Priority: optional
Architecture: $arch
Maintainer: Ahmed Ezzat <https://github.com/AhmedEzo686/ezoo-download>
Installed-Size: $installed_size
Depends: $deps
Recommends: ffmpeg, aria2, yt-dlp, xdg-desktop-portal
Homepage: https://github.com/AhmedEzo686/ezoo-download
Description: Ezoo Download - download manager
 Downloads files and videos over several connections, with pause, resume,
 queues, a scheduler, a speed limiter, torrents (through aria2) and a browser
 extension. Eight languages, including Arabic.
EOF

cat > "$deb/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null; then
    update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache -q -t /usr/share/icons/hicolor || true
fi
exit 0
EOF

cat > "$deb/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null; then
    update-desktop-database -q /usr/share/applications || true
fi
exit 0
EOF
chmod 755 "$deb/DEBIAN/postinst" "$deb/DEBIAN/postrm"
chmod 644 "$deb/DEBIAN/control"

dpkg-deb --root-owner-group --build "$deb" "$dist/ezoodownload_${version}_${arch}.deb" >/dev/null

say "Done"
ls -lh "$dist" | sed 's/^/   /'
echo
echo "Install:  sudo apt install ./dist/ezoodownload_${version}_${arch}.deb"
echo "Or:       tar xzf dist/EzooDownload-${version}-linux-x86_64.tar.gz && ./EzooDownload-${version}-linux-x86_64/install.sh"
if [ -f "$dist/EzooDownload-$version-x86_64.AppImage" ]; then
    echo "AppImage: chmod +x dist/EzooDownload-${version}-x86_64.AppImage && ./dist/EzooDownload-${version}-x86_64.AppImage"
fi
