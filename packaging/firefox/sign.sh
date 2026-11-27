#!/usr/bin/env bash
# Sign the Firefox add-on with Mozilla (AMO). A signed .xpi is the only way
# Firefox release builds keep the add-on after a restart.
#
#   export WEB_EXT_API_KEY=...        # or AMO_JWT_ISSUER
#   export WEB_EXT_API_SECRET=...     # or AMO_JWT_SECRET
#   ./packaging/firefox/sign.sh
#
# Keys: https://addons.mozilla.org/developers/addon/api/key/
# The signed file lands in extension-signed/ezoo-download-firefox-<version>.xpi
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
version="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$root/extension/manifest.firefox.json" | head -1)"
[ -n "$version" ] || { echo "no version in extension/manifest.firefox.json" >&2; exit 1; }

key="${WEB_EXT_API_KEY:-${AMO_JWT_ISSUER:-}}"
secret="${WEB_EXT_API_SECRET:-${AMO_JWT_SECRET:-}}"
if [ -z "$key" ] || [ -z "$secret" ]; then
    cat >&2 <<'EOF'
AMO API keys are missing.

1. Open https://addons.mozilla.org/developers/addon/api/key/
2. Create (or copy) the JWT issuer and secret.
3. Run:
     export WEB_EXT_API_KEY='user:12345678:123'
     export WEB_EXT_API_SECRET='...'
     ./packaging/firefox/sign.sh

Keep the secret private: do not paste it into a chat or commit it.
EOF
    exit 1
fi

stage="$root/build-firefox-$version"
rm -rf "$stage"
mkdir -p "$stage"
for f in background.js content.js early.js yt-core.js popup.html popup.js options.html options.js; do
    cp "$root/extension/$f" "$stage/"
done
cp -R "$root/extension/icons" "$stage/icons"
cp "$root/extension/manifest.firefox.json" "$stage/manifest.json"

# A signed build of the fork: its own id, no update_url (Mozilla-hosted
# add-ons may not carry one).
python3 - "$stage/manifest.json" <<'PY'
import json, sys
p = sys.argv[1]
m = json.load(open(p, encoding="utf-8"))
g = m.setdefault("browser_specific_settings", {}).setdefault("gecko", {})
g["id"] = "ezoo-download-fork@users.noreply.github.com"
g.pop("update_url", None)
json.dump(m, open(p, "w", encoding="utf-8"), indent=2, ensure_ascii=False)
open(p, "a", encoding="utf-8").write("\n")
PY

runner=(web-ext)
command -v web-ext >/dev/null 2>&1 || runner=(npx --yes web-ext)

out="$root/dist/firefox-signed"
mkdir -p "$out"
rm -f "$out"/*.xpi
"${runner[@]}" sign \
    --source-dir "$stage" \
    --artifacts-dir "$out" \
    --channel unlisted \
    --api-key "$key" \
    --api-secret "$secret"

signed="$(ls -t "$out"/*.xpi 2>/dev/null | head -1)"
[ -n "$signed" ] || { echo "web-ext made no .xpi in $out" >&2; exit 1; }
mkdir -p "$root/extension-signed"
target="$root/extension-signed/ezoo-download-firefox-$version.xpi"
cp -f "$signed" "$target"
echo
echo "Signed: $target"
echo "Now: ./packaging/linux/build-linux.sh   (it picks the signed file of this version)"
