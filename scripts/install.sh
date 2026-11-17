#!/bin/sh
# Install RustMediaHub for the current user (no root needed).
#
#   From a checkout:        make install           (builds a release first)
#   From a release tarball: ./install.sh
#   Refresh launcher only:  ./install.sh --integrate
#
# Layout:
#   ~/.local/share/rustmediahub/app/versions/<version>/   binary + assets
#   ~/.local/share/rustmediahub/app/current -> versions/<version>
#   ~/.local/bin/rustmediahub                             launcher wrapper
#   ~/.local/share/applications/rustmediahub.desktop      app launcher entry
#   ~/.local/share/icons/hicolor/*/apps/rustmediahub.*    icons
#   ~/.config/rustmediahub/.env                           API keys (0600)
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/rustmediahub"
APP_ROOT="$DATA/rustmediahub/app"
BIN_DIR="$HOME/.local/bin"
LAUNCHER="$BIN_DIR/rustmediahub"
INTEGRATE_ONLY=0
[ "${1:-}" = "--integrate" ] && INTEGRATE_ONLY=1

# Running from a checkout: build + package, then install from the package
if [ ! -f "$HERE/VERSION" ] && [ -f "$HERE/../Cargo.toml" ]; then
    REPO=$(cd "$HERE/.." && pwd)
    PKG=$("$REPO/scripts/package.sh" --dir-only | tail -n 1)
    RMH_KEYS_FROM="${RMH_KEYS_FROM:-$REPO/.env}" exec "$PKG/install.sh" "$@"
fi

VERSION=$(cat "$HERE/VERSION")

if [ $INTEGRATE_ONLY -eq 0 ]; then
    echo "Installing RustMediaHub $VERSION"
    mkdir -p "$APP_ROOT/versions"
    TARGET="$APP_ROOT/versions/$VERSION"
    if [ "$HERE" != "$TARGET" ]; then
        STAGE="$APP_ROOT/versions/.install-$VERSION-$$"
        rm -rf "$STAGE"
        cp -a "$HERE" "$STAGE"
        rm -rf "$TARGET"
        mv "$STAGE" "$TARGET"
    fi
    # sanity check, then switch atomically
    "$TARGET/rustmediahub" --version | grep -q "$VERSION" || { echo "installed binary failed its self-check" >&2; exit 1; }
    ln -sfn "versions/$VERSION" "$APP_ROOT/.current-$$"
    mv -T "$APP_ROOT/.current-$$" "$APP_ROOT/current"
fi

SRC="$APP_ROOT/current"
[ -d "$SRC/packaging" ] || SRC="$HERE"

# launcher wrapper
mkdir -p "$BIN_DIR"
sed "s|@APP_ROOT@|$APP_ROOT|g" "$SRC/packaging/launcher.sh.in" >"$LAUNCHER.tmp"
chmod 755 "$LAUNCHER.tmp"
mv -f "$LAUNCHER.tmp" "$LAUNCHER"

# icons
for f in "$SRC"/packaging/icons/rustmediahub-*.png; do
    size=$(basename "$f" .png); size=${size#rustmediahub-}
    mkdir -p "$DATA/icons/hicolor/${size}x${size}/apps"
    cp -f "$f" "$DATA/icons/hicolor/${size}x${size}/apps/rustmediahub.png"
done
mkdir -p "$DATA/icons/hicolor/scalable/apps"
cp -f "$SRC/packaging/icons/rustmediahub.svg" "$DATA/icons/hicolor/scalable/apps/rustmediahub.svg"
touch "$DATA/icons/hicolor"
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f "$DATA/icons/hicolor" 2>/dev/null || true

# launcher entry
mkdir -p "$DATA/applications"
sed "s|@LAUNCHER@|$LAUNCHER|g" "$SRC/packaging/rustmediahub.desktop.in" >"$DATA/applications/rustmediahub.desktop"
command -v desktop-file-validate >/dev/null 2>&1 && desktop-file-validate "$DATA/applications/rustmediahub.desktop" || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$DATA/applications" || true

# API keys for the installed app: copy only the keys it needs (values never printed)
if [ $INTEGRATE_ONLY -eq 0 ] && [ ! -f "$CONFIG/.env" ] && [ -n "${RMH_KEYS_FROM:-}" ] && [ -f "$RMH_KEYS_FROM" ]; then
    mkdir -p "$CONFIG"
    (
        umask 077
        {
            echo "# RustMediaHub API keys (copied by install.sh). Settings can override them."
            grep -E '^(TMDB_API_KEY|REAL_DEBRID_API_KEY)=' "$RMH_KEYS_FROM" || true
        } >"$CONFIG/.env"
    )
    chmod 600 "$CONFIG/.env"
    echo "Copied $(grep -cE '^(TMDB_API_KEY|REAL_DEBRID_API_KEY)=' "$CONFIG/.env") API key(s) to $CONFIG/.env (0600)"
fi
[ -f "$CONFIG/config.json" ] && chmod 600 "$CONFIG/config.json" || true

if [ $INTEGRATE_ONLY -eq 0 ]; then
    # keep the active version and the previous one
    ls -1t "$APP_ROOT/versions" | grep -v "^$VERSION\$" | tail -n +2 | while read -r old; do
        rm -rf "$APP_ROOT/versions/$old"
    done
    echo "Installed RustMediaHub $VERSION"
    echo "  launcher: $LAUNCHER"
    echo "  app menu: $DATA/applications/rustmediahub.desktop"
    case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "  note: $BIN_DIR is not on PATH" ;; esac
fi
