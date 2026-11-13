#!/usr/bin/env bash
# Builds Riptide in release mode and installs it for the current user (no root).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release
install -Dm755 target/release/riptide "$HOME/.local/bin/riptide"
ICON_PATH="$HOME/.local/share/icons/hicolor/scalable/apps/riptide.svg"
install -Dm644 packaging/riptide.svg "$ICON_PATH"
for size in 48 128 256; do
  png="$HOME/.local/share/icons/hicolor/${size}x${size}/apps/riptide.png"
  mkdir -p "$(dirname "$png")"
  if command -v rsvg-convert >/dev/null; then
    rsvg-convert -w "$size" -h "$size" packaging/riptide.svg -o "$png"
  elif command -v magick >/dev/null; then
    magick -background none -density 384 packaging/riptide.svg -resize "${size}x${size}" "$png"
  fi
done
# KDE does not always resolve icon names from ~/.local/share/icons: point at the file directly.
sed "s|^Icon=.*|Icon=$ICON_PATH|" packaging/riptide.desktop > "$HOME/.local/share/applications/riptide.desktop"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
kbuildsycoca6 2>/dev/null || true
echo "Riptide installé : ~/.local/bin/riptide (menu : Multimédia → Riptide)"
