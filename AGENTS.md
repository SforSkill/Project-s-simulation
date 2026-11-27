# Project-s-simulation (contexte agents)

Simulation partagée de Project S (serveur autoritaire, serveur local, prédiction). Lire [`SCOPE.md`](SCOPE.md).

- Simulation déterministe et sans E/S : l'aléa passe par `DetRng`, le temps par `TickRate`/`SimTime`. Jamais de Bevy, de tokio, de HTTP ni de SQL (`boundaries.toml`).
- La prédiction (`ps-sim-net/src/predict.rs`) suit exactement `Sim::run_cmd` : modifier l'un, c'est vérifier l'autre.
- Valeurs de gameplay : dans Project-s-content, jamais en dur. Le feeling validé (`SCOPE.md` §14) ne change pas sans décision explicite.
- Changement de `proto.rs` ou du format de replay : `PROTOCOL_VERSION`, `docs/network.md` (Project-s-netcode) et les consommateurs (serveur, client) dans la même série de PR.
- Gros fichiers à lire par plage : `ps-sim/src/sim.rs` (~2 350 lignes), `ps-rules/src/lib.rs`.

Vérification : `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --release --locked` avec Project-s-content voisin ou `PS_CONTENT_DIR`.
