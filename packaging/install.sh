#!/bin/sh
# Installs Tendril for you alone: the program in ~/.local/bin, its menu entry and icon in
# ~/.local/share. Nothing needs root, and Tendril can update itself in place from there.
#   ./install.sh               install, or update by hand
#   ./install.sh --uninstall   take it all away again (your settings stay, in ~/.config/tendril)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bin="${XDG_BIN_HOME:-$HOME/.local/bin}"
share="${XDG_DATA_HOME:-$HOME/.local/share}"

if [ "${1:-}" = "--uninstall" ]; then
    pkill -x tendril 2>/dev/null || true
    rm -f "$bin/tendril" "$share/applications/tendril.desktop" "$share/icons/hicolor/scalable/apps/tendril.svg" \
        "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/tendril.desktop"
    echo "Removed Tendril. Your settings are still in ${XDG_CONFIG_HOME:-$HOME/.config}/tendril; delete that folder too if you like."
    exit 0
fi

install -Dm755 "$here/tendril" "$bin/tendril"
install -Dm644 "$here/tendril.desktop" "$share/applications/tendril.desktop"
install -Dm644 "$here/tendril.svg" "$share/icons/hicolor/scalable/apps/tendril.svg"
sed -i "s|^Exec=tendril|Exec=$bin/tendril|" "$share/applications/tendril.desktop"

echo "Installed Tendril to $bin/tendril."
case ":$PATH:" in *":$bin:"*) ;; *) echo "Add $bin to your PATH to run \`tendril\` from a terminal." ;; esac
echo "Start it now with: $bin/tendril"
