#!/bin/bash
# Installs the Linux build for this user: the binary in ~/.local/opt/glint, its icon, an app menu entry, and an
# autostart entry so the global shortcuts work from login. Run `bun exact.mjs linux` first.
set -euo pipefail
cd "$(dirname "$0")/.."
id=dev.mchisolm0.glint
opt=$HOME/.local/opt/glint
data=${XDG_DATA_HOME:-$HOME/.local/share}
mkdir -p "$opt" "$data/applications" "${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
# Copy then rename, so a running Glint keeps its old file.
cp target/host-dev/glint-linux "$opt/glint-linux.new" && mv "$opt/glint-linux.new" "$opt/glint-linux"
for size in 256 512; do
  mkdir -p "$data/icons/hicolor/${size}x${size}/apps"
  python3 -c "from PIL import Image; import sys; Image.open('assets/icon.png').resize(($size, $size), Image.LANCZOS).save(sys.argv[1])" \
    "$data/icons/hicolor/${size}x${size}/apps/$id.png"
done
entry="[Desktop Entry]
Type=Application
Name=Glint
Comment=Screenshots, recordings and scrolling captures
Exec=$opt/glint-linux
Icon=$id
Terminal=false
Categories=Graphics;
StartupNotify=false"
printf '%s\n' "$entry" > "$data/applications/$id.desktop"
printf '%s\nX-GNOME-Autostart-enabled=true\n' "$entry" > "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/$id.desktop"
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
echo "Installed. Quit Glint and open it from the app menu to use this build."
