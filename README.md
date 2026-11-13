# Riptide

Application de bureau Linux pour télécharger l'audio ou la vidéo de liens YouTube : vidéos, Shorts et playlists entières. Riptide est écrit en Rust avec [GPUI](https://www.gpui.rs/) et a une apparence de vinyle rétro, en thème clair ou sombre.

- Collez un lien, choisissez les pistes d'une playlist, puis le format : MP3, M4A, OPUS, FLAC ou WAV pour l'audio, MP4 ou WEBM pour la vidéo, avec la qualité voulue.
- File de téléchargements en parallèle, avec progression, vitesse, temps restant, pause, reprise et annulation (sans fichiers partiels laissés derrière).
- Titre, artiste et pochette intégrés aux fichiers.
- Historique et réglages conservés entre deux lancements.
- Riptide télécharge lui-même [yt-dlp](https://github.com/yt-dlp/yt-dlp) au premier lancement (empreinte SHA-256 vérifiée) et peut le mettre à jour.

## Prérequis

- Linux x86_64 (Wayland ou X11).
- `ffmpeg` et `ffprobe`, par exemple `sudo pacman -S ffmpeg` ou `sudo apt install ffmpeg`.
- Facultatif : `deno`, `node` ou `bun`. yt-dlp en a besoin pour voir tous les formats de YouTube.

## Installation

### Depuis la dernière version publiée

```sh
curl -fsSLO https://raw.githubusercontent.com/Paul-Carouge/riptide/main/packaging/install-release.sh
bash install-release.sh
```

Le script télécharge `riptide-x86_64-linux` depuis les [releases](https://github.com/Paul-Carouge/riptide/releases), vérifie son empreinte avec `SHA256SUMS`, puis l'installe dans `~/.local/bin/riptide`, avec l'icône et l'entrée de menu. Il ne demande pas les droits root.

### Depuis les sources

Il faut Rust 1.98 ou plus récent ([rustup](https://rustup.rs/)).

```sh
git clone https://github.com/Paul-Carouge/riptide.git
cd riptide
./packaging/install.sh
```

## Mise à jour

Riptide vérifie au démarrage si une nouvelle version existe (un point orange apparaît alors sur le bouton Paramètres). Dans Paramètres › Mises à jour, « Rechercher une mise à jour » relance la vérification et « Installer et redémarrer » installe la nouvelle version. Il vérifie l'empreinte SHA-256 de la nouvelle version, remplace son exécutable, puis redémarre. Cela fonctionne quand Riptide est installé dans un dossier où il peut écrire, comme `~/.local/bin`.

## Développement

```sh
cargo run                      # lancer l'application
cargo test                     # tests
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test   # vérification complète
```

Sur Debian ou Ubuntu, la compilation demande aussi `pkg-config libfontconfig-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev`.

Une version est publiée en poussant un tag `vX.Y.Z` identique à la version de `Cargo.toml`. Le workflow `release.yml` compile alors le binaire et crée la release GitHub avec `SHA256SUMS`.

## Usage responsable

Riptide est destiné à un usage personnel. Ne téléchargez que des contenus dont vous avez le droit de faire une copie, et respectez le droit d'auteur ainsi que les [conditions d'utilisation de YouTube](https://www.youtube.com/t/terms). Vous êtes seul responsable de ce que vous téléchargez.

## Licences

- Code : [MIT](LICENSE).
- Polices IBM Plex et Young Serif : SIL Open Font License 1.1 (`assets/fonts/OFL-*.txt`).
- `vendor/xattr` : MIT ou Apache 2.0 (`vendor/xattr/LICENSE-*`).
- yt-dlp n'est pas inclus : Riptide le télécharge depuis son dépôt officiel (licence Unlicense).
