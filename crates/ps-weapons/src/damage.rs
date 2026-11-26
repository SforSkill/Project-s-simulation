//! Deterministic damage rules.

use crate::defs::WeaponDef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HitGroup {
    Head,
    Chest,
    Stomach,
    Arms,
    Legs,
}

impl HitGroup {
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<HitGroup> {
        Some(match v {
            0 => HitGroup::Head,
            1 => HitGroup::Chest,
            2 => HitGroup::Stomach,
            3 => HitGroup::Arms,
            4 => HitGroup::Legs,
            _ => return None,
        })
    }

    pub fn multiplier(self, w: &WeaponDef) -> f32 {
        match self {
            HitGroup::Head => w.head_mult,
            HitGroup::Chest => w.chest_mult,
            HitGroup::Stomach => w.stomach_mult,
            HitGroup::Arms => w.arms_mult,
            HitGroup::Legs => w.legs_mult,
        }
    }
}

/// CS:GO-style range falloff: `damage × range_modifier^(distance / 500)`.
pub fn distance_falloff(w: &WeaponDef, distance: f32) -> f32 {
    w.range_modifier.powf(distance.max(0.0) / 500.0)
}

/// Splits raw damage between health and armor (CS:GO rules): kevlar protects
/// everything but the head (unless helmet) and the legs; half of the absorbed damage
/// is removed from the armor value.
/// Returns `(health_damage, armor_damage)`, both integers.
pub fn apply_armor(
    raw: f32,
    group: HitGroup,
    armor: u8,
    helmet: bool,
    armor_ratio: f32,
) -> (u32, u32) {
    let protected = armor > 0
        && match group {
            HitGroup::Head => helmet,
            HitGroup::Legs => false,
            _ => true,
        };
    if !protected {
        return (raw.max(0.0) as u32, 0);
    }
    let mut health = raw * armor_ratio;
    let mut armor_dmg = (raw - health) * 0.5;
    if armor_dmg > armor as f32 {
        armor_dmg = armor as f32;
        health = raw - armor_dmg * 2.0;
    }
    (health.max(0.0) as u32, armor_dmg.max(0.0) as u32)
}

/// One material crossed by a bullet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenetrationInput {
    pub thickness: f32,
    /// Material `penetration_modifier` (0 = impenetrable).
    pub modifier: f32,
    pub retention_per_unit: f32,
    pub entry_loss: f32,
}

/// Thickness (units) of a modifier-1 material that one point of penetration power
/// goes through.
pub const PENETRATION_UNIT: f32 = 8.0;

/// Updates `(damage, power)` after crossing a surface. Returns false if the bullet
/// stops inside. Pure arithmetic: no randomness.
pub fn penetrate(damage: &mut f32, power: &mut f32, m: PenetrationInput) -> bool {
    if m.modifier <= 0.0 {
        return false;
    }
    let cost = m.thickness * m.modifier / PENETRATION_UNIT;
    if cost > *power {
        return false;
    }
    *power -= cost;
    *damage *= (1.0 - m.entry_loss) * m.retention_per_unit.powf(m.thickness);
    *damage > 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armor_rules() {
        // Unarmored: full damage.
        assert_eq!(
            apply_armor(100.0, HitGroup::Chest, 0, false, 0.775),
            (100, 0)
        );
        // Kevlar on chest.
        let (h, a) = apply_armor(100.0, HitGroup::Chest, 100, false, 0.775);
        assert_eq!((h, a), (77, 11));
        // Head without helmet: unprotected.
        assert_eq!(
            apply_armor(144.0, HitGroup::Head, 100, false, 0.775),
            (144, 0)
        );
        // Head with helmet: protected.
        assert_eq!(apply_armor(144.0, HitGroup::Head, 100, true, 0.775).0, 111);
        // Legs are never protected.
        assert_eq!(apply_armor(27.0, HitGroup::Legs, 100, true, 0.5), (27, 0));
        // Armor running out.
        let (h, a) = apply_armor(100.0, HitGroup::Chest, 5, false, 0.5);
        assert_eq!((h, a), (90, 5));
    }

    #[test]
    fn penetration_is_deterministic_and_bounded() {
        let wood = PenetrationInput {
            thickness: 8.0,
            modifier: 0.5,
            retention_per_unit: 0.99,
            entry_loss: 0.1,
        };
        let concrete = PenetrationInput {
            thickness: 32.0,
            modifier: 2.0,
            retention_per_unit: 0.97,
            entry_loss: 0.25,
        };
        let mut d = 36.0;
        let mut p = 2.0;
        assert!(penetrate(&mut d, &mut p, wood));
        assert!((p - 1.5).abs() < 1e-6);
        assert!((d - 36.0 * 0.9 * 0.99f32.powf(8.0)).abs() < 1e-4);
        assert!(!penetrate(&mut d, &mut p, concrete));
        let solid = PenetrationInput {
            modifier: 0.0,
            ..wood
        };
        assert!(!penetrate(&mut 50.0, &mut 10.0, solid));
    }
}
