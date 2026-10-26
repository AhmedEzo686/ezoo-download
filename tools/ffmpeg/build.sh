#!/usr/bin/env bash
# Builds Ezoo Download's own small ffmpeg.exe for 64-bit Windows: only what the
# program uses (joining video + sound without re-encoding, the "play while
# downloading" preview, turning streams into MP4, and MP3 files with LAME).
# Nothing else is built: no network, no video encoders, no filters for video,
# no hardware access. LGPL build (LAME is LGPL too), linked statically.
#
#   tools/ffmpeg/build.sh <ffmpeg source dir> <lame source dir> <out dir>
#
# Needs a mingw-w64 cross compiler (x86_64-w64-mingw32-gcc / -clang on PATH).
set -euo pipefail
FF_SRC=$(realpath "$1"); LAME_SRC=$(realpath "$2"); OUT=$(realpath -m "$3")
HOST=x86_64-w64-mingw32
JOBS=$(nproc 2>/dev/null || echo 4)
WORK=$(mktemp -d)
PREFIX="$WORK/prefix"
mkdir -p "$PREFIX" "$OUT"
CC=${CC:-$HOST-gcc}
command -v "$CC" >/dev/null || CC=$HOST-clang
export CFLAGS="-O2 -fno-strict-aliasing"

echo "== LAME"
cp -r "$LAME_SRC" "$WORK/lame"
cd "$WORK/lame"
./configure --host=$HOST --prefix="$PREFIX" --enable-static --disable-shared \
  --disable-frontend --disable-decoder --disable-gtktest --enable-nasm=no CC="$CC" >/dev/null
make -j"$JOBS" >/dev/null 2>&1
make install >/dev/null

echo "== ffmpeg"
mkdir -p "$WORK/ff" && cd "$WORK/ff"
"$FF_SRC/configure" \
  --prefix="$PREFIX" --target-os=mingw32 --arch=x86_64 --enable-cross-compile \
  --cross-prefix=$HOST- --cc="$CC" \
  --pkg-config=pkg-config --pkg-config-flags=--static \
  --extra-cflags="-I$PREFIX/include" \
  --extra-ldflags="-L$PREFIX/lib -static -Wl,--dynamicbase -Wl,--nxcompat -Wl,--high-entropy-va" \
  --disable-everything --disable-autodetect --disable-network --disable-doc \
  --disable-ffplay --disable-ffprobe --disable-debug --disable-swscale \
  --disable-avdevice --disable-hwaccels --disable-iconv --disable-zlib --disable-bzlib --disable-lzma \
  --enable-static --disable-shared --enable-version3 \
  --enable-libmp3lame \
  --enable-protocol=file,subfile,pipe \
  --enable-demuxer=mov,matroska,mpegts,aac,mp3,ogg,flac,wav,ac3,eac3,webvtt,srt \
  --enable-muxer=mp4,ipod,mov,matroska,webm,mp3,mpegts,adts,ogg,opus \
  --enable-decoder=aac,aac_latm,opus,vorbis,mp3,mp3float,flac,ac3,eac3,pcm_s16le,pcm_s24le,pcm_f32le,alac \
  --enable-encoder=libmp3lame \
  --enable-parser=h264,hevc,av1,vp8,vp9,aac,aac_latm,opus,vorbis,mpegaudio,flac,ac3 \
  --enable-bsf=aac_adtstoasc,h264_mp4toannexb,hevc_mp4toannexb,extract_extradata,vp9_superframe,av1_frame_merge,av1_frame_split,dump_extradata,setts \
  --enable-filter=aresample,aformat,anull,null,format,abuffer,abuffersink,buffer,buffersink,atrim,trim,volume \
  --enable-swresample >"$WORK/configure.log" || { tail -40 "$WORK/configure.log"; exit 1; }
make -j"$JOBS" ffmpeg.exe >"$WORK/make.log" 2>&1 || { tail -40 "$WORK/make.log"; exit 1; }
$HOST-strip -o "$OUT/ffmpeg.exe" ffmpeg.exe
# what the build contains, next to the program (and the licences)
{ echo "Ezoo Download's own ffmpeg build"; echo "ffmpeg $(cat "$FF_SRC/RELEASE" 2>/dev/null)"; echo; grep -m1 "^  *--" "$WORK/configure.log" >/dev/null || true; ./ffmpeg.exe -version 2>/dev/null | head -1 || true; } >/dev/null
cp "$FF_SRC/COPYING.LGPLv3" "$OUT/ffmpeg-LICENSE.txt"
cp "$LAME_SRC/COPYING" "$OUT/lame-COPYING.txt"
sed -n '/^License:/,$p' "$WORK/configure.log" | head -3 >"$OUT/ffmpeg-build.txt" || true
echo "built $(du -h "$OUT/ffmpeg.exe" | cut -f1)"
rm -rf "$WORK"
