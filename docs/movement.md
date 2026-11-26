# Déplacement

`engine-movement` réimplémente le modèle `CGameMovement` des jeux Source à
partir de la description publique de l'algorithme. Aucun code n'est copié.

- Friction au sol avec `stop_speed`, et accélération vers une vitesse souhaitée
  plafonnée.
- En l'air, l'accélération est plafonnée à 30 u/s de vitesse souhaitée. L'air
  strafe et le bunny hop en découlent ; ce n'est pas un mécanisme ajouté.
- Gravité intégrée en deux demi-pas, glissement le long des plans, marches de
  18 u, accrochage au sol dans les escaliers et les pentes.
- Pas de sprint, pas d'endurance, pas de contre-strafe automatique.
- La vitesse maximale dépend de l'arme en main (`configs/weapons.toml`).
- Le marquage (*tagging*) ralentit le joueur touché. La vitesse redevient normale
  en 0,35 s et ne descend jamais sous 50 %.

Constantes : `configs/movement.toml`. Ce sont des valeurs de référence de type
CS:GO : 250 u/s, `accelerate` 5,5, `friction` 5,2, gravité 800.

## Contre-strafe : deux modes

Le contre-strafe consiste à appuyer sur la direction opposée pour s'arrêter.
En physique Source pure, s'il est relâché trop tôt ou trop tard, il reste une
glissade résiduelle. Deux modes interchangeables sont fournis
(`counter_strafe = "source" | "buffered"`) :

- **`source`** : la physique d'origine, sans aide.
- **`buffered`** (défaut) : les entrées opposées sont horodatées en sous-tick.
  Si le joueur relâche la touche opposée alors qu'au moins 90 % de la vitesse a
  été annulée (`completion_ratio`), la vitesse restante, très faible, est
  supprimée. Le mode ne freine jamais plus fort que la physique et ne repart
  jamais dans l'autre sens. Il n'aide pas les tapotements courts.

Mesures du test `comparison_table` (`cargo test --release -p engine-movement --test counter_strafe comparison_table -- --nocapture`), à 128 Hz :

| Mode | Scénario | Précis après (ms) | Arrêt (ms) | Distance (u) | Vitesse résiduelle (u/s) |
|---|---|---|---|---|---|
| Source | relâche seule | 203.1 | 382.8 | 33.21 | 211.06 |
| Source | contre exact | 78.1 | 132.8 | 11.25 | 1.72 |
| Buffered | contre exact | 78.1 | 125.0 | 11.27 | 0.00 |
| Source / Buffered | contre 80 % | 78.1 | 187.5 | 12.44 | 33.97 |
| Source / Buffered | contre 130 % | 78.1 | 242.2 | 9.04 | 32.27 |
| Source / Buffered | A+D ensemble | 203.1 | 382.8 | 33.22 | 113.35 |

Ce que montrent les mesures :
- Le temps avant d'être précis est **identique** dans les deux modes : le mode
  buffered n'apporte aucun avantage de vitesse.
- Il supprime seulement la glissade d'environ 2 u/s lorsque le contre-strafe est
  bien dosé.
- Un contre-strafe trop court (80 %) ou trop long (130 %) est puni de la même
  façon dans les deux modes à 128 Hz.
- Les résultats à 64 et 256 Hz restent à un tick près (test
  `tick_rate_independent_within_one_tick`).

## Sensibilité

Lacet et tangage : 0,022° par compte × sensibilité, comme dans les jeux Source.
La rotation est appliquée par événement souris, sans lissage ni accélération ;
l'angle envoyé au serveur est celui de la frame rendue.
