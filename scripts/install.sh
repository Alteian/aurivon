#!/bin/sh
# Install Aurivon for the current user (no root needed).
#
#   From a checkout:        make install           (builds a release first)
#   From a release tarball: ./install.sh
#   Refresh launcher only:  ./install.sh --integrate
#
# Layout:
#   ~/.local/share/aurivon/app/versions/<version>/   binary + assets
#   ~/.local/share/aurivon/app/current -> versions/<version>
#   ~/.local/bin/aurivon                             launcher wrapper
#   ~/.local/share/applications/aurivon.desktop      app launcher entry
#   ~/.local/share/icons/hicolor/*/apps/aurivon.*    icons
#   ~/.config/aurivon/.env                           API keys (0600)
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/aurivon"
APP_ROOT="$DATA/aurivon/app"
BIN_DIR="$HOME/.local/bin"
LAUNCHER="$BIN_DIR/aurivon"
INTEGRATE_ONLY=0
[ "${1:-}" = "--integrate" ] && INTEGRATE_ONLY=1

# Running from a checkout: build + package, then install from the package
if [ ! -f "$HERE/VERSION" ] && [ -f "$HERE/../Cargo.toml" ]; then
    REPO=$(cd "$HERE/.." && pwd)
    PKG=$("$REPO/scripts/package.sh" --dir-only | tail -n 1)
    AURIVON_KEYS_FROM="${AURIVON_KEYS_FROM:-${RMH_KEYS_FROM:-$REPO/.env}}" exec "$PKG/install.sh" "$@"
fi

VERSION=$(cat "$HERE/VERSION")

if [ $INTEGRATE_ONLY -eq 0 ]; then
    echo "Installing Aurivon $VERSION"
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
    "$TARGET/aurivon" --version | grep -q "$VERSION" || { echo "installed binary failed its self-check" >&2; exit 1; }
    ln -sfn "versions/$VERSION" "$APP_ROOT/.current-$$"
    mv -T "$APP_ROOT/.current-$$" "$APP_ROOT/current"
fi

# Remove any leftover pre-rename install bits, then migrate config/library if needed
if [ $INTEGRATE_ONLY -eq 0 ]; then
    if [ -e "$DATA/rustmediahub/app" ] || [ -e "$HOME/.local/bin/rustmediahub" ] || [ -e "$DATA/applications/rustmediahub.desktop" ]; then
        rm -rf "$DATA/rustmediahub/app"
        rm -f "$HOME/.local/bin/rustmediahub" "$DATA/applications/rustmediahub.desktop"
        rm -f "$DATA"/icons/hicolor/*/apps/rustmediahub.png "$DATA/icons/hicolor/scalable/apps/rustmediahub.svg"
        echo "Removed leftover pre-rename install files"
    fi
    "$APP_ROOT/current/aurivon" --migrate | sed 's/^/  /'
fi

SRC="$APP_ROOT/current"
[ -d "$SRC/packaging" ] || SRC="$HERE"

# launcher wrapper
mkdir -p "$BIN_DIR"
sed "s|@APP_ROOT@|$APP_ROOT|g" "$SRC/packaging/launcher.sh.in" >"$LAUNCHER.tmp"
chmod 755 "$LAUNCHER.tmp"
mv -f "$LAUNCHER.tmp" "$LAUNCHER"

# icons
for f in "$SRC"/packaging/icons/aurivon-*.png; do
    size=$(basename "$f" .png); size=${size#aurivon-}
    mkdir -p "$DATA/icons/hicolor/${size}x${size}/apps"
    cp -f "$f" "$DATA/icons/hicolor/${size}x${size}/apps/aurivon.png"
done
mkdir -p "$DATA/icons/hicolor/scalable/apps"
cp -f "$SRC/packaging/icons/aurivon.svg" "$DATA/icons/hicolor/scalable/apps/aurivon.svg"
touch "$DATA/icons/hicolor"
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f "$DATA/icons/hicolor" 2>/dev/null || true

# launcher entry
mkdir -p "$DATA/applications"
sed "s|@LAUNCHER@|$LAUNCHER|g" "$SRC/packaging/aurivon.desktop.in" >"$DATA/applications/aurivon.desktop"
command -v desktop-file-validate >/dev/null 2>&1 && desktop-file-validate "$DATA/applications/aurivon.desktop" || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$DATA/applications" || true

# API keys for the installed app: copy only the keys it needs (values never printed)
KEYS_FROM="${AURIVON_KEYS_FROM:-${RMH_KEYS_FROM:-}}"
if [ $INTEGRATE_ONLY -eq 0 ] && [ ! -f "$CONFIG/.env" ] && [ -n "$KEYS_FROM" ] && [ -f "$KEYS_FROM" ]; then
    mkdir -p "$CONFIG"
    (
        umask 077
        {
            echo "# Aurivon API keys (copied by install.sh). Settings can override them."
            grep -E '^(TMDB_API_KEY|REAL_DEBRID_API_KEY|TORBOX_API_KEY|ALLDEBRID_API_KEY)=' "$KEYS_FROM" || true
        } >"$CONFIG/.env"
    )
    chmod 600 "$CONFIG/.env"
    echo "Copied $(grep -cE '^(TMDB_API_KEY|REAL_DEBRID_API_KEY|TORBOX_API_KEY|ALLDEBRID_API_KEY)=' "$CONFIG/.env") API key(s) to $CONFIG/.env (0600)"
fi
[ -f "$CONFIG/config.json" ] && chmod 600 "$CONFIG/config.json" || true

if [ $INTEGRATE_ONLY -eq 0 ]; then
    # keep the active version and the previous one
    ls -1t "$APP_ROOT/versions" | grep -v "^$VERSION\$" | tail -n +2 | while read -r old; do
        rm -rf "$APP_ROOT/versions/$old"
    done
    echo "Installed Aurivon $VERSION"
    echo "  launcher: $LAUNCHER"
    echo "  app menu: $DATA/applications/aurivon.desktop"
    case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "  note: $BIN_DIR is not on PATH" ;; esac
fi
