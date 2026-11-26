//! Game data loaded from the data directory (`configs/`, `maps/`).

use engine_movement::MovementConfig;
use engine_physics::map::MapDef;
use engine_physics::material::MaterialTable;
use engine_weapons::Catalog;
use game_rules::{RulesConfig, RulesFile};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct GameData {
    pub catalog: Catalog,
    pub materials: MaterialTable,
    pub movement: MovementConfig,
    pub rules: RulesFile,
    pub maps: BTreeMap<String, MapDef>,
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

impl GameData {
    /// Loads and validates everything under `dir` (which contains `configs/` and `maps/`).
    pub fn load(dir: &Path) -> Result<GameData, String> {
        let cfg = dir.join("configs");
        let catalog = Catalog::from_toml(&read(&cfg.join("weapons.toml"))?)
            .map_err(|e| format!("weapons.toml: {e}"))?;
        let materials: MaterialTable = toml::from_str(&read(&cfg.join("materials.toml"))?)
            .map_err(|e| format!("materials.toml: {e}"))?;
        materials.validate()?;
        if materials.materials.first().map(|m| m.name.as_str()) != Some("clip") {
            return Err("materials.toml: the first material must be \"clip\"".into());
        }
        let movement: MovementConfig = toml::from_str(&read(&cfg.join("movement.toml"))?)
            .map_err(|e| format!("movement.toml: {e}"))?;
        movement.validate()?;
        let rules = RulesFile::from_toml(&read(&cfg.join("rules.toml"))?)
            .map_err(|e| format!("rules.toml: {e}"))?;
        let mut maps = BTreeMap::new();
        let mdir = dir.join("maps");
        let mut entries: Vec<_> = std::fs::read_dir(&mdir)
            .map_err(|e| format!("{}: {e}", mdir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries {
            if p.extension().and_then(|e| e.to_str()) != Some("ron") {
                continue;
            }
            let map: MapDef =
                ron::from_str(&read(&p)?).map_err(|e| format!("{}: {e}", p.display()))?;
            maps.insert(map.name.clone(), map);
        }
        if maps.is_empty() {
            return Err(format!("no map found in {}", mdir.display()));
        }
        Ok(GameData {
            catalog,
            materials,
            movement,
            rules,
            maps,
        })
    }

    pub fn rules_profile(&self, name: &str) -> Result<RulesConfig, String> {
        self.rules.profiles.get(name).cloned().ok_or_else(|| {
            format!(
                "unknown rules profile '{name}' (available: {})",
                self.rules
                    .profiles
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
    }

    /// Stable fingerprint of the gameplay data, exchanged at connection time so a
    /// client with different configs/maps is refused (prediction would diverge).
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |s: &str| {
            for b in s.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        feed(&format!("{:?}", self.catalog));
        feed(&format!("{:?}", self.materials));
        feed(&format!("{:?}", self.movement));
        for m in self.maps.values() {
            feed(&format!("{m:?}"));
        }
        h
    }
}
