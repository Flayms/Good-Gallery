#!/bin/sh
# Installs Good Gallery for the current user: the app in ~/.local/bin, its menu entry and its icon.
# `./install.sh --uninstall` removes them again. The data folder (~/.local/share/good-gallery) stays either way.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
bin=$HOME/.local/bin
entry=$data/applications/good-gallery.desktop
icon=$data/icons/hicolor/256x256/apps/good-gallery.png

if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$bin/good-gallery" "$entry" "$icon"
  echo "Good Gallery removed."
  exit 0
fi

install -Dm755 "$here/good-gallery" "$bin/good-gallery"
install -Dm644 "$here/good-gallery.png" "$icon"
mkdir -p "$(dirname "$entry")"
# The menu entry names the app by its full path, so it starts even where ~/.local/bin isn't on PATH.
sed "s|^Exec=.*|Exec=\"$bin/good-gallery\"|" "$here/good-gallery.desktop" > "$entry"
echo "Good Gallery installed: $bin/good-gallery"
for helper in exiftool ffmpeg; do
  command -v "$helper" > /dev/null || echo "Missing: $helper (see README.txt)"
done
