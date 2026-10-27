#!/usr/bin/env bash
# Ezoo Download — browser extension packaging (Linux workflow / by hand)
#
# Usage:  ./packaging/linux/build-extensions.sh [out-dir]
# Result: <out-dir>/ezoo-download-chrome-<version>.zip   (unsigned, always)
#         <out-dir>/ezoo-download-firefox-<version>.xpi  (unsigned zip; Mozilla
#                 signing happens in CI on tag releases, see linux-release.yml)
#         <out-dir>/ezoo-download-chrome-<version>.crx   (only when the Chrome
#                 private key is supplied — see below)
#
# The .crx keeps the extension's fixed identity
# (bilpkcfhiogfelpdfpgmnjkpblgmcbkd — the original Chrome Web Store
# listing's id): it MUST be signed with the same RSA private key every
# time, in all releases. The key is never committed anywhere.
# Supply it as the CRX_PRIVATE_KEY environment variable (the PEM text):
#   CRX_PRIVATE_KEY="$(cat key.pem)" ./packaging/linux/build-extensions.sh dist
# In CI it comes from the CRX_PRIVATE_KEY repository secret. When the key
# is absent the script still produces the .zip (load it unpacked in
# chrome://extensions with Developer mode) and says so clearly.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
EXT_VERSION="$(python3 -c "import json;print(json.load(open('$ROOT/extension/manifest.json'))['version'])")"
OUT="${1:-$ROOT/dist}"
mkdir -p "$OUT"
# Absolute from here on: zip_dir() below cds into the staging dir, so a
# relative OUT would make zip write into a non-existent path (CI failure).
OUT="$(cd "$OUT" && pwd)"

command -v zip >/dev/null 2>&1 || { echo "build-extensions.sh: zip not found" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

# ------------------------------------------------------------------ staging
# Chrome: the plain manifest; Firefox: manifest.firefox.json as manifest.json.
# Tests never ship.
mkdir -p "$STAGE/chrome" "$STAGE/firefox"
cp -r "$ROOT/extension/." "$STAGE/chrome/"
cp -r "$ROOT/extension/." "$STAGE/firefox/"
rm -rf "$STAGE/chrome/test" "$STAGE/firefox/test" \
       "$STAGE/chrome/manifest.firefox.json"
mv "$STAGE/firefox/manifest.firefox.json" "$STAGE/firefox/manifest.json"

zip_dir() { # $1 = dir, $2 = out file
    (cd "$1" && zip -r -X -q "$2" .)
}

echo "==> Chrome .zip (unsigned)…"
zip_dir "$STAGE/chrome" "$OUT/ezoo-download-chrome-$EXT_VERSION.zip"
echo "    $OUT/ezoo-download-chrome-$EXT_VERSION.zip"

echo "==> Firefox .xpi (unsigned zip)…"
zip_dir "$STAGE/firefox" "$OUT/ezoo-download-firefox-$EXT_VERSION.xpi"
echo "    $OUT/ezoo-download-firefox-$EXT_VERSION.xpi"

# ----------------------------------------------------------------------- crx
if [ -n "${CRX_PRIVATE_KEY:-}" ]; then
    echo "==> Chrome .crx (signed with the supplied private key)…"
    KEY="$STAGE/crx-key.pem"
    printf '%s' "$CRX_PRIVATE_KEY" > "$KEY"
    chmod 600 "$KEY"
    # crx3 (CRXv3, Chrome 64+): an existing key file is reused, so the
    # extension keeps its fixed id. Pinned version for reproducibility.
    npx --yes crx3@1.1.2 -o "$OUT/ezoo-download-chrome-$EXT_VERSION.crx" -p "$KEY" "$STAGE/chrome"
    rm -f "$KEY"
    # the CRX magic: a broken pack must not ship silently
    head -c 4 "$OUT/ezoo-download-chrome-$EXT_VERSION.crx" | grep -q "Cr24" \
        || { echo "build-extensions.sh: the .crx does not start with the Cr24 magic" >&2; exit 1; }
    echo "    $OUT/ezoo-download-chrome-$EXT_VERSION.crx"
else
    echo "==> No CRX_PRIVATE_KEY: no .crx produced. Set the secret (the PEM that owns id bilpkcfhiogfelpdfpgmnjkpblgmcbkd) to get a signed .crx; the .zip above loads unpacked." >&2
fi
echo "==> Done. (app version $VERSION, extension version $EXT_VERSION)"
