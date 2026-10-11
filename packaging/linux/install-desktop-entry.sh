#!/bin/sh
# Adds Axe-Grid to the application menu of the current user. Nothing is
# installed system-wide. Run with --remove to undo it.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
entry="$data/applications/axe-grid.desktop"
icon="$data/icons/hicolor/scalable/apps/axe-grid.svg"

if [ "${1:-}" = "--remove" ]; then
    rm -f "$entry" "$icon"
    echo "Removed Axe-Grid from the application menu."
    exit 0
fi

mkdir -p "$(dirname "$entry")" "$(dirname "$icon")"
cp "$here/axe-grid.svg" "$icon"
# Point the launcher at wherever this folder is.
sed "s|^Exec=.*|Exec=\"$here/axegrid\"|" "$here/axe-grid.desktop" > "$entry"
chmod 644 "$entry"
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$data/applications" || true
fi
echo "Axe-Grid added to the application menu. If you move this folder, run this script again."
