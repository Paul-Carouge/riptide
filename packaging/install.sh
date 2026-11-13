#!/usr/bin/env bash
# Builds Riptide in release mode and installs it for the current user (no root).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release
install -Dm755 target/release/riptide "$HOME/.local/bin/riptide"
install -Dm644 packaging/riptide.desktop "$HOME/.local/share/applications/riptide.desktop"
install -Dm644 packaging/riptide.svg "$HOME/.local/share/icons/hicolor/scalable/apps/riptide.svg"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
kbuildsycoca6 2>/dev/null || true
echo "Riptide installé : ~/.local/bin/riptide (menu : Multimédia → Riptide)"
