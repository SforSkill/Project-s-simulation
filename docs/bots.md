# Bots

Code : `crates/game-sim/src/bots.rs` (`BotBrain`, `bot_buy`), navigation : `NavGraph` dans `crates/game-sim/src/level.rs`.

Les bots tournent sur le serveur et produisent des `UserCmd` ordinaires : mêmes règles que les humains, aucune aide à la visée, ils contre-straffent avant de tirer. Ils sont déterministes pour une graine donnée (`DetRng`).

## Configuration (`server.toml`)

| Clé | Défaut | Sens |
|---|---|---|
| `bots` | 0 | bots ajoutés au démarrage |
| `bot_difficulty` | 0.5 | 0 = facile, 1 = difficile |
| `fill_with_bots` | `false` | complète les deux équipes jusqu'à `team_size` |

La difficulté règle le temps de réaction (0,45 s à 0,15 s) et la vitesse de rotation de la vue (260 à 780 °/s).

## Comportement

- **Deathmatch, duel, entraînement** : va vers l'ennemi le plus proche, sinon vers un point d'apparition deathmatch au hasard.
- **Compétitif** : les attaquants vont vers un site choisi (escorte du porteur, ramassage d'une bombe lâchée) ; bombe posée, les défenseurs vont dessus et les attaquants se postent à côté.
- **Achats** (économie active) : sniper lourd pour un bot sur cinq s'il en a les moyens, sinon fusil, sinon SMG ; puis armure, kit (défense), fumigène, flash, HE, dans la limite de l'argent.
