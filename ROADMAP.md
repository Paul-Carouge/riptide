# Roadmap
Statuts : `à faire` · `en cours` · `fait` (codé) · `vérifié` (testé + rendu regardé)

## Jalon 1 — Riptide v1 (en cours)
- [ ] Moteur : gestion de yt-dlp (téléchargement au 1er lancement, SHA-256, mise à jour) + détection ffmpeg — `fait` — critère : binaire installé, hash vérifié, erreur claire si ffmpeg absent.
- [ ] Moteur : analyse d'un lien (vidéo, Short, playlist) via `yt-dlp -J --flat-playlist` — `fait` — critère : tests de parsing sur JSON fixtures ; titre, durée, miniature, liste des pistes.
- [ ] Moteur : téléchargement avec préréglages MP3/M4A/OPUS/FLAC/WAV et MP4/WEBM + qualité, tags et pochette — `fait` — critère : fichier lu par ffprobe avec le bon codec, tags et pochette présents.
- [ ] Moteur : file parallèle (concurrence réglable), progression/vitesse/ETA, pause/reprise, annulation avec nettoyage — `fait` — critère : tests du parseur de progression ; annulation sans fichier partiel.
- [ ] UI : identité vinyle rétro (tokens, grain, typographie) et écran principal (barre de lien, aperçu, pistes cochables, choix format/qualité) — `fait` — critère : capture regardée.
- [ ] UI : file animée (cartes qui glissent, disque qui tourne, progression), états vide/chargement/erreur — `fait` — critère : capture + animation à 60 fps sans saccade visible.
- [ ] Réglages et historique persistés (dossier de sortie, concurrence, format par défaut) — `fait` — critère : relance de l'app conserve réglages et historique.

## Jalon 2 — Confort
- [ ] Notifications de bureau à la fin d'une file
- [ ] Glisser-déposer d'un lien, collage automatique depuis le presse-papiers
- [ ] Paquet AUR / AppImage local

## Idées
- Limiter l'animation des disques à 30 fps pour baisser le CPU (source : session 2026-11-13)
- Infobulle sur l'erreur tronquée d'une carte en échec ; bouton « Dossier » plus court en 800 px (source : session 2026-11-13)
- Découpage par chapitres en fichiers séparés (source : kickoff 2026-11-13)
- Recherche YouTube intégrée (source : kickoff 2026-11-13)
