#!/usr/bin/env bash
# Ezoo Download — the macOS app: an .app bundle, a .dmg and a .zip.
#
#   ./build-macos.sh [--target aarch64-apple-darwin|x86_64-apple-darwin] [--skip-build]
#
# Build this on macOS (a real machine or a GitHub macOS runner): it needs
# Apple's toolchain (xcrun, sips, iconutil, codesign, hdiutil). The bundle is
# signed ad-hoc, which is enough for anyone to run it after the first
# right-click > Open (or: xattr -d com.apple.quarantine "Ezoo Download.app").
set -euo pipefail

cd "$(dirname "$0")"

target=""
skip_build=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target) target="$2"; shift 2 ;;
        --skip-build) skip_build=true; shift ;;
        -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

[[ "$(uname -s)" == Darwin ]] || { echo "Build this on macOS: the Apple toolchain is required." >&2; exit 1; }
command -v cargo >/dev/null || { echo "Install Rust from https://rustup.rs first." >&2; exit 1; }
xcrun --find clang >/dev/null 2>&1 || { echo "Run: xcode-select --install" >&2; exit 1; }

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"

if [[ -z "$target" ]]; then
    target="$(rustc -vV | sed -n 's/^host: //p')"
fi
case "$target" in
    aarch64-apple-darwin) arch=arm64 ;;
    x86_64-apple-darwin) arch=x86_64 ;;
    *) echo "Unsupported macOS target: $target" >&2; exit 1 ;;
esac

version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
[[ -n "$version" ]] || { echo "no version in Cargo.toml" >&2; exit 1; }

if [[ "$skip_build" == false ]]; then
    echo "==> Building EzooDownload $version for $target"
    cargo build --release -p swiftget --target "$target"
fi
binary="target/$target/release/EzooDownload"
[[ -x "$binary" ]] || { echo "the build made no $binary" >&2; exit 1; }

app="dist/macos-$arch/Ezoo Download.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m755 "$binary" "$app/Contents/MacOS/EzooDownload"

# the window icon, as the .icns Finder and the Dock read
iconset="$(mktemp -d "${TMPDIR:-/tmp}/ezoo-iconset.XXXXXX")"
mkdir -p "$iconset/AppIcon.iconset"
for size in 16 32 64 128 256 512; do
    sips -z "$size" "$size" crates/app/ui/icon.png --out "$iconset/AppIcon.iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" crates/app/ui/icon.png --out "$iconset/AppIcon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset/AppIcon.iconset" -o "$app/Contents/Resources/EzooDownload.icns"
rm -rf "$iconset"

# the browser extension, next to the app: no writes into the bundle later
for name in extension extension-firefox; do
    cp -R "$name" "$app/Contents/Resources/$name"
    rm -rf "$app/Contents/Resources/$name/test"
done
mv "$app/Contents/Resources/extension-firefox/manifest.firefox.json" \
   "$app/Contents/Resources/extension-firefox/manifest.json"
rm -f "$app/Contents/Resources/extension/manifest.firefox.json"
extver="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$app/Contents/Resources/extension/manifest.json" | head -1)"
if [[ -f "extension-signed/ezoo-download-firefox-$extver.xpi" ]]; then
    cp "extension-signed/ezoo-download-firefox-$extver.xpi" "$app/Contents/Resources/ezoo-download-firefox.xpi"
fi
cat > "$app/Contents/Resources/THIRD-PARTY-NOTICES.txt" <<'EOF'
Ezoo Download bundles no third-party media tools on macOS: it uses the
system's own tooling or the tools you install (ffmpeg, yt-dlp, aria2).
The browser extension and the Rust dependencies keep their own licences;
see the repository's THIRD-PARTY-NOTICES.txt and Cargo.lock.
EOF

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Ezoo Download</string>
  <key>CFBundleDisplayName</key><string>Ezoo Download</string>
  <key>CFBundleIdentifier</key><string>io.github.aelrased.ezoo-download</string>
  <key>CFBundleExecutable</key><string>EzooDownload</string>
  <key>CFBundleIconFile</key><string>EzooDownload</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
EOF

# ad-hoc signature: no Apple developer account, but the bundle is valid and
# the user only needs one right-click > Open the first time
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

mkdir -p dist
ditto -c -k --keepParent "$app" "dist/EzooDownload-macos-$arch.zip"
hdiutil create -volname "Ezoo Download" -srcfolder "$app" -ov -format UDZO \
    "dist/EzooDownload-macos-$arch.dmg" >/dev/null
(cd dist && shasum -a 256 "EzooDownload-macos-$arch.zip" > "EzooDownload-macos-$arch.zip.sha256")
(cd dist && shasum -a 256 "EzooDownload-macos-$arch.dmg" > "EzooDownload-macos-$arch.dmg.sha256")

echo
echo "   dist/EzooDownload-macos-$arch.dmg"
echo "   dist/EzooDownload-macos-$arch.zip"
