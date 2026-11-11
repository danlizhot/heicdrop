# Composants tiers

HeicDrop est distribué sous licence MIT. Il utilise les composants suivants.

## Bibliothèques natives (livrées sous forme de DLL à côté de `HeicDrop.exe`)

| Composant | Rôle | Licence | Source |
|---|---|---|---|
| libheif | Lecture des fichiers HEIF/HEIC | LGPL-3.0 (bibliothèque), MIT (exemples) | https://github.com/strukturag/libheif |
| libde265 | Décodage vidéo HEVC/H.265 (contenu des HEIC) | LGPL-3.0 | https://github.com/strukturag/libde265 |

Ces bibliothèques sont **liées dynamiquement** (`heif.dll`, `libde265.dll`) : conformément
à la LGPL, vous pouvez les remplacer par vos propres versions compilées. Leur code source
est disponible aux adresses ci-dessus ; les versions exactes utilisées sont celles du
port vcpkg `libheif[core,libde265]` au moment de la compilation.

Aucun encodeur HEVC (x265, sous licence GPL) n'est inclus : HeicDrop ne fait que **lire** le HEIC.

## Crates Rust (liées statiquement)

Principales dépendances, toutes sous licences permissives (MIT, Apache-2.0, BSD, Zlib, OFL pour les polices) :

- `eframe` / `egui` — interface graphique (MIT OU Apache-2.0)
- `image` — lecture/écriture JPEG, PNG, WEBP, TIFF, BMP, GIF… (MIT OU Apache-2.0)
- `libheif-rs`, `libheif-sys` — liaison vers libheif (MIT)
- `rfd` — boîtes de dialogue natives (MIT)
- `rayon` — traitement en parallèle (MIT OU Apache-2.0)
- Polices intégrées par egui : Ubuntu Light (Ubuntu Font Licence), Noto Emoji (OFL), emoji-icon-font (OFL / MIT)

La liste complète peut être générée avec `cargo install cargo-about && cargo about generate about.hbs`
ou consultée via `cargo tree`.
