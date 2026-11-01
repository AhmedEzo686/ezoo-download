#!/usr/bin/env bash
# Installs this folder for the current user only (no root, nothing outside
# ~/.local). Remove it again with the last lines of this file's output.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
prefix="${PREFIX:-$HOME/.local}"
lib="$prefix/lib/ezoodownload"

echo "Installing into $lib"
mkdir -p "$lib" "$prefix/bin" "$prefix/share/applications" "$prefix/share/icons/hicolor"
cp -f "$here/EzooDownload" "$lib/EzooDownload"
cp -f "$here/THIRD-PARTY-NOTICES.txt" "$lib/" 2>/dev/null || true
for item in extension extension-firefox ezoo-download-firefox.xpi ezoo-download-chrome.crx; do
    if [ -e "$here/$item" ]; then
        cp -a "$here/$item" "$lib/"
    fi
done
ln -sfn "$lib/EzooDownload" "$prefix/bin/EzooDownload"

for file in "$here"/icons/ezoodownload-*.png; do
    [ -e "$file" ] || continue
    size=$(basename "$file" .png | sed 's/^ezoodownload-//')
    mkdir -p "$prefix/share/icons/hicolor/${size}x${size}/apps"
    cp -f "$file" "$prefix/share/icons/hicolor/${size}x${size}/apps/ezoodownload.png"
done

# the menu entry with this copy's own path
sed -e "s|^Exec=EzooDownload |Exec=\"$lib/EzooDownload\" |" \
    -e "s|^TryExec=EzooDownload$|TryExec=$lib/EzooDownload|" \
    "$here/ezoodownload.desktop" > "$prefix/share/applications/ezoodownload.desktop"

if command -v update-desktop-database >/dev/null; then
    update-desktop-database -q "$prefix/share/applications" || true
fi
# no gtk-update-icon-cache here: a cache of only these icons is not complete
# and the desktops read ~/.local/share/icons without one
if command -v xdg-desktop-menu >/dev/null; then
    xdg-desktop-menu forceupdate >/dev/null 2>&1 || true
fi

echo
echo "Installed. Start it from the menu (Ezoo Download) or with: $prefix/bin/EzooDownload"
echo "To remove it again:"
echo "  rm -rf \"$lib\" \"$prefix/bin/EzooDownload\" \"$prefix/share/applications/ezoodownload.desktop\" \"$prefix/share/icons/hicolor\"/*/apps/ezoodownload.png"
