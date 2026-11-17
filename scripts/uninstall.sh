#!/bin/sh
# Remove Aurivon installed by install.sh. Library, settings and keys are kept
# unless --purge is given.
set -eu
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/aurivon"
rm -rf "$DATA/aurivon/app"
rm -f "$HOME/.local/bin/aurivon" "$DATA/applications/aurivon.desktop"
rm -f "$DATA"/icons/hicolor/*/apps/aurivon.png "$DATA/icons/hicolor/scalable/apps/aurivon.svg"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$DATA/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f "$DATA/icons/hicolor" 2>/dev/null || true
if [ "${1:-}" = "--purge" ]; then
    rm -rf "$CONFIG" "$DATA/aurivon" "${XDG_CACHE_HOME:-$HOME/.cache}/aurivon" "${XDG_STATE_HOME:-$HOME/.local/state}/aurivon"
    echo "Aurivon removed, including library, settings and keys"
else
    echo "Aurivon removed (library, settings and keys kept; --purge removes them)"
fi
