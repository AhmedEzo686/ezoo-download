#!/usr/bin/env bash
# Ezoo Download — the ffmpeg that goes inside the Linux packages.
#
#   ./packaging/linux/fetch-ffmpeg.sh <out-dir>
#   -> <out-dir>/ffmpeg, ffmpeg-version.txt, ffmpeg-LICENSE.txt
#
# A static build (johnvansickle.com) is used so the packages need no system
# ffmpeg: a dynamically linked copy would drag a dozen shared libraries into
# the package, and the distribution's package names differ anyway.
#
# The download happens at PACKAGING time and the binary never enters the
# source tree; this script only stages into <out-dir>.
#
# Integrity: the publisher ships only an MD5 next to the tarball, so the
# exact MD5 is pinned here and a mismatch stops the build. Refresh the pin
# deliberately when a new static build is published (fetch the .md5, check
# the version the binary reports, update both the pin and the note below).
#
# License: these static builds are GPLv3 (they include libx264/libx265 and
# libmp3lame), unlike the small LGPL build the Windows setup carries. The
# tarball's GPLv3.txt is staged next to the binary and listed in
# THIRD-PARTY-NOTICES.txt.
set -euo pipefail

OUT="${1:?usage: fetch-ffmpeg.sh <out-dir>}"
URL="https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz"
EXPECTED_MD5="7fa72b652e19bf84c9461e332ea1cdf3"

command -v curl >/dev/null 2>&1 || { echo "fetch-ffmpeg.sh: curl not found" >&2; exit 1; }
command -v md5sum >/dev/null 2>&1 || { echo "fetch-ffmpeg.sh: md5sum not found" >&2; exit 1; }

mkdir -p "$OUT"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "==> Downloading the pinned static ffmpeg build…"
curl -sSfL -o "$WORK/ffmpeg.tar.xz" "$URL"
echo "$EXPECTED_MD5  $WORK/ffmpeg.tar.xz" | md5sum -c - \
    || { echo "fetch-ffmpeg.sh: MD5 mismatch — the published build changed; refresh the pin deliberately, do not ship an unchecked binary" >&2; exit 1; }
echo "    MD5 OK. SHA-256 of the tarball (for the record):"
sha256sum "$WORK/ffmpeg.tar.xz" | sed 's/^/    /'

tar -xJf "$WORK/ffmpeg.tar.xz" -C "$WORK"
TOP="$(echo "$WORK"/ffmpeg-*-amd64-static)"
[ -d "$TOP" ] || { echo "fetch-ffmpeg.sh: unexpected tarball layout" >&2; exit 1; }
[ -x "$TOP/ffmpeg" ] || { echo "fetch-ffmpeg.sh: no ffmpeg binary in the tarball" >&2; exit 1; }

# only ffmpeg itself: ffprobe, qt-faststart, man pages and model files stay out
cp "$TOP/ffmpeg" "$OUT/ffmpeg"
chmod 755 "$OUT/ffmpeg"
if [ -f "$TOP/GPLv3.txt" ]; then
    cp "$TOP/GPLv3.txt" "$OUT/ffmpeg-LICENSE.txt"
else
    echo "fetch-ffmpeg.sh: WARNING: GPLv3.txt missing; writing a licence pointer" >&2
    printf 'The ffmpeg bundled with the Linux packages is a static build by johnvansickle.com, licensed under the GNU General Public License version 3.\nThe full licence text: https://www.gnu.org/licenses/gpl-3.0.txt\n' > "$OUT/ffmpeg-LICENSE.txt"
fi
"$OUT/ffmpeg" -hide_banner -version 2>/dev/null | head -1 > "$OUT/ffmpeg-version.txt" \
    || { echo "fetch-ffmpeg.sh: the staged ffmpeg does not run on this machine" >&2; exit 1; }
echo "    $(cat "$OUT/ffmpeg-version.txt")"
echo "    staged $(du -h "$OUT/ffmpeg" | cut -f1) in $OUT"
