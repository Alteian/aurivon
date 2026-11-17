#!/bin/sh
# Build a release and package it as dist/rustmediahub-<version>-<arch>-linux.tar.gz
# (+ .sha256). Prints the package directory (--dir-only) or the tarball path last.
set -eu
REPO=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO"
VERSION=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
ARCH=$(uname -m)
NAME="rustmediahub-$VERSION"
[ "${RMH_SKIP_BUILD:-0}" = 1 ] || cargo build --release --bin rustmediahub >&2
BIN=target/release/rustmediahub
"$BIN" --version | grep -q "$VERSION" || { echo "release binary is not $VERSION; rebuild" >&2; exit 1; }
rm -rf "dist/$NAME"
mkdir -p "dist/$NAME/packaging/icons"
cp "$BIN" "dist/$NAME/rustmediahub"
cp packaging/icons/* "dist/$NAME/packaging/icons/"
cp packaging/rustmediahub.desktop.in packaging/launcher.sh.in "dist/$NAME/packaging/"
cp scripts/install.sh scripts/uninstall.sh "dist/$NAME/"
cp README.md LICENSE "dist/$NAME/" 2>/dev/null || true
[ -f CHANGELOG.md ] && cp CHANGELOG.md "dist/$NAME/"
echo "$VERSION" >"dist/$NAME/VERSION"
chmod 755 "dist/$NAME/rustmediahub" "dist/$NAME/install.sh" "dist/$NAME/uninstall.sh"
if [ "${1:-}" = "--dir-only" ]; then
    echo "$REPO/dist/$NAME"
    exit 0
fi
TAR="$NAME-$ARCH-linux.tar.gz"
tar -C dist --owner=0 --group=0 -czf "dist/$TAR" "$NAME"
(cd dist && sha256sum "$TAR" >"$TAR.sha256")
echo "$REPO/dist/$TAR"
