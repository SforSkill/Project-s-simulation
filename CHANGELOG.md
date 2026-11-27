# Changelog

Toutes les versions de `Project-s-simulation`. Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), versions [SemVer](https://semver.org/lang/fr/).

## [0.1.0] - 2026-11-27

Première version, extraite de `SforSkill/Projet-S` (historique conservé). Comportement de gameplay inchangé.

### Modifié
- Crates renommées : `engine-core` → `ps-sim-core`, `engine-physics` → `ps-physics`, `engine-movement` → `ps-movement`, `engine-weapons` → `ps-weapons`, `engine-hitreg` → `ps-hitreg`, `game-rules` → `ps-rules`, `game-sim` → `ps-sim`.
- `proto`, `predict` et `record` passent de `game-sim` à la nouvelle crate `ps-sim-net` : `ps-sim` ne dépend plus du transport ni du conteneur de replay.
- Identifiants, `UserCmd`, ticks et angles viennent de `ps-types` (réexportés sous leurs anciens chemins).
- Recherche des données : `PS_CONTENT_DIR` (l'ancien `PROJET_S_DATA` reste lu) et dépôt voisin `Project-s-content/`.

### Déplacé
- Tests des valeurs livrées (catalogue d'armes, profils de règles) vers Project-s-content.
