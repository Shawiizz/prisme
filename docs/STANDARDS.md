# Standards de Prisme

Ce que Prisme nomme, numérote et publie, et comment. Une règle d'ici l'emporte sur une
habitude ; une exception se discute, puis s'écrit ici.

## 1. Versions de l'application

- **Une seule source** : `version` dans le `Cargo.toml` de l'espace de travail. Tout en
  découle : les crates, l'installateur, l'APK (`--config {"version": …}`), l'écran À propos.
  Les `package.json` du frontend et du site sont privés : leur champ `version` ne compte pas.
- **SemVer, phase 0.x** : `0.MINEUR.CORRECTIF`.
  - `CORRECTIF` (0.4.**1**) : des corrections seulement, rien de nouveau à apprendre.
  - `MINEUR` (0.**5**.0) : une fonction nouvelle, un changement de comportement, une donnée
    écrite autrement (base, réglages, profils).
  - `1.0.0` : quand Prisme tient un set entier sur scène, sur les trois systèmes et Android,
    sans correction pressante en attente. À partir de là, `MAJEUR` pour ce qui casse.
- **Une version publiée ne change jamais.** Une erreur dans une version se corrige par la
  suivante, pas en remplaçant ses fichiers.

## 2. Tags et versions publiées (GitHub)

| Ce qui est publié | Tag | Titre | Statut |
| --- | --- | --- | --- |
| L'application | `X.Y.Z` | `Prisme X.Y.Z` | brouillon, puis publiée, « Latest » |
| Un modèle de séparation | `model-<id>-<n>` | `Separation model: <nom> (<n>)` | pré-version, jamais « Latest » |

- Pas de préfixe `v`. Plus de tag `mobile-…` (une version unique pour tous les systèmes).
- Les notes de version suivent toujours le même plan, en anglais : **New**, **Improved**,
  **Fixed**, **Install**. Une ligne dit ce que le DJ voit, pas comment c'est codé.
- **Avant de publier un brouillon**, ses fichiers sont essayés sur un vrai appareil, les tests
  ne tournant que sur ordinateur :
  - Windows : l'installateur par-dessus la version publiée, la bibliothèque relue, un mix court
    avec un contrôleur.
  - Android : l'APK par-dessus la version publiée ; un contrôleur branché, son profil changé,
    débranché puis rebranché, l'appli ouverte avec lui ; l'écran éteint pendant la lecture ; un
    fichier enregistré (rapport de diagnostic) et un fichier ouvert (son d'un pad).

## 3. Fichiers publiés

`Prisme-<version>-<système>-<architecture>[-<rôle>].<ext>`

| Système | Fichier pour le DJ | Fichier de mise à jour (signé) |
| --- | --- | --- |
| Windows | `Prisme-X.Y.Z-windows-x64.exe` (l'installateur Prisme) | `Prisme-X.Y.Z-windows-x64-update.exe` + `.sig` |
| macOS | `Prisme-X.Y.Z-macos-universal.dmg` | `Prisme-X.Y.Z-macos-universal-update.tar.gz` + `.sig` |
| Linux | `Prisme-X.Y.Z-linux-x64.AppImage`, `Prisme-X.Y.Z-linux-x64.deb` | l'AppImage + `.sig` |
| Android | `Prisme-X.Y.Z-android-arm64.apk` | le même APK |

- Architectures : `x64`, `arm64`, `universal`. Jamais `amd64` ni `x86_64` dans un nom publié.
- `latest.json` est le seul fichier sans version dans son nom.

## 4. Modèles de séparation

- Identifiant dans l'application : `<pistes>-<qualité>` (`four-fast`, `vocals-quality`…).
- Fichier : `<architecture>-<pistes>-<précision>-<n>.safetensors`
  (`bs-roformer-four-f16-1.safetensors`).
- `<n>` monte à chaque nouvel entraînement ou export ; un fichier publié ne change jamais.
- Chaque modèle publié porte sa licence à côté de lui, et son empreinte SHA-256 dans le code.

## 5. Données écrites par Prisme

| Donnée | Version | Règle |
| --- | --- | --- |
| Bibliothèque (SQLite) | `PRAGMA user_version` | une migration par changement, jamais modifiée une fois publiée |
| Réglages (`preferences.json`) | aucune | un champ nouveau a une valeur par défaut ; un champ renommé se migre une fois |
| Profils de contrôleurs | `format` + `revision` | `format` monte quand le lecteur change ; `revision` à chaque modification du fichier |
| Session, crash recovery | aucune | jetable : illisible = ignorée |

Prisme est maintenant installé chez d'autres DJ : une version publiée ne perd jamais les
données de la précédente.

## 6. Code et commits

- **Branches** : `main` seule sur GitHub. En local, une branche par chantier,
  `<sujet>` court en minuscules (`stems-switch`, `pads-overlap`), supprimée après fusion.
- **Commits** : `zone: ce qui change, dit simplement`, en anglais, sans signature ajoutée.
  Zones permises (une seule par commit ; deux zones = deux commits) :
  `engine`, `audio`, `vinyl`, `sync`, `loops`, `deck`, `pads`, `waveform`, `mixer`, `fx`,
  `stems`, `library`, `controllers`, `keyboard`, `settings`, `phone`, `android`, `ios`,
  `macos`, `linux`, `windows`, `updates`, `installer`, `i18n`, `docs`, `website`, `ci`,
  `release`, `tests`.
- **Avant chaque commit** : `cargo fmt`, `cargo clippy --workspace --all-targets -D warnings`,
  les tests touchés ; `ng build`, `ng test`, Prettier.
- **Code** : le fil audio n'alloue, ne verrouille, ne bloque ni ne panique jamais. Peu de
  commentaires, qui disent ce qu'est une chose ou pourquoi, en phrases.
- **Textes de l'interface** : en anglais dans le code, en français dans `fr.json`, espace
  insécable avant `:` `;` `?` `!`.

## 7. Vocabulaire

Un mot par chose, le même partout : interface, documentation, notes de version.

| Chose | Anglais | Français | À ne pas dire |
| --- | --- | --- | --- |
| Lecteur virtuel A–D | deck | deck | platine |
| Tourne-disque, sa platine simulée | turntable | platine | deck |
| Fichier audio de la bibliothèque | track | morceau | titre, son, piste |
| Voix, batterie, basse, mélodie séparées | stems | stems | pistes |
| Séparer un morceau en stems | separate | séparer | extraire, découper |
| Fine bande d'un stem sous l'aperçu | stem lane | bande | piste |
| Liste faite à la main | crate | crate | bac, playlist |
| Liste faite par des règles | smart crate | crate intelligente | playlist auto |
| Liste des morceaux à préparer | Prepare | Préparation | file, attente |
| Ce qui a été joué, par soirée | history, set | historique, set | |
| Repère numéroté | hot cue | hot cue | repère, cue point |
| Point de départ du deck | cue | cue | |
| Deck qui donne le tempo | leader | leader | maître, meneur |
| Façon dont la platine pilote le deck | INT / REL / ABS / THRU | INT / REL / ABS / THRU | |
| Dossier de musique ajouté | music folder | dossier | répertoire |
| Morceau hors de ces dossiers | elsewhere | ailleurs | orphelin |
