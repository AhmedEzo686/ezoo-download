#!/usr/bin/env bash
# Ezoo Download's own small ffmpeg for Android: what the app uses of it
# (joining a video's picture and sound without re-encoding, turning sound
# into MP3 with LAME, streams into MP4). Nothing else: no network, no
# devices, no hardware. LGPL build (LAME is LGPL), linked statically.
#
#   tools/ffmpeg/build-android.sh <ffmpeg source dir> <lame source dir> <jniLibs dir> [ABI...]
#
# Needs the Android NDK ($ANDROID_NDK_HOME). Each ABI gets
# <jniLibs dir>/<ABI>/libffmpeg.so: the ffmpeg program itself, named like a
# library so Android installs it where the app may run it.
set -euo pipefail
FF_SRC=$(realpath "$1"); LAME_SRC=$(realpath "$2"); OUT=$(realpath -m "$3"); shift 3
[ $# -gt 0 ] && ABIS=("$@") || ABIS=(arm64-v8a armeabi-v7a)
API=24
TC="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64"
JOBS=$(nproc 2>/dev/null || echo 4)

for ABI in "${ABIS[@]}"; do
  case "$ABI" in
    arm64-v8a)   TRIPLE=aarch64-linux-android; FF_ARCH=aarch64; LAME_HOST=aarch64-linux ;;
    armeabi-v7a) TRIPLE=armv7a-linux-androideabi; FF_ARCH=arm; LAME_HOST=arm-linux ;;
    x86_64)      TRIPLE=x86_64-linux-android; FF_ARCH=x86_64; LAME_HOST=x86_64-linux ;;
    *) echo "unknown ABI $ABI"; exit 1 ;;
  esac
  CC="$TC/bin/$TRIPLE$API-clang"
  WORK=$(mktemp -d)
  PREFIX="$WORK/prefix"
  mkdir -p "$PREFIX" "$OUT/$ABI"
  echo "== $ABI: LAME"
  cp -r "$LAME_SRC" "$WORK/lame"
  (cd "$WORK/lame" && ./configure --host=$LAME_HOST --prefix="$PREFIX" --enable-static --disable-shared \
    --disable-frontend --disable-decoder --disable-gtktest --enable-nasm=no \
    CC="$CC" AR="$TC/bin/llvm-ar" RANLIB="$TC/bin/llvm-ranlib" CFLAGS="-O2 -fPIC" >/dev/null &&
    make -j"$JOBS" >/dev/null 2>&1 && make install >/dev/null) || { echo "LAME failed ($ABI)"; exit 1; }

  echo "== $ABI: ffmpeg"
  mkdir -p "$WORK/ff" && cd "$WORK/ff"
  "$FF_SRC/configure" \
    --prefix="$PREFIX" --target-os=android --arch=$FF_ARCH --enable-cross-compile \
    --cc="$CC" --cxx="$CC++" --ar="$TC/bin/llvm-ar" --nm="$TC/bin/llvm-nm" --ranlib="$TC/bin/llvm-ranlib" \
    --strip="$TC/bin/llvm-strip" \
    --extra-cflags="-I$PREFIX/include -fPIE" --extra-ldflags="-L$PREFIX/lib -pie" \
    --disable-everything --disable-autodetect --disable-doc --disable-asm \
    --disable-ffplay --disable-ffprobe --disable-debug \
    --disable-avdevice --disable-hwaccels --disable-iconv --disable-zlib --disable-bzlib --disable-lzma \
    --enable-static --disable-shared --enable-version3 \
    --enable-libmp3lame \
    --enable-protocol=file,pipe \
    --enable-demuxer=mov,matroska,mpegts,aac,mp3,ogg,flac,wav,ac3,eac3,webvtt,srt \
    --enable-muxer=mp4,ipod,mov,matroska,webm,mp3,mpegts,adts,ogg,opus \
    --enable-decoder=aac,aac_latm,opus,vorbis,mp3,mp3float,flac,ac3,eac3,pcm_s16le,pcm_s24le,pcm_f32le,alac \
    --enable-encoder=libmp3lame,aac \
    --enable-parser=h264,hevc,av1,vp8,vp9,aac,aac_latm,opus,vorbis,mpegaudio,flac,ac3 \
    --enable-bsf=aac_adtstoasc,h264_mp4toannexb,hevc_mp4toannexb,extract_extradata,vp9_superframe,av1_frame_merge,av1_frame_split,dump_extradata,setts \
    --enable-filter=aresample,aformat,anull,null,format,abuffer,abuffersink,buffer,buffersink,atrim,trim,volume,setpts,asetpts \
    --enable-swresample >"$WORK/configure.log" || { tail -40 "$WORK/configure.log"; exit 1; }
  make -j"$JOBS" ffmpeg >"$WORK/make.log" 2>&1 || { tail -40 "$WORK/make.log"; exit 1; }
  "$TC/bin/llvm-strip" -o "$OUT/$ABI/libffmpeg.so" ffmpeg
  echo "built $ABI: $(du -h "$OUT/$ABI/libffmpeg.so" | cut -f1)"
  cd /; rm -rf "$WORK"
done
