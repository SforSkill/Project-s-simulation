# Armes

Définitions : `configs/weapons.toml`. Les noms et les chiffres sont originaux.

| Id | Nom | Type | Prix | Dégâts | Cadence (s) | Vitesse max |
|---|---|---|---|---|---|---|
| knife | Couteau | mêlée | 0 | 40 | — | 250 |
| pistol | P-9 « Rivet » | pistolet | 200 | 30 | voir config | 240 |
| smg | SM-4 « Wasp » | mitraillette | 1250 | 26 | voir config | 240 |
| rifle | AR-7 « Kestrel » | fusil d'assaut | 2700 | 36 | 0,1 | 220 |
| sniper_light | LR-2 « Marten » | fusil de précision léger | 950 | 101 | voir config | 230 |
| sniper_heavy | HX-50 « Bastion » | fusil de précision lourd | 4700 | 150 | voir config | 190 |

Grenades : flash (200), fumigène (300), molotov (500), explosive (300).
Équipement : gilet, gilet + casque, kit de désamorçage.

## Règles

Elles sont appliquées par `engine-weapons` et vérifiées par ses tests.

1. **Pas de dispersion intrinsèque.** Au sol, sous le seuil de précision
   (`accurate_fraction` × vitesse max de l'arme), la balle part exactement là où
   pointent la vue et le recul accumulé. Le premier tir est exact.
2. **Recul à motif fixe** par arme (`recoil_pattern`, en degrés). L'indice de
   tir récupère en continu (`recoil_recovery`, après `recoil_recovery_delay`) :
   les mêmes entrées donnent toujours la même rafale. Aucun recul aléatoire.
3. **Dispersion aléatoire uniquement en mouvement** : au-delà du seuil
   (`move_spread`, croissant avec la vitesse réelle) ou en l'air (`air_spread`).
4. **Dégâts déterministes** :
   - atténuation avec la distance (`range_modifier`) ;
   - multiplicateur selon la zone touchée (tête ×4 pour le fusil) ;
   - armure (`armor_ratio`) ;
   - pénétration (`penetration`, comparée à l'épaisseur et au matériau traversés).
5. **Cadences en secondes**, évaluées à l'horodatage sous-tick du clic :
   indépendantes du FPS et du tick.

## Aléa et triche

Le générateur (`engine-core/src/rng.rs`) n'est utilisé que pour la dispersion
de mouvement et les décisions des bots. Pour chaque tir, le serveur dérive le
flux à partir d'une graine de match secrète : un client ne peut pas prévoir la
dispersion de son prochain tir. La prédiction locale de l'impact sert uniquement
aux effets visuels ; seul le serveur décide.

## Inspecteur de tirs (F3)

Pour chaque tir, l'inspecteur affiche :
- le tick et le `view_time` utilisé ;
- le rembobinage appliqué ;
- la capsule touchée ;
- les surfaces traversées (épaisseur, balle arrêtée ou traversée, dégâts
  restants) ;
- les dégâts finaux.
