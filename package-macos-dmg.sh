#!/bin/bash
# Package an already-built app without changing its executable or signature.
set -euo pipefail
cd "$(dirname "$0")"

[[ $# == 2 ]] || { echo "Usage: $0 APP_PATH OUTPUT.dmg" >&2; exit 1; }
[[ "$(uname -s)" == Darwin ]] || { echo "Build disk images on macOS." >&2; exit 1; }
command -v python3 >/dev/null || { echo "Install Python 3 to build the disk image." >&2; exit 1; }
python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 10) else "DMG packaging requires Python 3.10 or newer.")'
app="$1"
image="$2"
[[ -x "$app/Contents/MacOS/EzooDownload" ]] || { echo "App bundle is missing: $app" >&2; exit 1; }
[[ "$image" == *.dmg ]] || { echo "Output must end in .dmg" >&2; exit 1; }
codesign --verify --deep --strict "$app"

# Keep build-only dependencies isolated from both global Python and the app.
tool_dir="build-cache/dmgbuild-1.6.7"
if [[ ! -x "$tool_dir/bin/dmgbuild" ]]; then
    python3 -m venv "$tool_dir"
    "$tool_dir/bin/python" -m pip install --disable-pip-version-check -r packaging/dmg-requirements.txt
fi
mkdir -p "$(dirname "$image")"
"$tool_dir/bin/dmgbuild" -s packaging/dmg-settings.py -D "app=$app" "Ezoo Download" "$image"
hdiutil verify "$image"

# Verify the packaged copy, too: Finder attributes must not invalidate the
# existing signature, and packaging must never alter the tested app files.
validation_mount="$(mktemp -d "${TMPDIR:-/tmp}/ezoo-dmg-check.XXXXXX")"
mounted=false
cleanup() {
    if [[ "$mounted" == true ]]; then
        hdiutil detach "$validation_mount" >/dev/null || true
    fi
    rmdir "$validation_mount" 2>/dev/null || true
}
trap cleanup EXIT
hdiutil attach -readonly -nobrowse -mountpoint "$validation_mount" "$image" >/dev/null
mounted=true
codesign --verify --deep --strict "$validation_mount/$(basename "$app")"
diff -qr "$app" "$validation_mount/$(basename "$app")"
[[ "$(readlink "$validation_mount/Applications")" == /Applications ]]
hdiutil detach "$validation_mount" >/dev/null
mounted=false
(cd "$(dirname "$image")" && shasum -a 256 "$(basename "$image")" > "$(basename "$image").sha256")
echo "Disk image: $image"
