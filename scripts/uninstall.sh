#!/bin/sh
# Remove RustMediaHub installed by install.sh. Library, settings and keys are kept
# unless --purge is given.
set -eu
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/rustmediahub"
rm -rf "$DATA/rustmediahub/app"
rm -f "$HOME/.local/bin/rustmediahub" "$DATA/applications/rustmediahub.desktop"
rm -f "$DATA"/icons/hicolor/*/apps/rustmediahub.png "$DATA/icons/hicolor/scalable/apps/rustmediahub.svg"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$DATA/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f "$DATA/icons/hicolor" 2>/dev/null || true
if [ "${1:-}" = "--purge" ]; then
    rm -rf "$CONFIG" "$DATA/rustmediahub" "${XDG_CACHE_HOME:-$HOME/.cache}/rustmediahub" "${XDG_STATE_HOME:-$HOME/.local/state}/rustmediahub"
    echo "RustMediaHub removed, including library, settings and keys"
else
    echo "RustMediaHub removed (library, settings and keys kept; --purge removes them)"
fi
