# Journal

## 2026-11-13 — Riptide v1 codée et installée
- **Fait** : moteur yt-dlp (install SHA-256, probe, 7 formats, file, pause/annulation, noms sans écrasement `Titre (2).ext`), UI GPUI vinyle clair/sombre (Auto/Clair/Sombre), cadre de fenêtre custom (CSD), réglages et historique, Réessayer, install utilisateur `packaging/install.sh`. PR #1.
- **Vérifié** : fmt/clippy/78 tests ; smoke réseau réel (MP3, MP4, Short, doublon, annulation) ; vrai parcours UI MP3 ; captures clair/sombre/800×600/CSD regardées.
- **Prochaine étape** : réduire le CPU des disques animés (~44 % pendant téléchargement), tester souris/clavier à la main.
- **Blocages** : aucun.
- **Hypothèses** : noms de fichiers propres + suffixe `(n)` ; fichiers partiels dans `<sortie>/.riptide-partial/` ; CSD par défaut.
- **Questions ouvertes** : aucune.

## 2026-11-13 — Kickoff
- **Fait** : grilling, création du dépôt, `CLAUDE.md`, `ROADMAP.md`, ADR 0001-0002.
- **Vérifié** : `cargo build` du squelette.
- **Prochaine étape** : jalon 1 — moteur yt-dlp + UI de base.
- **Blocages** : aucun.
- **Hypothèses** :
  - Moteur yt-dlp en sidecar géré par l'app (ADR 0002), ffmpeg système.
  - Périmètre v1 proposé au grilling (aperçu, formats, file parallèle, tags/pochette, historique).
  - Git : branches par tâche + PR, merge par le lead après `code-reviewer`.
  - Dossier de sortie par défaut `~/Musique/Riptide` (XDG music dir), concurrence par défaut 3.
  - Interface en français.
- **Questions ouvertes** : aucune.
