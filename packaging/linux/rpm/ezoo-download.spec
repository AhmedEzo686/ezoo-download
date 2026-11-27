# ezoo-download — RPM packaging (Fedora / RHEL / openSUSE and derivatives)
#
# Built by packaging/linux/build-linux.sh (target "rpm"), which supplies
# Source0 (a tarball of the staged /usr tree) and rewrites Version from
# Cargo.toml. Do not edit the Version by hand.
#
# The staged tree is the same one the .deb uses: the program in /usr/bin,
# the bundled static ffmpeg in /usr/lib/ezoodownload, the desktop file,
# hicolor icons, and the browser extension sources.

Name:           ezoo-download
Version:        %{?ver}%{!?ver:1.1.5}
Release:        1%{?dist}
Summary:        Download manager with multi-connection segmented downloads
License:        MIT
URL:            https://github.com/aelrased/ezoo-download
Source0:        root.tar.gz
BuildArch:      x86_64
# the same runtime closure the .deb declares, in Fedora names
Requires:       fontconfig, freetype, libxkbcommon, libX11, libxcb, zlib, dbus-libs
Recommends:     xdg-desktop-portal, aria2, yt-dlp

%description
Ezoo Download is a desktop download manager written in Rust (Slint UI).
It splits files over up to 32 simultaneous connections, resumes
interrupted downloads, saves video and audio through yt-dlp + ffmpeg,
and ships a browser extension that sends links from Chrome and Firefox.

This Linux edition adds a native StatusNotifierItem system tray
(KDE / GNOME / XFCE), XDG autostart, freedesktop hicolor icons and
systemd power integration. Data is stored under the XDG directories.
A static ffmpeg build is bundled inside the package, so no system
ffmpeg is needed.

%install
mkdir -p %{buildroot}
tar -xzf %{SOURCE0} -C %{buildroot}

%files
/usr/bin/EzooDownload
/usr/bin/ezoo-download-chrome.crx
/usr/lib/ezoodownload/
/usr/share/applications/ezoodownload.desktop
/usr/share/doc/ezoodownload/
/usr/share/ezoodownload/
/usr/share/icons/hicolor/

%post
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi

%postun
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications >/dev/null 2>&1 || true
fi
