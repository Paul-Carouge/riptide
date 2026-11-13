# Riptide — direction artistique

Tous les tokens vivent dans `src/ui/theme.rs` ; les vues n'utilisent aucune valeur littérale.

## Intention
Une platine vinyle des années 70 posée sur du papier kraft clair : pochette crème avec du grain,
encre brun-noir, orange brûlé d'étiquette de disque, ambre de lampe, vert olive et bleu canard
pour les états. L'écran se lit comme un disque : **Face A** (le lien, l'aperçu, la sortie) à gauche,
**Face B** (la file et l'historique) à droite.

## Palette (sRGB)
| Token | Hex | Usage |
|---|---|---|
| `PAPER` | `#F3E9D2` | fond de fenêtre (avec grain) |
| `PAPER_DEEP` | `#EADBBB` | pistes de progression, pastilles neutres, survols |
| `CARD` / `CARD_HOVER` | `#FBF5E6` / `#FFFAEE` | cartes « pochette », boutons secondaires |
| `RULE` / `RULE_STRONG` | `#D8C49C` / `#BFA77A` | filets, bordures de contrôles |
| `INK` | `#2A1E16` | texte principal (13,4:1 sur papier) |
| `INK_MUTED` | `#6B5646` | texte secondaire (5,7:1) |
| `INK_FAINT` | `#7A6553` | placeholders (4,6:1) |
| `ACCENT` / `ACCENT_HOVER` | `#B4481A` / `#9E3D14` | action principale (texte crème 5,0:1) |
| `ACCENT_TEXT` | `#9E3D14` | accent utilisé comme texte (AA) |
| `ACCENT_SOFT` | `#F2D3B3` | état « Téléchargement » |
| `AMBER` | `#E39B2D` | fin du dégradé de progression, sélection de texte |
| `SUCCESS` / `SUCCESS_SOFT` | `#56622A` / `#E1E3C2` | olive : terminé, ffmpeg OK |
| `TEAL` / `TEAL_SOFT` | `#1F6F6B` / `#CFE2DC` | bleu canard : conversion, version yt-dlp, anneau de focus |
| `DANGER` / `DANGER_SOFT` | `#A23022` / `#F4D5C9` | erreurs, annuler |
| `VINYL`, `VINYL_GROOVE`, `VINYL_SHEEN`, `VINYL_LABEL` | `#1C1714`, `#342B26`, crème 22 %, `#C4541C` | disque |

Contrastes vérifiés (WCAG 2.2 AA) pour toutes les paires texte/fond utilisées.

## Palette sombre « vinyle de nuit »
La même pochette sous une lampe, la nuit : fond brun-noir chaud (jamais de noir pur ni de gris
froid), grain crème très discret, encre crème, même orange brûlé. Tokens : `theme::DARK` (même
struct `Palette` que `theme::LIGHT`). Les vues lisent `theme::palette()`, la palette de l'image
en cours, posée par la vue racine.

| Token | Hex | Contraste |
|---|---|---|
| `paper` | `#1D1611` | fond |
| `paper_deep` | `#34291F` | ink 11,5:1 · ink_muted 6,7:1 · accent_text 5,7:1 |
| `card` / `card_hover` | `#261D16` / `#30251C` | ink 13,4:1 |
| `rule` / `rule_strong` | `#3B2E24` / `#806A55` | bordure de contrôle 3,2:1 sur carte |
| `ink` | `#F2E6CD` | 14,4:1 sur papier |
| `ink_muted` | `#C4B094` | 8,5:1 sur papier, 7,9:1 sur carte |
| `ink_faint` | `#9C8872` | 5,3:1 sur papier (placeholder) |
| `accent` / `accent_hover` | `#B84A1B` / `#A3411A` | texte crème 4,9:1 / 5,9:1 |
| `accent_text` | `#EE8A52` | 7,1:1 sur papier, 6,6:1 sur carte, 5,3:1 sur `accent_soft` |
| `accent_soft` | `#4A2716` | pastille « Téléchargement » |
| `amber` | `#E9A640` | fin du dégradé |
| `success` / `success_soft` | `#AEBD72` / `#2F3419` | 6,3:1 |
| `teal` / `teal_soft` | `#76C6BC` / `#183633` | 6,6:1 ; anneau de focus 9,0:1 sur papier |
| `danger` / `danger_soft` | `#F2907A` / `#45201A` | 6,1:1 ; 7,1:1 sur carte |
| `vinyl`, `vinyl_groove`, `vinyl_rim` | `#110D0A`, `#2E2620`, crème 16 % | liseré éclairé qui détache le disque du fond |

Choix du thème : sélecteur **Auto / Clair / Sombre** dans l'en-tête, persisté (`theme_mode`).
**Auto** (défaut) suit l'apparence du bureau (`window.appearance()`, observée en direct). Le
passage d'une palette à l'autre est un fondu de 320 ms (`THEME_CROSSFADE_DURATION`, ease-out
cubique), calculé par l'animateur comme le reste.

## Typographie (embarquée, OFL, `assets/fonts/`)
- **Young Serif** : titres, wordmark, onglets (serif display « pochette 70s », chiffres elzéviriens).
- **IBM Plex Sans** 400/500/600 : texte courant, boutons.
- **IBM Plex Mono** 500 : surtitres en capitales, chiffres (vitesse, ETA, durée, formats), comme un compteur de magnéto.

Tailles : 12 · 13 · 15 · 18 · 24 · 34 px (`TEXT_XS` → `TEXT_DISPLAY`).

## Espacements, rayons, tailles
- Grille de 4 px : `SPACE_1..6` = 4, 8, 12, 16, 24, 32.
- Rayons : 6 (chips, cases), 10 (champ), 16 (cartes), pilule (boutons, pastilles).
- Contrôles : 36 px (44 px pour « Analyser » / « Télécharger »), boutons icône 32 px, case 18 px dans une ligne ≥ 32 px.
- Fenêtre 1100×760, minimum 800×600. Colonne gauche max 500 px. Sous 960 px de large, les
  libellés secondaires (« SIMULTANÉS ») disparaissent.
- Listes virtualisées (`uniform_list`) à hauteur fixe : piste 36 px, carte de file 124 px + 12 px.
  Les miniatures ne se chargent que pour les lignes affichées, 6 requêtes au plus en parallèle.

## Signatures visuelles
- **Grain du papier** : ~14 points / 10 000 px², déterministes (pas d'image), peints sous les cartes.
- **Disque vinyle** (`ui/vinyl.rs`) : sillons concentriques, reflet fixe (il ne tourne pas, comme sous une vraie lampe), étiquette orange avec la miniature, anneau imprimé + point et reflets de sillon qui tournent.
- **Progression « sillon »** : piste 6 px, remplissage dégradé orange → ambre ; olive une fois terminé, bleu canard pulsé pendant la conversion, brun en pause.
- **Pastilles d'état** en Mono : En attente, Téléchargement, Conversion, En pause, Terminé, Échec, Annulé.
  Une carte en échec affiche l'erreur à la place du sillon, avec « Réessayer » (même requête) ;
  après « Annuler », la carte montre « Annulation… » (désactivé) jusqu'à la confirmation du moteur.

## Mouvement
| Constante | Valeur | Rôle |
|---|---|---|
| `VINYL_PLAYING_RADIANS_PER_SECOND` | 33⅓ tr/min | vitesse de lecture |
| `VINYL_SPIN_UP_SECONDS` | 0,55 s | constante de temps du moteur au démarrage / reprise |
| `VINYL_SPIN_DOWN_SECONDS` | 0,9 s | freinage en pause / fin (décélération exponentielle) |
| `SPRING_FREQUENCY_HZ` / `SPRING_DAMPING_RATIO` | 2,1 Hz / 0,58 | ressort sous-amorti (léger dépassement ~10 %) |
| `CARD_ENTER_DURATION` / `CARD_ENTER_OFFSET` | 620 ms / 28 px | cartes de file qui glissent de la droite en ressort |
| `PREVIEW_UNFOLD_DURATION` / `PREVIEW_UNFOLD_OFFSET` | 560 ms / −14 px | aperçu qui se déplie |
| `PROGRESS_SMOOTHING_SECONDS` | 0,25 s | la barre suit la progression réelle sans à-coups |
| `ERROR_SHAKE_DURATION` / `_AMPLITUDE` | 380 ms / 6 px | secousse du message d'erreur du lien |
| `LOADING_PULSE_DURATION` | 1,4 s | pulsation « Préparation de yt-dlp… », conversion |

Règles : l'angle du disque est intégré analytiquement (indépendant du nombre d'images) ;
la vue ne demande une image suivante (`request_animation_frame`) que tant qu'un disque bouge ou
qu'une barre rattrape sa valeur : aucun redessin quand rien ne bouge. Le ressort est calculé dans
l'animateur (pas en `easing` : GPUI impose un easing dans 0..=1).

## Accessibilité
- Tout est atteignable au clavier : Tab / Maj+Tab, Entrée ou Espace sur le contrôle focalisé,
  Entrée dans le champ = Analyser, Échap = efface l'erreur, puis ferme l'aperçu, puis vide le champ.
- Focus visible : bordure bleu canard 2 px sur tous les contrôles.
- Les contrôles désactivés sortent de l'ordre de tabulation.

## Mode démo (builds debug uniquement)
`RIPTIDE_DEMO=1` (scène complète animée), `=empty` (états vides), `=history` (onglet historique),
`=big` (playlist de 1500 pistes, toutes en file) : données fictives, aucun appel au moteur, rien
n'est enregistré. `RIPTIDE_LINK=<url>` pré-remplit le champ et `RIPTIDE_AUTOSTART=1` analyse puis
met toutes les pistes en file (tests de bout en bout). Les builds release ignorent ces variables.
