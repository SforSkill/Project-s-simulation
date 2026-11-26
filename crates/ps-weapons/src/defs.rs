//! Weapon and equipment definitions (`configs/weapons.toml`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct WeaponId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponKind {
    Knife,
    Pistol,
    Smg,
    Rifle,
    SniperLight,
    SniperHeavy,
    Grenade,
    Bomb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrenadeKind {
    Flash,
    Smoke,
    Molotov,
    He,
}

impl GrenadeKind {
    pub fn to_u8(self) -> u8 {
        match self {
            GrenadeKind::Flash => 0,
            GrenadeKind::Smoke => 1,
            GrenadeKind::Molotov => 2,
            GrenadeKind::He => 3,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => GrenadeKind::Flash,
            1 => GrenadeKind::Smoke,
            2 => GrenadeKind::Molotov,
            3 => GrenadeKind::He,
            _ => return None,
        })
    }
}

/// Inventory slot (keyboard 1..5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Primary,
    Secondary,
    Melee,
    Grenade,
    Bomb,
}

impl Slot {
    pub fn index(self) -> usize {
        match self {
            Slot::Primary => 0,
            Slot::Secondary => 1,
            Slot::Melee => 2,
            Slot::Grenade => 3,
            Slot::Bomb => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoomDef {
    /// FOV (4:3 horizontal, degrees) of each zoom level.
    pub levels: Vec<f32>,
    /// Seconds before a zoom level is usable.
    pub zoom_time: f32,
    /// Drop the scope after each shot, re-scope at the end of the cycle.
    pub unzoom_on_fire: bool,
    /// Movement speed while scoped.
    pub scoped_speed: f32,
}

impl Default for ZoomDef {
    fn default() -> Self {
        Self {
            levels: Vec::new(),
            zoom_time: 0.1,
            unzoom_on_fire: false,
            scoped_speed: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeleeDef {
    pub range: f32,
    pub damage: f32,
    pub backstab_damage: f32,
    pub heavy_range: f32,
    pub heavy_damage: f32,
    pub heavy_backstab_damage: f32,
    pub heavy_cycle: f32,
}

impl Default for MeleeDef {
    fn default() -> Self {
        Self {
            range: 48.0,
            damage: 40.0,
            backstab_damage: 90.0,
            heavy_range: 32.0,
            heavy_damage: 65.0,
            heavy_backstab_damage: 180.0,
            heavy_cycle: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeaponDef {
    pub id: String,
    pub name: String,
    pub kind: WeaponKind,
    pub slot: Slot,
    pub price: u32,
    pub kill_reward: u32,
    /// Base damage per bullet at 0 distance.
    pub damage: f32,
    /// Damage multiplier per 500 units (CS:GO `RangeModifier`).
    pub range_modifier: f32,
    pub range: f32,
    pub head_mult: f32,
    pub chest_mult: f32,
    pub stomach_mult: f32,
    pub arms_mult: f32,
    pub legs_mult: f32,
    /// Fraction of damage reaching health on an armored hit (CS:GO `ArmorRatio` / 2).
    pub armor_ratio: f32,
    /// Penetration power (see `damage::penetrate`).
    pub penetration: f32,
    /// Seconds between two shots.
    pub cycle_time: f32,
    pub automatic: bool,
    pub mag_size: u16,
    pub reserve: u16,
    pub reload_time: f32,
    pub deploy_time: f32,
    /// Movement speed with this weapon in hand.
    pub max_speed: f32,
    /// Speed fraction under which the weapon is perfectly accurate.
    pub accurate_fraction: f32,
    /// Maximum movement spread (degrees, half-angle) reached at full speed.
    pub move_spread: f32,
    /// Spread while airborne (degrees).
    pub air_spread: f32,
    /// Multiplier of `move_spread` when a zoomable weapon is used without scope.
    pub unscoped_move_spread_mult: f32,
    /// Cumulative recoil pattern: (pitch up, yaw left) in degrees for shot index i.
    pub recoil_pattern: Vec<[f32; 2]>,
    /// Recoil shot-counter recovery, in shots per second.
    pub recoil_recovery: f32,
    /// Delay after the last shot before recovery starts (s).
    pub recoil_recovery_delay: f32,
    /// Speed reduction applied to an unarmored target hit by this weapon.
    pub tagging: f32,
    pub zoom: ZoomDef,
    pub melee: MeleeDef,
    pub grenade: Option<GrenadeKind>,
    /// Maximum count carried (grenades).
    pub max_carry: u8,
}

impl Default for WeaponDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: WeaponKind::Pistol,
            slot: Slot::Secondary,
            price: 0,
            kill_reward: 300,
            damage: 30.0,
            range_modifier: 0.9,
            range: 8192.0,
            head_mult: 4.0,
            chest_mult: 1.0,
            stomach_mult: 1.25,
            arms_mult: 1.0,
            legs_mult: 0.75,
            armor_ratio: 0.5,
            penetration: 1.0,
            cycle_time: 0.15,
            automatic: false,
            mag_size: 12,
            reserve: 24,
            reload_time: 2.2,
            deploy_time: 0.8,
            max_speed: 240.0,
            accurate_fraction: 0.34,
            move_spread: 3.0,
            air_spread: 6.0,
            unscoped_move_spread_mult: 1.0,
            recoil_pattern: vec![[0.0, 0.0]],
            recoil_recovery: 10.0,
            recoil_recovery_delay: 0.0,
            tagging: 0.3,
            zoom: ZoomDef::default(),
            melee: MeleeDef::default(),
            grenade: None,
            max_carry: 1,
        }
    }
}

impl WeaponDef {
    pub fn is_gun(&self) -> bool {
        matches!(
            self.kind,
            WeaponKind::Pistol
                | WeaponKind::Smg
                | WeaponKind::Rifle
                | WeaponKind::SniperLight
                | WeaponKind::SniperHeavy
        )
    }

    /// FOV ratio of a zoom level (1.0 unscoped), for sensitivity scaling.
    pub fn zoom_fov(&self, level: u8) -> Option<f32> {
        if level == 0 {
            None
        } else {
            self.zoom.levels.get(level as usize - 1).copied()
        }
    }

    pub fn speed(&self, zoom_level: u8) -> f32 {
        if zoom_level > 0 && self.zoom.scoped_speed > 0.0 {
            self.zoom.scoped_speed
        } else {
            self.max_speed
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EquipmentDef {
    pub kevlar_price: u32,
    pub kevlar_helmet_price: u32,
    pub helmet_upgrade_price: u32,
    pub defuse_kit_price: u32,
    pub max_grenades: u8,
}

impl Default for EquipmentDef {
    fn default() -> Self {
        Self {
            kevlar_price: 650,
            kevlar_helmet_price: 1000,
            helmet_upgrade_price: 350,
            defuse_kit_price: 400,
            max_grenades: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Catalog {
    #[serde(rename = "weapon")]
    pub weapons: Vec<WeaponDef>,
    #[serde(default)]
    pub equipment: EquipmentDef,
}

impl Catalog {
    pub fn from_toml(text: &str) -> Result<Catalog, String> {
        let c: Catalog = toml::from_str(text).map_err(|e| e.to_string())?;
        c.validate()?;
        Ok(c)
    }

    pub fn get(&self, id: WeaponId) -> &WeaponDef {
        &self.weapons[id.0 as usize]
    }

    pub fn try_get(&self, id: WeaponId) -> Option<&WeaponDef> {
        self.weapons.get(id.0 as usize)
    }

    pub fn by_name(&self, name: &str) -> Option<WeaponId> {
        self.weapons
            .iter()
            .position(|w| w.id == name)
            .map(|i| WeaponId(i as u8))
    }

    pub fn grenade(&self, kind: GrenadeKind) -> Option<WeaponId> {
        self.weapons
            .iter()
            .position(|w| w.grenade == Some(kind))
            .map(|i| WeaponId(i as u8))
    }

    pub fn first_of(&self, kind: WeaponKind) -> Option<WeaponId> {
        self.weapons
            .iter()
            .position(|w| w.kind == kind)
            .map(|i| WeaponId(i as u8))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.weapons.len() > 250 {
            return Err("too many weapons".into());
        }
        for w in &self.weapons {
            let e = |m: &str| Err(format!("weapon {}: {m}", w.id));
            if w.id.is_empty() {
                return Err("weapon without id".into());
            }
            if w.cycle_time <= 0.0 || !w.cycle_time.is_finite() {
                return e("cycle_time must be > 0");
            }
            if w.max_speed <= 0.0 {
                return e("max_speed must be > 0");
            }
            if !(0.0..=1.0).contains(&w.armor_ratio) || !(0.0..1.0).contains(&w.accurate_fraction) {
                return e("armor_ratio in [0,1], accurate_fraction in [0,1)");
            }
            if w.recoil_pattern.is_empty() {
                return e("recoil_pattern must have at least one entry");
            }
            if w.recoil_pattern[0] != [0.0, 0.0] {
                return e("the first recoil entry must be [0, 0] (first shot exact)");
            }
            if !(0.0..=1.0).contains(&w.tagging) {
                return e("tagging in [0,1]");
            }
            if w.kind == WeaponKind::Grenade && w.grenade.is_none() {
                return e("grenade weapon without grenade kind");
            }
            if w.move_spread < 0.0 || w.air_spread < 0.0 {
                return e("spreads must be >= 0");
            }
        }
        for k in [WeaponKind::Knife, WeaponKind::Pistol] {
            if !self.weapons.iter().any(|w| w.kind == k) {
                return Err(format!("catalog needs a {k:?}"));
            }
        }
        Ok(())
    }
}
