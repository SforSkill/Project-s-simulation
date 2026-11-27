# Project-s-simulation

Simulation partagée de [Project S](https://github.com/SforSkill/Project-s-docs) : déplacement, collisions, armes, hitboxes, règles de match, grenades, fumées, bots, messages de jeu, prédiction et replays. Le serveur dédié, le serveur local du client et la prédiction exécutent le même code.

| Crate | Rôle |
|---|---|
| `ps-sim-core` | Maths, RNG déterministe, sensibilité, chargement des données |
| `ps-physics` | Collisions, traces, format de carte |
| `ps-movement` | Déplacement et counter-strafe |
| `ps-weapons` | Armes, recul, dispersion, dégâts, pénétration |
| `ps-hitreg` | Hitboxes et compensation de latence |
| `ps-rules` | Formats, phases, économie, bombe |
| `ps-sim` | `Sim::step`, grenades, fumées, bots |
| `ps-sim-net` | Messages de jeu, prédiction, replays |

```sh
git clone https://github.com/SforSkill/Project-s-content ../Project-s-content   # données des tests
cargo test --workspace --release
```

Documentation : [`docs/`](docs/) (déplacement, armes, grenades, règles, bots). Périmètre : [`SCOPE.md`](SCOPE.md). Licence : propriétaire ([`LICENSE`](LICENSE)).
