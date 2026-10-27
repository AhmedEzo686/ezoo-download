#!/usr/bin/env bash
# Ezoo Download — fetch the bundled ffmpeg for the Linux packages.
#
# Usage:  ./packaging/linux/fetch-ffmpeg.sh <out-dir>
# Result: <out-dir>/ffmpeg, <out-dir>/ffmpeg-version.txt, <out-dir>/ffmpeg-LICENSE.txt
#
# The Linux packages bundle a static ffmpeg build (johnvansickle.com) so
# beginners never have to install ffmpeg themselves and early versions
# avoid the download complications of 1.0.7's deleted ffmpeg download.
# A later version may switch to the user's system install instead.
#
# The download happens at PACKAGING time and the binary NEVER enters the
# source tree (see .gitignore): this script only stages into <out-dir>.
#
# Integrity: johnvansickle.com publishes only an MD5 file next to the
# tarball (no SHA-256), so the build pins the exact MD5 of the tarball
# below and FAILS on any mismatch. The "release" file is replaced when a
# new FFmpeg release ships: refresh this pin deliberately (fetch the
# current ...tar.xz.md5, check the version the tarball reports, update
# the pin and the version note), never blindly. The script also prints
# the SHA-256 of the downloaded file for the record.
#
# License: johnvansickle.com static builds are GPLv3 (they include
# libx264/libx265/libmp3lame and other GPL components), unlike the small
# LGPL build the Windows setup carries. The tarball's GPLv3.txt is staged
# as ffmpeg-LICENSE.txt next to the binary; see THIRD-PARTY-NOTICES.txt.
set -euo pipefail

OUT="${1:?usage: fetch-ffmpeg.sh <out-dir>}"
URL="https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz"
# MD5 of the exact tarball, published by johnvansickle.com alongside it
# (...tar.xz.md5), verified 2026-10-27. See the header: refresh deliberately.
EXPECTED_MD5="7fa72b652e19bf84c9461e332ea1cdf3"

command -v curl >/dev/null 2>&1 || { echo "fetch-ffmpeg.sh: curl not found" >&2; exit 1; }
command -v md5sum >/dev/null 2>&1 || { echo "fetch-ffmpeg.sh: md5sum not found" >&2; exit 1; }

mkdir -p "$OUT"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "==> Downloading the pinned static ffmpeg build…"
curl -sSfL -o "$WORK/ffmpeg-release-amd64-static.tar.xz" "$URL"
echo "$EXPECTED_MD5  $WORK/ffmpeg-release-amd64-static.tar.xz" | md5sum -c - \
    || { echo "fetch-ffmpeg.sh: MD5 mismatch — the published build changed; refresh the pin deliberately, do not ship an unchecked binary" >&2; exit 1; }
echo "    MD5 OK. SHA-256 of the tarball (for the record):"
sha256sum "$WORK/ffmpeg-release-amd64-static.tar.xz" | sed 's/^/    /'

tar -xJf "$WORK/ffmpeg-release-amd64-static.tar.xz" -C "$WORK"
TOP="$(echo "$WORK"/ffmpeg-*-amd64-static)"
[ -d "$TOP" ] || { echo "fetch-ffmpeg.sh: unexpected tarball layout (no ffmpeg-*-amd64-static dir)" >&2; exit 1; }
[ -x "$TOP/ffmpeg" ] || { echo "fetch-ffmpeg.sh: no ffmpeg binary in the tarball" >&2; exit 1; }

# only the ffmpeg binary is bundled: ffprobe, qt-faststart, manpages and
# the model files stay out of the packages
cp "$TOP/ffmpeg" "$OUT/ffmpeg"
chmod 755 "$OUT/ffmpeg"
if [ -f "$TOP/GPLv3.txt" ]; then
    cp "$TOP/GPLv3.txt" "$OUT/ffmpeg-LICENSE.txt"
else
    # the publisher's licence file moved: ship a pointer instead of failing
    # the whole packaging run, and say so loudly
    echo "fetch-ffmpeg.sh: WARNING: GPLv3.txt not in the tarball; writing a licence pointer instead" >&2
    printf 'The ffmpeg binary bundled with the Linux packages is a static build by johnvansickle.com (https://johnvansickle.com/ffmpeg/), licensed under the GNU General Public License version 3.\nThe full licence text: https://www.gnu.org/licenses/gpl-3.0.txt\n' > "$OUT/ffmpeg-LICENSE.txt"
fi
# the exact build, for the package and for bug reports
"$OUT/ffmpeg" -hide_banner -version 2>/dev/null | head -1 > "$OUT/ffmpeg-version.txt" \
    || { echo "fetch-ffmpeg.sh: the staged ffmpeg does not run on this machine" >&2; exit 1; }
echo "    $(cat "$OUT/ffmpeg-version.txt")"
echo "    staged $(du -h "$OUT/ffmpeg" | cut -f1) in $OUT"
