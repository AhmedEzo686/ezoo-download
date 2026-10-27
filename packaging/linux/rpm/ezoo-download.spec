# ezoo-download — RPM packaging (Fedora / RHEL / openSUSE and derivatives)
#
# Built by packaging/linux/build-linux.sh (target "rpm"), which supplies
# Source0 (a tarball of the staged /usr tree) and rewrites the Version
# below from Cargo.toml. Do not edit the Version by hand.
#
# The staged tree is the same one the .deb uses: the program in /usr/bin,
# the bundled static ffmpeg in /usr/lib/ezoo-download, desktop file,
# hicolor icons, AppStream metadata and the browser extension sources.

Name:           ezoo-download
Version:        1.0.7
Release:        1%{?dist}
Summary:        Download manager with multi-connection segmented downloads
License:        MIT
URL:            https://github.com/AhmedEzo686/ezoo-download
Source0:        root.tar.gz
BuildArch:      x86_64
# Runtime libraries: the same closure as the .deb's Depends, Fedora names.
Requires:       fontconfig, freetype, libxkbcommon, libX11, libxcb, openssl-libs, zlib
Recommends:     xdg-desktop-portal

%description
Ezoo Download is a desktop download manager written in Rust (Slint UI).
It splits files over up to 32 simultaneous connections, resumes
interrupted downloads, saves video and audio through yt-dlp + ffmpeg,
and ships a browser extension that sends links from Chrome and Firefox.

This Linux edition adds a native StatusNotifierItem system tray
(KDE / GNOME / XFCE), XDG autostart, freedesktop hicolor icons and a
systemd power integration. Data is stored under the XDG directories.
A static ffmpeg build is bundled inside the package, so no system
ffmpeg is needed.

%install
mkdir -p %{buildroot}
tar -xzf %{SOURCE0} -C %{buildroot}

%files
/usr/bin/EzooDownload
/usr/lib/ezoo-download/
/usr/share/applications/ezoo-download.desktop
/usr/share/doc/ezoo-download/
/usr/share/ezoo-download/
/usr/share/icons/hicolor/
/usr/share/metainfo/io.github.ahmedezo686.ezoo-download.metainfo.xml

%post
# best-effort refresh of the desktop and icon caches after install/upgrade
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi
