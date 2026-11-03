#!/usr/bin/env bash
# Ezoo Download — the Flatpak (dist/EzooDownload-x86_64.flatpak).
#
#   ./packaging/flatpak/build-flatpak.sh <version> <stage-dir> [ffmpeg-dir]
#
# <stage-dir> is the one build-linux.sh prepares: EzooDownload, icons/,
# extension/, extension-firefox/, the signed .xpi and THIRD-PARTY-NOTICES.txt.
#
# The program is already built (and the ffmpeg is the same static one the
# other packages carry), so this does not use flatpak-builder: it assembles
# the files, writes the metadata, and lets flatpak export and bundle them.
# That keeps the Flatpak identical to the .deb — same binary, same tools.
set -euo pipefail

version="${1:?usage: build-flatpak.sh <version> <stage-dir> [ffmpeg-dir]}"
stage="${2:?}"
ffdir="${3:-}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
app_id="io.github.aelrased.ezoo-download"
runtime="${FLATPAK_RUNTIME:-org.freedesktop.Platform/x86_64/26.08}"

command -v flatpak >/dev/null 2>&1 || { echo "flatpak is not installed" >&2; exit 1; }
flatpak info "$runtime" >/dev/null 2>&1 || {
    echo "runtime $runtime is not installed: flatpak install flathub $runtime" >&2
    exit 1
}

build="$stage/flatpak-build"
repo="$stage/flatpak-repo"
rm -rf "$build" "$repo"
mkdir -p "$build/files/bin" \
         "$build/files/share/applications" \
         "$build/files/share/metainfo" \
         "$build/files/share/ezoodownload" \
         "$build/files/share/doc/$app_id"

install -m755 "$stage/EzooDownload" "$build/files/bin/EzooDownload"
if [ -n "$ffdir" ] && [ -x "$ffdir/ffmpeg" ]; then
    install -m755 "$ffdir/ffmpeg" "$build/files/bin/ffmpeg"
    cp "$ffdir/ffmpeg-version.txt" "$build/files/bin/ffmpeg-version.txt"
    cp "$ffdir/ffmpeg-LICENSE.txt" "$build/files/share/doc/$app_id/ffmpeg-LICENSE.txt"
fi
cp "$here/$app_id.desktop" "$build/files/share/applications/"
cp "$here/$app_id.metainfo.xml" "$build/files/share/metainfo/"

# icons under the application id, which is what a Flatpak is expected to ship
for file in "$stage"/icons/ezoodownload-*.png; do
    [ -e "$file" ] || continue
    size=$(basename "$file" .png | sed 's/^ezoodownload-//')
    install -Dm644 "$file" "$build/files/share/icons/hicolor/${size}x${size}/apps/$app_id.png"
done

for item in extension extension-firefox ezoo-download-firefox.xpi THIRD-PARTY-NOTICES.txt; do
    [ -e "$stage/$item" ] && cp -a "$stage/$item" "$build/files/share/ezoodownload/" || true
done

cat > "$build/metadata" <<EOF
[Application]
name=$app_id
runtime=$runtime
sdk=${runtime/Platform/Sdk}
command=EzooDownload

[Context]
shared=ipc;network;
sockets=x11;wayland;pulseaudio;
devices=dri;
filesystems=home;
talk-name=org.freedesktop.portal.Desktop;org.freedesktop.Notifications;org.freedesktop.FileManager1;org.kde.StatusNotifierWatcher;
EOF

# the permissions a download manager needs: the browser extension talks to
# 127.0.0.1:9614, so the network is shared with the host
flatpak build-finish "$build" \
    --command=EzooDownload \
    --share=network --share=ipc \
    --socket=x11 --socket=wayland --socket=pulseaudio \
    --device=dri --filesystem=home \
    --talk-name=org.freedesktop.portal.Desktop \
    --talk-name=org.freedesktop.Notifications \
    --talk-name=org.freedesktop.FileManager1 \
    --talk-name=org.kde.StatusNotifierWatcher >/dev/null

flatpak build-export "$repo" "$build" "$version" >/dev/null
mkdir -p "$root/dist"
flatpak build-bundle "$repo" "$root/dist/EzooDownload-x86_64.flatpak" "$app_id" "$version" >/dev/null
echo "   dist/EzooDownload-x86_64.flatpak ($(du -h "$root/dist/EzooDownload-x86_64.flatpak" | cut -f1))"
