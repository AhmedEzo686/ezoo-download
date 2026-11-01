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
