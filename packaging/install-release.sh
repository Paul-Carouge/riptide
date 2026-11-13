#!/usr/bin/env bash
# Installs the latest Riptide release for the current user (no root, no Rust toolchain):
# downloads the binary, checks its SHA-256, then installs it like install.sh.
set -euo pipefail

readonly REPOSITORY="Paul-Carouge/riptide"
readonly ASSET_NAME="riptide-x86_64-linux"

if [ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]; then
    echo "Riptide est publié pour Linux x86_64 uniquement. Utilise ./packaging/install.sh pour compiler." >&2
    exit 1
fi

# /releases/latest redirects to /releases/tag/<tag>.
latest_release_page="$(curl -fsSLo /dev/null -w '%{url_effective}' "https://github.com/$REPOSITORY/releases/latest")"
tag="${latest_release_page##*/}"
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Aucune version publiée trouvée sur https://github.com/$REPOSITORY/releases." >&2
    exit 1
fi

work_directory="$(mktemp -d)"
trap 'rm -rf "$work_directory"' EXIT
cd "$work_directory"

download_base_url="https://github.com/$REPOSITORY/releases/download/$tag"
raw_base_url="https://raw.githubusercontent.com/$REPOSITORY/$tag/packaging"
curl -fsSLO "$download_base_url/$ASSET_NAME"
curl -fsSLO "$download_base_url/SHA256SUMS"
curl -fsSLO "$raw_base_url/riptide.desktop"
curl -fsSLO "$raw_base_url/riptide.svg"
sha256sum --check --strict SHA256SUMS

install -Dm755 "$ASSET_NAME" "$HOME/.local/bin/riptide"
icon_path="$HOME/.local/share/icons/hicolor/scalable/apps/riptide.svg"
install -Dm644 riptide.svg "$icon_path"
# KDE does not always resolve icon names from ~/.local/share/icons: point at the file directly.
mkdir -p "$HOME/.local/share/applications"
sed "s|^Icon=.*|Icon=$icon_path|" riptide.desktop > "$HOME/.local/share/applications/riptide.desktop"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
kbuildsycoca6 2>/dev/null || true

echo "Riptide ${tag#v} installé : ~/.local/bin/riptide (menu : Multimédia → Riptide)"
if ! command -v ffmpeg >/dev/null; then
    echo "Attention : ffmpeg est introuvable. Installe-le (par exemple « sudo pacman -S ffmpeg »)." >&2
fi
