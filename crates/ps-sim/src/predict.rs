//! Client-side movement prediction: the exact movement path of
//! `Sim::run_cmd` (same segments, same collision), without weapons or rules.

use crate::collide::{PlayerWorld, player_box};
use crate::data::GameData;
use crate::level::Level;
use crate::proto::SelfState;
use engine_core::Aabb;
use engine_core::cmd::UserCmd;
use engine_movement::MoveState;
use engine_movement::controller::{MoveInput, hull_for, simulate_segment};
use engine_weapons::{Slot, WeaponKind};
use glam::Vec3;

/// Movement speed cap of the weapon in hand (same rule as `Player::speed_cap`).
pub fn speed_cap(data: &GameData, me: &SelfState) -> f32 {
    let cat = &data.catalog;
    let w = match me.active {
        Slot::Primary => me.primary.as_ref().map(|w| w.0),
        Slot::Secondary => me.secondary.as_ref().map(|w| w.0),
        Slot::Melee => me.knife,
        Slot::Grenade => me.grenades.get(me.grenade_index as usize).copied(),
        Slot::Bomb => cat.first_of(WeaponKind::Bomb),
    };
    w.map(|w| cat.get(w).speed(me.hand.zoom)).unwrap_or(250.0)
}

/// Collision boxes of other players from their (interpolated) origins.
pub fn boxes(data: &GameData, others: &[(Vec3, bool)]) -> Vec<Aabb> {
    let (mins, maxs) = hull_for(&data.movement, false);
    let (dmins, dmaxs) = hull_for(&data.movement, true);
    others
        .iter()
        .map(|(o, ducked)| {
            if *ducked {
                player_box(*o, dmins, dmaxs)
            } else {
                player_box(*o, mins, maxs)
            }
        })
        .collect()
}

/// Applies the movement of one command.
pub fn predict_cmd(
    data: &GameData,
    level: &Level,
    mv: &mut MoveState,
    cmd: &UserCmd,
    max_speed: f32,
    frozen: bool,
    boxes: &[Aabb],
    dt: f64,
) {
    let pw = PlayerWorld {
        world: &level.world,
        boxes,
    };
    for seg in cmd.segments() {
        let sdt = (seg.t1 - seg.t0) * dt as f32;
        if sdt <= 0.0 {
            continue;
        }
        let input = MoveInput {
            buttons: seg.buttons,
            view: seg.view,
            max_speed,
            frozen,
            dt: sdt,
        };
        simulate_segment(mv, &input, &data.movement, &pw);
    }
}
