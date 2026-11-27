#!/usr/bin/env bash
# Ezoo Download — the .rpm, built in a Fedora container.
#
#   ./packaging/linux/build-rpm.sh
#
# rpmbuild is not on every distribution, so this uses podman (rootless is
# fine) and a Fedora image. It packages the same staged tree the .deb uses:
# run packaging/linux/build-linux.sh (or at least set up build-linux/deb)
# first, then this script copies the resulting .rpm into dist/.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
image="${RPM_IMAGE:-fedora:41}"
debdir="$root/build-linux/deb"
rtop="$root/build-linux/rpmtop"

command -v podman >/dev/null 2>&1 || { echo "podman is needed for the .rpm" >&2; exit 1; }
[ -d "$debdir/usr" ] || { echo "no staged tree at $debdir — run packaging/linux/build-linux.sh first" >&2; exit 1; }

# files the container wrote belong to a subordinate uid: remove them from
# inside the user namespace, where they are root's
podman unshare rm -rf "$rtop" 2>/dev/null || true
mkdir -p "$rtop/SOURCES" "$root/dist"
tar czf "$rtop/SOURCES/root.tar.gz" -C "$debdir" usr

echo "==> rpmbuild in $image (version $version)"
podman run --rm -v "$root:/src:Z" -w /src "$image" bash -lc "
    set -e
    dnf -y install rpm-build >/dev/null 2>&1
    rpmbuild -bb \
        --define '_topdir /src/build-linux/rpmtop' \
        --define '_sourcedir /src/build-linux/rpmtop/SOURCES' \
        --define 'ver $version' \
        /src/packaging/linux/rpm/ezoo-download.spec
    cp -f /src/build-linux/rpmtop/RPMS/*/*.rpm /src/dist/
"

# the copy is owned by the container's root; give it back to this user
podman unshare chown 0:0 "$root"/dist/*.rpm
podman unshare rm -rf "$rtop" 2>/dev/null || true

ls -lh "$root"/dist/*.rpm
