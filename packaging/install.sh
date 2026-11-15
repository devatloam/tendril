#!/bin/sh
# Installs Tendril for you alone: the program in ~/.local/bin, its menu entry and icon in
# ~/.local/share. Nothing needs root, and Tendril can update itself in place from there.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bin="${XDG_BIN_HOME:-$HOME/.local/bin}"
share="${XDG_DATA_HOME:-$HOME/.local/share}"

install -Dm755 "$here/tendril" "$bin/tendril"
install -Dm644 "$here/tendril.desktop" "$share/applications/tendril.desktop"
install -Dm644 "$here/tendril.svg" "$share/icons/hicolor/scalable/apps/tendril.svg"
sed -i "s|^Exec=tendril|Exec=$bin/tendril|" "$share/applications/tendril.desktop"

echo "Installed Tendril to $bin/tendril."
case ":$PATH:" in *":$bin:"*) ;; *) echo "Add $bin to your PATH to run \`tendril\` from a terminal." ;; esac
echo "Start it now with: $bin/tendril"
