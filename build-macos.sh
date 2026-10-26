#!/bin/bash
# Build a native macOS app bundle and a ZIP suitable for GitHub Releases.
set -euo pipefail

cd "$(dirname "$0")"

usage() {
    echo "Usage: $0 [--target aarch64-apple-darwin|x86_64-apple-darwin] [--debug] [--skip-build]"
}

target=""
skip_build=false
debug=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target)
            [[ $# -ge 2 ]] || { usage >&2; exit 1; }
            target="$2"
            shift 2
            ;;
        --skip-build) skip_build=true; shift ;;
        --debug) debug=true; shift ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; exit 1 ;;
    esac
done

[[ "$(uname -s)" == Darwin ]] || { echo "Build this app on macOS." >&2; exit 1; }
command -v cargo >/dev/null || { echo "Install Rust from https://rustup.rs first." >&2; exit 1; }
xcrun --find clang >/dev/null 2>&1 || {
    echo "Install Apple's Command Line Tools with: xcode-select --install" >&2
    exit 1
}

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
cargo_target_dir="${CARGO_TARGET_DIR:-target}"
build_args=(-p swiftget --locked)
profile=release
if [[ "$debug" == true ]]; then
    profile=debug
else
    build_args+=(--release)
fi
if [[ -n "$target" ]]; then
    build_args+=(--target "$target")
    binary="$cargo_target_dir/$target/$profile/EzooDownload"
else
    target="$(rustc -vV | sed -n 's/^host: //p')"
    binary="$cargo_target_dir/$profile/EzooDownload"
fi
case "$target" in
    aarch64-apple-darwin) arch=arm64 ;;
    x86_64-apple-darwin) arch=x86_64 ;;
    *) echo "Unsupported macOS target: $target" >&2; exit 1 ;;
esac

if [[ "$skip_build" == false ]]; then
    cargo build "${build_args[@]}"
fi
[[ -x "$binary" ]] || { echo "Build output is missing: $binary" >&2; exit 1; }
lipo "$binary" -verify_arch "$arch"

version="$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' Cargo.toml | head -n 1)"
[[ -n "$version" ]] || { echo "Could not read the workspace version." >&2; exit 1; }
app="dist/macos-$arch/Ezoo Download.app"
archive="dist/EzooDownload-macos-$arch.zip"
icon_work="$(mktemp -d "${TMPDIR:-/tmp}/ezoo-macos.XXXXXX")"
trap 'rm -rf "$icon_work"' EXIT
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/EzooDownload"
cp THIRD-PARTY-NOTICES.txt "$app/Contents/Resources/"

# Both browsers use the same source, with their own manifest. Resources
# stay in the bundle; no writes to /Applications are needed at runtime.
for name in extension extension-firefox; do
    cp -R extension "$app/Contents/Resources/$name"
    rm -rf "$app/Contents/Resources/$name/test"
done
cp "$app/Contents/Resources/extension-firefox/manifest.firefox.json" \
    "$app/Contents/Resources/extension-firefox/manifest.json"
rm "$app/Contents/Resources/extension-firefox/manifest.firefox.json" \
    "$app/Contents/Resources/extension/manifest.firefox.json"
ext_version="$(sed -nE 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' extension/manifest.firefox.json | head -n 1)"
signed_xpi="extension-signed/ezoo-download-firefox-$ext_version.xpi"
if [[ -f "$signed_xpi" ]]; then
    cp "$signed_xpi" "$app/Contents/Resources/ezoo-download-firefox.xpi"
fi

# Convert the existing artwork using tools included with macOS.
mkdir "$icon_work/EzooDownload.iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" crates/app/ui/icon.png \
        --out "$icon_work/EzooDownload.iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" crates/app/ui/icon.png \
        --out "$icon_work/EzooDownload.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$icon_work/EzooDownload.iconset" -o "$app/Contents/Resources/EzooDownload.icns"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>CFBundleExecutable</key><string>EzooDownload</string>
    <key>CFBundleIdentifier</key><string>com.ezoo.EzooDownload</string>
    <key>CFBundleName</key><string>Ezoo Download</string>
    <key>CFBundleDisplayName</key><string>Ezoo Download</string>
    <key>CFBundleIconFile</key><string>EzooDownload.icns</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleURLTypes</key><array><dict>
        <key>CFBundleURLName</key><string>com.ezoo.EzooDownload</string>
        <key>CFBundleTypeRole</key><string>Viewer</string>
        <key>CFBundleURLSchemes</key><array><string>ezoo-download</string></array>
    </dict></array>
    <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
    <key>LSFileQuarantineEnabled</key><true/>
    <key>NSAppleEventsUsageDescription</key><string>Ezoo Download uses System Events when you choose to shut down your Mac after downloads finish.</string>
    <key>NSHumanReadableCopyright</key><string>Copyright © 2026 Ahmed Ezzat</string>
</dict></plist>
EOF
plutil -lint "$app/Contents/Info.plist"
# An ad-hoc signature lets local Apple Silicon builds run. Distribution
# still needs a Developer ID signature and notarization to satisfy Gatekeeper.
codesign --force --sign - "$app"
codesign --verify --strict "$app"
rm -f "$archive"
ditto -c -k --sequesterRsrc --keepParent "$app" "$archive"
(cd dist && shasum -a 256 "$(basename "$archive")" > "$(basename "$archive").sha256")

echo "Built: $app"
echo "Archive: $archive"
echo "Optional video/audio/torrent helpers: brew install yt-dlp ffmpeg aria2"
echo "These helpers are managed by Homebrew and are not bundled or updated by this app."
