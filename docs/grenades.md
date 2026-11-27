# Grenades, feu et fumée

Code : `crates/game-sim/src/grenades.rs` (projectiles, molotov), `crates/game-sim/src/smoke.rs` (fumée), catalogue et prix : `configs/weapons.toml` (`flash`, `smoke`, `molotov`, `he`), maximum 4 grenades (`[equipment].max_grenades`). Décision sur la fumée : [ADR 0004](decisions/0004-hybrid-smoke.md).

Les constantes ci-dessous sont encore dans le code (`GrenadeConfig::default`, `SmokeConfig::default`, portées par `SimConfig`), pas dans `configs/` : les y déplacer est un travail à faire.

## Projectiles

| Paramètre | Valeur |
|---|---|
| Vitesse de lancer / gravité / rebond | 750 u/s, ×0,4, élasticité 0,45 |
| Délai flash / HE | 1,6 s |
| Flash | aveuglement jusqu'à 4,9 s, portée 2 000 u |
| HE | 98 dégâts max, rayon 350 u |
| Molotov | prend feu en touchant le sol ; encore en l'air après 2 s, il s'éteint sans effet. Feu de 7 s, rayon 150 u, 40 dégâts/s |

## Fumée hybride

Volume voxel opaque (voxels de 16 u) rempli par propagation depuis le point d'explosion : il épouse la pièce et ne traverse jamais un mur. Rayon 144 u, hauteur 112 u, durée 12 s (expansion 0,6 s, disparition 0,8 s).

- Calculée une fois par le serveur, envoyée aux clients (`SmokeNet`).
- Une HE perce un trou de 112 u en 0,6 s, qui se referme en 1,6 s ; les clients le recalculent à partir de l'événement répliqué, avec la même formule.
- Ligne de vue symétrique (même test des deux côtés) : pas de fumée à sens unique. Les balles ne modifient pas le volume.
