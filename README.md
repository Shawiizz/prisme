# Prisme

Logiciel DJ pensé pour le DVS (vinyles timecodés) : moteur audio temps réel en Rust,
interface Tauri v2 + Angular (à venir).

## Architecture

```
apps/
  prisme-cli/        outil en ligne de commande : toute la chaîne sans interface
crates/
  prisme-timecode/   décodeur de vinyle timecodé (vitesse, sens, position absolue)
  prisme-media/      décodage des fichiers audio en mémoire
  prisme-engine/     moteur temps réel : decks, contrôle vinyle, mixeur
  prisme-audio-io/   cartes son (WASAPI via cpal ; ASIO à venir) derrière une interface
```

Dépendances entre crates (aucun cycle, l'interface audio ne connaît pas le moteur) :

```
prisme-cli ──> prisme-engine ──> prisme-timecode
     │               └─────────> prisme-media
     └───────> prisme-audio-io
```

### Règles du temps réel

Le thread audio (`Engine::process`) n'alloue jamais de mémoire, ne prend aucun verrou et
ne libère rien :

- l'interface envoie des `Command` par une file sans verrou ;
- l'état (`EngineStatus`) est publié dans un triple buffer : la lecture donne toujours
  la dernière valeur ;
- ce que le moteur remplace (morceaux, décodeurs) revient au contrôleur par une file
  `Garbage` et y est libéré (`EngineController::collect_garbage`).

### Décodeur de timecode

Écrit de zéro (aucun code de xwax ni de Mixxx). Il mesure la vitesse aux passages à zéro
de la porteuse, apprend le rapport niveau / vitesse de la cellule, gère les sauts
d'aiguille, les poses d'aiguille et la marche arrière. Formats : Serato CV02 faces A et B,
Serato CD. Les performances sont figées par les tests de `crates/prisme-timecode/tests`.

## Utilisation

```
cargo test
cargo build --release
target\release\prisme devices
target\release\prisme play "morceau.mp3" --pitch 1.02
target\release\prisme simulate "morceau.mp3" --pattern scratch --gain 0.3
target\release\prisme vinyl "morceau.mp3" --input "nom de la carte" --mode absolute
```

`simulate` génère le signal d'une platine (lecture, scratch, coups de pitch) : pratique
pour tester sans matériel. `vinyl` lit une vraie platine sur l'entrée de la carte son.
