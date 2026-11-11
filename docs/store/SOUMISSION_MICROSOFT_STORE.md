# Dossier technique — Publication de HeicDrop sur le Microsoft Store

Ce document décrit, étape par étape, comment publier HeicDrop sur le Microsoft Store
sous forme de paquet **MSIX**, et rassemble toutes les informations techniques demandées
par l'Espace partenaires (Partner Center).

---

## 1. Fiche technique de l'application

| Élément | Valeur |
|---|---|
| Nom | **HeicDrop** |
| Type | Application de bureau Win32 empaquetée (MSIX, *full trust*) |
| Langage / UI | Rust, egui (rendu OpenGL via `glow`) |
| Architecture | x64 |
| Système minimal | Windows 10 version 1809 (build 17763) |
| Testé jusqu'à | Windows 11 24H2 (build 26100) |
| Langue de l'interface | Français |
| Taille du paquet | ≈ 10 Mo (exe + `heif.dll` + `libde265.dll`) |
| Connexion Internet | Aucune |
| Données collectées | Aucune |
| Capacité requise | `runFullTrust` (voir §5) |
| Intégration système | Entrée « Ouvrir avec → HeicDrop » pour .heic, .heif, .jpg, .png, .webp, .tif, .bmp, .gif |
| Licence | MIT (dépendances : voir `THIRD_PARTY_NOTICES.md`) |
| Catégorie Store | Photo et vidéo |
| Prix | Gratuit |
| Classification d'âge visée | Tout public (IARC 3+ / PEGI 3) |

### Contenu du paquet MSIX

```
AppxManifest.xml
resources.pri                  (généré par makepri)
Assets\                        icônes et tuiles (échelles 100 à 400 %, targetsize 16 à 256)
HeicDrop\heicdrop.exe          application
HeicDrop\heif.dll              libheif (LGPL-3.0, liaison dynamique)
HeicDrop\libde265.dll          décodeur HEVC (LGPL-3.0, liaison dynamique)
HeicDrop\LICENSE
HeicDrop\THIRD_PARTY_NOTICES.md
```

Fichiers sources correspondants dans le dépôt :

- Manifeste : [`packaging/msix/AppxManifest.xml`](../../packaging/msix/AppxManifest.xml)
- Visuels : [`packaging/msix/Assets/`](../../packaging/msix/Assets) (générés par `tools/generate_icons.py`)
- Script : [`packaging/build-msix.ps1`](../../packaging/build-msix.ps1)
- CI : [`.github/workflows/build.yml`](../../.github/workflows/build.yml)

---

## 2. Créer le compte et réserver le nom

1. Créer un compte développeur sur **https://partner.microsoft.com/dashboard** (programme
   « Applications et jeux »). Un compte **Particulier** suffit ; un compte **Entreprise**
   affiche le nom de la société comme éditeur (vérification plus longue). Les frais
   d'inscription dépendent du type de compte et du pays : vérifier sur la page d'inscription.
2. **Applications et jeux → Nouveau produit → Application MSIX ou PWA**.
3. Réserver le nom **HeicDrop** (s'il est déjà pris, réserver une variante, p. ex.
   « HeicDrop – Convertisseur HEIC », et modifier `DisplayName` dans le manifeste).

## 3. Récupérer l'identité du produit

Dans **Gestion du produit → Identité du produit**, relever :

| Champ Partner Center | Exemple | Où l'utiliser |
|---|---|---|
| `Package/Identity/Name` | `12345NomEditeur.HeicDrop` | variable `MSIX_IDENTITY_NAME` |
| `Package/Identity/Publisher` | `CN=1A2B3C4D-....` | variable `MSIX_PUBLISHER` |
| `Package/Properties/PublisherDisplayName` | `Nom Éditeur` | variable `MSIX_PUBLISHER_DISPLAY_NAME` |

Valeurs de HeicDrop (ID Store **9NZTB7VCGFGH**), déjà inscrites par défaut dans `packaging/build-msix.ps1` :

| Champ | Valeur |
|---|---|
| Name | `DanielLizotte.HeicDrop` |
| Publisher | `CN=EDE12BA8-4C62-4B6C-BCA0-C4A53527144E` |
| PublisherDisplayName | `Daniel Lizotte` |

Pour les remplacer sans modifier le script, créer les variables GitHub correspondantes
(**Settings → Secrets and variables → Actions → Variables**).

> ⚠️ Si ces valeurs ne correspondent pas exactement, le téléversement du paquet est refusé.

## 4. Construire le paquet

### Automatiquement (recommandé)

1. Mettre à jour `version` dans `Cargo.toml` (ex. `1.0.1`).
2. Envoyer le commit sur `main`.
3. Le workflow **Build** compile, exécute les tests puis, comme le tag `v1.0.1` n'existe pas encore,
   le crée et publie une *Release* GitHub contenant `HeicDrop_1.0.1.0_x64.msix` et l'archive
   portable `.zip`. (Pousser soi-même un tag `vX.Y.Z` fonctionne aussi.)

La version MSIX est `X.Y.Z.0` : le Store exige que le 4ᵉ nombre soit **0**, et chaque
nouvelle soumission doit avoir une version **supérieure** à la précédente.

### Manuellement (poste Windows)

Prérequis : Rust (MSVC), vcpkg avec `libheif[core,libde265]:x64-windows`, SDK Windows 10/11.

```powershell
cargo build --release
.\packaging\build-msix.ps1 -Version 1.0.0.0 -IdentityName "12345NomEditeur.HeicDrop" -Publisher "CN=..." -PublisherDisplayName "Nom Éditeur"
```

Le paquet est créé dans `dist\`. **Il n'a pas besoin d'être signé** pour le Store :
Microsoft le signe lors de la publication.

### Tester le paquet localement avant soumission

Un MSIX non signé ne s'installe pas par double-clic. Pour tester :

- **Méthode simple** (mode développeur Windows activé : *Paramètres → Système → Espace développeurs*) :
  ```powershell
  Add-AppxPackage -Register .\dist\msix-stage\AppxManifest.xml
  ```
- **Kit de certification** : lancer *Windows App Cert Kit* (installé avec le SDK), choisir
  « Valider un paquet d'application » et sélectionner le `.msix`. Corriger tout échec avant soumission.

Vérifier : lancement depuis le menu Démarrer, icônes nettes, glisser-déposer, conversion vers
chaque format, « Ouvrir avec → HeicDrop » sur un `.heic`, désinstallation propre.

## 5. Justification de la capacité `runFullTrust`

À coller dans **Soumission → Packages → Capacités** (ou dans les *Notes pour la certification*) :

> HeicDrop est une application de bureau Win32 classique (Rust) empaquetée en MSIX.
> La capacité runFullTrust est requise pour toute application Win32 empaquetée
> (EntryPoint Windows.FullTrustApplication). L'application l'utilise uniquement pour lire les
> images que l'utilisateur dépose ou sélectionne et pour écrire les fichiers convertis dans
> le dossier qu'il choisit via la boîte de dialogue standard de Windows. Elle n'effectue
> aucun accès réseau, ne collecte aucune donnée et ne modifie pas le système.

## 6. Remplir la soumission

| Section Partner Center | Contenu |
|---|---|
| **Tarification et disponibilité** | Gratuit ; tous les marchés (ou marchés francophones si l'interface reste uniquement en français) ; visibilité publique |
| **Propriétés** | Catégorie *Photo et vidéo* ; sous-catégorie aucune ; URL de politique de confidentialité (voir ci-dessous) ; site web et contact de support = page GitHub du projet / onglet Issues |
| **Propriétés → Configuration requise** | Clavier et souris recommandés ; aucune configuration matérielle particulière |
| **Classification d'âge** | Questionnaire IARC : aucune violence, aucun contenu sensible, pas de communication entre utilisateurs, pas d'achat, pas de partage de position → *Tout public* |
| **Packages** | Téléverser `HeicDrop_X.Y.Z.0_x64.msix` ; familles d'appareils : *Windows 10/11 Desktop* uniquement |
| **Fiches du Store** | Texte de [`FICHE_STORE.md`](FICHE_STORE.md) ; captures `docs/screenshots/*.png` ; logos `docs/store/images/` |
| **Notes pour la certification** | « Application hors ligne, aucun compte requis. Pour tester : glisser une image HEIC/JPEG dans la fenêtre, choisir un format, cliquer Convertir et choisir un dossier. » |

**URL de politique de confidentialité** : le Store l'exige pour les applications *full trust*.
Utiliser l'adresse publique du fichier [`docs/PRIVACY.md`](../PRIVACY.md) sur GitHub, par ex. :
`https://github.com/<compte>/heicdrop/blob/main/docs/PRIVACY.md`

## 7. Visuels fournis

| Usage | Fichier | Taille |
|---|---|---|
| Capture d'écran 1 (accueil) | `docs/screenshots/01-accueil.png` | 1366 × 768 |
| Capture d'écran 2 (lot → PDF) | `docs/screenshots/02-lot-pdf.png` | 1366 × 768 |
| Logo du Store (1:1) | `docs/store/images/logo-300x300.png` | 300 × 300 |
| Illustration 1:1 (box art) | `docs/store/images/box-art-1080x1080.png`, `box-art-2160x2160.png` | 1080², 2160² |
| Icône application (.ico) | `assets/icon.ico` | 16 → 256 px |
| Tuiles et icônes du paquet | `packaging/msix/Assets/*.png` | toutes échelles |

Le Store exige au moins **1 capture**, d'au moins **1366 × 768** pour les applications de bureau
(jusqu'à 10 ; 4 recommandées). Les captures fournies ont été prises sur l'application réelle ;
pour la version finale, il est conseillé de les refaire sous Windows 11 avec de vraies photos
(fenêtre de 1366 × 768 ou plus, sans données personnelles visibles).

## 8. Après la publication

- Délai de certification : généralement de quelques heures à 3 jours ouvrables.
- Ajouter le lien du Store dans le `README.md`.
- **Mises à jour** : incrémenter la version dans `Cargo.toml`, pousser un tag `vX.Y.Z`,
  puis créer une nouvelle soumission dans Partner Center avec le nouveau `.msix`.
  Les utilisateurs reçoivent la mise à jour automatiquement.

## 9. Points de vigilance

- **Licences** : libheif et libde265 sont sous LGPL-3.0. Elles sont livrées en DLL séparées
  (remplaçables), ce qui respecte la LGPL ; les mentions figurent dans `THIRD_PARTY_NOTICES.md`,
  inclus dans le paquet. L'encodeur x265 (GPL) n'est volontairement **pas** inclus.
- **Brevets HEVC** : HeicDrop décode le HEVC avec libde265 (logiciel libre). Le Store accepte
  de nombreuses applications utilisant ce décodeur ; aucune démarche n'est requise pour la
  soumission, mais la question des brevets HEVC relève de la responsabilité de l'éditeur.
- **Nom et marque** : « HEIC » est un format, pas une marque ; éviter les mentions « Apple » ou
  « iPhone » dans le nom de l'application (autorisé dans la description à titre descriptif).
- **Langue** : l'interface est en français. Si l'application est proposée hors des marchés
  francophones, ajouter une fiche anglaise (fournie dans `FICHE_STORE.md`) en précisant que
  l'interface est en français, ou traduire l'interface.
