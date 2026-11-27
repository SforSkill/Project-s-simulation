# Périmètre de Project-s-simulation

## 1. Mission
Fournir l'unique implémentation des mécaniques de Project S : la même bibliothèque fait la simulation autoritaire du serveur, celle du serveur local embarqué du client et la prédiction client. Il n'existe pas de seconde implémentation du déplacement, du recul ou des dégâts.

## 2. Responsabilités
| Crate | Contenu |
|---|---|
| `ps-sim-core` | Maths (`Aabb`, décalages angulaires), `DetRng`, conversion de sensibilité, chargement TOML/RON et recherche du dossier de contenu ; réexporte `ps-types` |
| `ps-physics` | Brushes convexes, `CollisionWorld`, traces, format de carte `MapDef`, matériaux physiques |
| `ps-movement` | Contrôleur de déplacement type Source : accélération, friction, gravité, air strafe, bunny hop, postures, counter-strafe, buffer d'axes |
| `ps-weapons` | Catalogue, machine d'état des armes, cadence, munitions, recul déterministe, dispersion en mouvement, dégâts, armure, tagging, pénétration |
| `ps-hitreg` | Capsules de hitbox, historique de poses, compensation de latence |
| `ps-rules` | Machine d'état du match : formats (MR15 officiel, MR12/MR16 configurables), phases, économie, bombe |
| `ps-sim` | `Sim::step` (un tick) : joueurs, tirs, grenades (flash, molotov, smoke, HE), fumées, bombe, achats, bots, navigation |
| `ps-sim-net` | Messages de jeu client ↔ serveur (`proto`), prédiction client (`predict`), enregistrement et relecture de replays (`record`) |

Règles spécifiques à Project S (armes, modes, économie) : `ps-weapons::defs`, `ps-rules`, `ps-sim`. Primitives génériques (collisions, déplacement) : `ps-physics`, `ps-movement`.

## 3. Ne contient jamais
Rendu, audio, fenêtre, E/S réseau, HTTP, base de données, contrat back-end, logique de compte ou de sanction, valeur de gameplay en dur (elles sont dans Project-s-content, sauf exception connue ci-dessous).

## 4. Dépendances autorisées
`glam`, `serde`, `toml`, `ron`, `postcard`, `thiserror` ; `ps-types` et `ps-replay` (Project-s-protocol) ; `ps-transport` (Project-s-netcode) pour le codec `wire` de `ps-sim-net` uniquement.

## 5. Dépendances interdites
Bevy et toute pile graphique ou audio, runtimes async, clients HTTP/SQL, Project-s-server, Project-s-engine, le client, `ps-core-protocol`. Contrôlé par `boundaries.toml` en CI.

## 6. Interfaces publiques
`Sim`, `SimConfig`, `GameData`, `Level`, `GameEvent`, `Controller`, `simulate_cmd`/`simulate_segment`, `Catalog`, `RulesConfig`, `hitboxes`, `proto::{ClientMsg, ServerMsg}`, `predict`, `record`.

## 7. Contrats
- Données : schémas des fichiers de Project-s-content (`configs/*.toml`, `maps/*.ron`). `GameData::fingerprint()` doit être identique côté client et serveur.
- Messages de jeu : `ps-sim-net::proto`, version `ps_types::PROTOCOL_VERSION`.
- Replays : trames `ps-sim-net::record` dans le conteneur `ps-replay`.

## 8. Performance
`Sim::step` tourne à 128 Hz sur le serveur : aucune allocation évitable par tick, aucune E/S, aucun verrou. La prédiction suit exactement le chemin de `Sim::run_cmd`. Benchmarks criterion : `ps-physics`, `ps-movement`, `ps-hitreg`, `ps-sim`.

## 9. Sécurité
Le serveur est autoritaire : la simulation valide chaque commande (cadence, munitions, positions) et ne fait jamais confiance au client.

## 10. Compilation
`cargo build --workspace`. Les tests et benchmarks lisent le contenu de Project-s-content (`PS_CONTENT_DIR` ou dépôt voisin, version épinglée dans `.content-version`).

## 11. Tests
`cargo test --workspace --release` : counter-strafe, déplacement, armes, hitreg, règles, grenades, fumées, bots, déterminisme des replays. Les tests des valeurs livrées (catalogue d'armes, profils de règles) sont dans Project-s-content.

## 12. Versionnement
SemVer, tag `vX.Y.Z`. Une modification du comportement de gameplay est au minimum une version mineure, avec une mention explicite au changelog.

## 13. Déploiement
Bibliothèque consommée par le serveur, le client et le Lab via tag Git épinglé.

## 14. Limites connues
- Grenades et fumée gardent quelques valeurs dans `GrenadeConfig`/`SmokeConfig` (voir `docs/grenades.md`).
- Déterminisme vérifié sur une même plateforme (replays) ; l'identité bit à bit des flottants entre Windows et Linux n'est pas garantie et reste à mesurer (Project-s-lab).
- Validé et à préserver : déplacement inspiré de CS:GO sans sprint, counter-strafe à arrêt franc avec buffer, précision parfaite à l'arrêt hors recul, recul déterministe, aléa uniquement en mouvement, pas d'aim punch, tagging seulement sans kevlar, snipers type Valorant, wallbang type CS:GO, fumées courtes (12 s à valider) dispersées par la HE et non par les tirs.
