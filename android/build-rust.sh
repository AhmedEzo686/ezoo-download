#!/usr/bin/env bash
# The download engine for Android (crates/android -> libezoo.so), for the
# phones' processors, into app/src/main/jniLibs where the APK takes it from.
#
#   android/build-rust.sh            (needs the Android NDK and cargo-ndk)
#
# Rust's Android targets: rustup target add aarch64-linux-android \
#   armv7-linux-androideabi x86_64-linux-android
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="$ROOT/android/app/src/main/jniLibs"
rm -rf "$OUT"/*/libezoo.so
# arm64-v8a: nearly every phone of the last years; armeabi-v7a: older and
# cheaper ones (the ABIs of app/build.gradle.kts)
(cd "$ROOT" && cargo ndk -t arm64-v8a -t armeabi-v7a -P 24 -o "$OUT" \
  build --release --locked -p ezoo-android)
ls -la "$OUT"/*
