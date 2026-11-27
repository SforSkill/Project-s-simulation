# Règles de match

Code : `crates/game-rules/src/lib.rs` (machine d'état pure, sans E/S ni connaissance de la carte). Profils : `configs/rules.toml`, choisis par `profile` dans `server.toml`, `--rules <profil>` ou le menu du client.

## Modes

| Profil | Mode | Format | Équipe | Économie | Bombe |
|---|---|---|---|---|---|
| `competitive` | compétitif | MR15 (30 manches, premier à 16), prolongations MR3 à 10 000 $ | 5 | oui | oui |
| `competitive_mr12` | compétitif | MR12 (24 manches) | 5 | oui | oui |
| `competitive_mr16` | compétitif | MR16 (32 manches) | 5 | oui | oui |
| `duel` | duel | 16 manches de 60 s, prolongations de 2 manches | 1 | non (fusil + pistolet + armure offerts) | non |
| `deathmatch` | deathmatch | 10 min, réapparition à 2 s, achat partout | 16 | non | non |
| `training` | entraînement | 1 h, réapparition à 0,5 s, achat partout | 16 | non | non |

Un champ absent d'un profil prend la valeur par défaut de `RulesConfig::default()`. `RulesConfig::validate` refuse un nombre de manches impair ou < 2, des prolongations impaires, `round_time <= 0`, `team_size` hors de 1..=32 et un `loss_bonus` vide.

## Phases

`Warmup` → `Freeze` (achat, immobile) → `Live` → `RoundOver` → (`HalfTime` à la mi-temps : changement de camp et remise à zéro de l'argent) → … → `GameOver`.

Fin de manche (`RoundEndReason`) : attaquants éliminés, défenseurs éliminés, bombe explosée, bombe désamorcée, temps écoulé. Victoire : premier à `max_rounds / 2 + 1`.

## Économie (valeurs par défaut)

| Clé | Valeur | Sens |
|---|---|---|
| `start_money` / `max_money` | 800 / 16 000 | argent de départ / plafond |
| `win_elimination`, `win_time` | 3 250 | victoire par élimination ou au temps |
| `win_bomb`, `win_defuse` | 3 500 | victoire par explosion ou désamorçage |
| `loss_bonus` | 1 400, 1 900, 2 400, 2 900, 3 400 | selon le nombre de défaites consécutives ; la dernière valeur se répète |
| `plant_team_bonus` | 800 | à chaque attaquant, en cas de défaite après pose |
| `plant_reward` / `defuse_reward` | 300 / 300 | au poseur / au désamorceur |
| `no_loss_bonus_for_time_survivors` | `true` | un attaquant encore en vie à la fin du temps ne touche pas le bonus de défaite |

Récompenses par élimination : `kill_reward` de chaque arme dans `configs/weapons.toml`. Prix de l'équipement : section `[equipment]` du même fichier.

## Bombe (valeurs par défaut)

Minuterie 40 s, pose 3,2 s, désamorçage 10 s (5 s avec kit), portée de désamorçage 72 u. Les zones de pose sont les `bomb_sites` de la carte (`docs/maps.md`).

## Divers

`friendly_fire` (désactivé), `reconnect_grace` (180 s : place réservée à un joueur déconnecté), `frag_limit` et `time_limit` (deathmatch), `free_armor`, `free_loadout`, `buy_anywhere`.
