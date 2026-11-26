//! Weapon state machine evaluated on sub-tick time stamps.

use crate::defs::{WeaponDef, WeaponKind};
use crate::spread::recoil_offset;
use engine_core::cmd::Buttons;

/// Ammunition of one gun.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct GunState {
    pub mag: u16,
    pub reserve: u16,
}

impl GunState {
    pub fn full(w: &WeaponDef) -> GunState {
        GunState {
            mag: w.mag_size,
            reserve: w.reserve,
        }
    }
}

/// State of the weapon in hand.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct HandState {
    pub next_attack: f64,
    pub deploy_until: f64,
    pub reload_until: Option<f64>,
    /// Shot counter driving the recoil pattern (recovers continuously).
    pub recoil_index: f32,
    pub last_shot: f64,
    pub zoom: u8,
    pub rezoom_at: Option<(f64, u8)>,
    pub attack_latched: bool,
    pub grenade_armed: bool,
    /// Consecutive shots of the current spray (statistics, Shot Inspector).
    pub burst: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WeaponEvent {
    Shot {
        time: f64,
        /// Recoil offset applied to this bullet (pitch up, yaw left), degrees.
        recoil: (f32, f32),
        recoil_index: f32,
        burst: u32,
    },
    DryFire {
        time: f64,
    },
    Melee {
        time: f64,
        heavy: bool,
    },
    Throw {
        time: f64,
        strength: f32,
    },
    ReloadStarted {
        time: f64,
    },
    ReloadFinished {
        time: f64,
    },
    Zoom {
        time: f64,
        level: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponInput {
    /// Segment boundaries in simulation seconds.
    pub t0: f64,
    pub t1: f64,
    pub buttons: Buttons,
    pub prev: Buttons,
    /// False during freeze time, planting, defusing, when dead...
    pub can_fire: bool,
}

impl HandState {
    /// State after drawing a weapon at `time`.
    pub fn deploy(w: &WeaponDef, time: f64, keep_recoil: Option<&HandState>) -> HandState {
        let mut h = HandState {
            deploy_until: time + w.deploy_time as f64,
            next_attack: time + w.deploy_time as f64,
            ..Default::default()
        };
        if let Some(prev) = keep_recoil {
            h.recoil_index = prev.recoil_index_at(w, time);
            h.last_shot = time;
        }
        h
    }

    /// Recoil shot counter at `time`, including recovery.
    pub fn recoil_index_at(&self, w: &WeaponDef, time: f64) -> f32 {
        let since = (time - self.last_shot - w.recoil_recovery_delay as f64).max(0.0) as f32;
        (self.recoil_index - w.recoil_recovery * since).max(0.0)
    }

    /// Current recoil offset (for the HUD and the cosmetic camera kick).
    pub fn recoil_now(&self, w: &WeaponDef, time: f64) -> (f32, f32) {
        recoil_offset(w, self.recoil_index_at(w, time))
    }

    pub fn is_reloading(&self) -> bool {
        self.reload_until.is_some()
    }

    fn start_reload(&mut self, w: &WeaponDef, gun: &GunState, t: f64, out: &mut Vec<WeaponEvent>) {
        if self.reload_until.is_none() && gun.mag < w.mag_size && gun.reserve > 0 {
            let start = t.max(self.deploy_until);
            self.reload_until = Some(start + w.reload_time as f64);
            self.rezoom_at = None;
            if self.zoom != 0 {
                self.zoom = 0;
                out.push(WeaponEvent::Zoom { time: t, level: 0 });
            }
            out.push(WeaponEvent::ReloadStarted { time: start });
        }
    }

    /// Advances the weapon over one constant-button segment.
    pub fn update(
        &mut self,
        w: &WeaponDef,
        gun: &mut GunState,
        input: &WeaponInput,
        out: &mut Vec<WeaponEvent>,
    ) {
        let WeaponInput {
            t0,
            t1,
            buttons,
            prev,
            can_fire,
        } = *input;
        let pressed = |b: u32| buttons.has(b) && !prev.has(b);

        if let Some(end) = self.reload_until
            && end <= t1
        {
            let need = w.mag_size - gun.mag;
            let take = need.min(gun.reserve);
            gun.mag += take;
            gun.reserve -= take;
            self.reload_until = None;
            self.recoil_index = 0.0;
            out.push(WeaponEvent::ReloadFinished { time: end });
        }
        if let Some((at, level)) = self.rezoom_at
            && at <= t1
        {
            self.rezoom_at = None;
            if buttons.has(Buttons::ATTACK2) || level > 0 {
                self.zoom = level;
                out.push(WeaponEvent::Zoom { time: at, level });
            }
        }

        match w.kind {
            WeaponKind::Knife => self.update_melee(w, input, out),
            WeaponKind::Grenade => self.update_grenade(input, out),
            WeaponKind::Bomb => {}
            _ => {
                if pressed(Buttons::RELOAD) {
                    self.start_reload(w, gun, t0, out);
                }
                if pressed(Buttons::ATTACK2)
                    && !w.zoom.levels.is_empty()
                    && self.reload_until.is_none()
                    && t0 >= self.deploy_until
                {
                    self.rezoom_at = None;
                    self.zoom = (self.zoom + 1) % (w.zoom.levels.len() as u8 + 1);
                    out.push(WeaponEvent::Zoom {
                        time: t0,
                        level: self.zoom,
                    });
                }
                self.update_gun(
                    w,
                    gun,
                    t0,
                    t1,
                    buttons.has(Buttons::ATTACK) && can_fire,
                    out,
                );
            }
        }
    }

    fn update_gun(
        &mut self,
        w: &WeaponDef,
        gun: &mut GunState,
        t0: f64,
        t1: f64,
        held: bool,
        out: &mut Vec<WeaponEvent>,
    ) {
        if !held {
            self.attack_latched = false;
            if t0 > self.last_shot + w.cycle_time as f64 * 1.5 {
                self.burst = 0;
            }
            return;
        }
        if self.reload_until.is_some() {
            // CS:GO behaviour: attacking during a reload does nothing.
            return;
        }
        let mut t = t0.max(self.next_attack).max(self.deploy_until);
        while t < t1 {
            if !w.automatic && self.attack_latched {
                break;
            }
            if gun.mag == 0 {
                if !self.attack_latched {
                    if gun.reserve > 0 {
                        self.start_reload(w, gun, t, out);
                    } else {
                        out.push(WeaponEvent::DryFire { time: t });
                        self.next_attack = t + 0.2;
                    }
                }
                self.attack_latched = true;
                break;
            }
            let idx = self.recoil_index_at(w, t);
            let recoil = recoil_offset(w, idx);
            if t > self.last_shot + w.cycle_time as f64 * 1.5 {
                self.burst = 0;
            }
            self.burst += 1;
            out.push(WeaponEvent::Shot {
                time: t,
                recoil,
                recoil_index: idx,
                burst: self.burst,
            });
            gun.mag -= 1;
            self.recoil_index = idx + 1.0;
            self.last_shot = t;
            self.next_attack = t + w.cycle_time as f64;
            self.attack_latched = true;
            if self.zoom > 0 && w.zoom.unzoom_on_fire {
                self.rezoom_at = if gun.mag > 0 {
                    Some((self.next_attack, self.zoom))
                } else {
                    None
                };
                self.zoom = 0;
                out.push(WeaponEvent::Zoom { time: t, level: 0 });
            }
            t = self.next_attack;
        }
    }

    fn update_melee(&mut self, w: &WeaponDef, input: &WeaponInput, out: &mut Vec<WeaponEvent>) {
        let heavy = input.buttons.has(Buttons::ATTACK2);
        let light = input.buttons.has(Buttons::ATTACK);
        if !input.can_fire || !(heavy || light) {
            return;
        }
        let mut t = input.t0.max(self.next_attack).max(self.deploy_until);
        while t < input.t1 {
            let is_heavy = heavy && !light;
            out.push(WeaponEvent::Melee {
                time: t,
                heavy: is_heavy,
            });
            self.next_attack = t + if is_heavy {
                w.melee.heavy_cycle as f64
            } else {
                w.cycle_time as f64
            };
            t = self.next_attack;
        }
    }

    fn update_grenade(&mut self, input: &WeaponInput, out: &mut Vec<WeaponEvent>) {
        let held = input.buttons.has(Buttons::ATTACK) || input.buttons.has(Buttons::ATTACK2);
        if held && input.can_fire && input.t0 >= self.deploy_until {
            self.grenade_armed = true;
            return;
        }
        if self.grenade_armed && !held {
            self.grenade_armed = false;
            let a1 = input.prev.has(Buttons::ATTACK);
            let a2 = input.prev.has(Buttons::ATTACK2);
            let strength = match (a1, a2) {
                (true, true) => 0.5,
                (false, true) => 0.0,
                _ => 1.0,
            };
            if input.can_fire {
                out.push(WeaponEvent::Throw {
                    time: input.t0.max(self.deploy_until),
                    strength,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::WeaponDef;

    fn rifle() -> WeaponDef {
        WeaponDef {
            kind: WeaponKind::Rifle,
            automatic: true,
            cycle_time: 0.1,
            mag_size: 30,
            reserve: 90,
            deploy_time: 0.0,
            recoil_pattern: vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.5], [3.0, -0.5]],
            recoil_recovery: 10.0,
            recoil_recovery_delay: 0.15,
            ..Default::default()
        }
    }

    fn run(
        h: &mut HandState,
        w: &WeaponDef,
        g: &mut GunState,
        b: u32,
        t0: f64,
        t1: f64,
        prev: u32,
    ) -> Vec<WeaponEvent> {
        let mut out = Vec::new();
        h.update(
            w,
            g,
            &WeaponInput {
                t0,
                t1,
                buttons: Buttons(b),
                prev: Buttons(prev),
                can_fire: true,
            },
            &mut out,
        );
        out
    }

    #[test]
    fn fire_rate_is_exact_regardless_of_segmentation() {
        let w = rifle();
        // One second of holding, split in 1, 128 or 1000 segments: same shot times.
        let mut reference = None;
        for n in [1usize, 128, 1000] {
            let mut h = HandState::default();
            let mut g = GunState::full(&w);
            let mut times = Vec::new();
            let mut prev = 0;
            for i in 0..n {
                let t0 = i as f64 / n as f64;
                let t1 = (i + 1) as f64 / n as f64;
                for e in run(&mut h, &w, &mut g, Buttons::ATTACK, t0, t1, prev) {
                    if let WeaponEvent::Shot { time, .. } = e {
                        times.push(time);
                    }
                }
                prev = Buttons::ATTACK;
            }
            assert_eq!(times.len(), 10, "{n}");
            match &reference {
                None => reference = Some(times),
                Some(r) => {
                    for (a, b) in r.iter().zip(&times) {
                        assert!((a - b).abs() < 1e-9);
                    }
                }
            }
        }
    }

    #[test]
    fn first_shot_has_no_recoil_and_spray_is_reproducible() {
        let w = rifle();
        let spray = || {
            let mut h = HandState::default();
            let mut g = GunState::full(&w);
            run(&mut h, &w, &mut g, Buttons::ATTACK, 0.0, 0.35, 0)
                .into_iter()
                .filter_map(|e| {
                    if let WeaponEvent::Shot { recoil, .. } = e {
                        Some(recoil)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        let a = spray();
        assert_eq!(a[0], (0.0, 0.0));
        assert_eq!(a, spray());
        assert_eq!(a.len(), 4);
        // The recovery delay (0.15 s) is longer than the cycle (0.1 s): a spray climbs.
        assert!(a[3].0 > a[1].0);
    }

    #[test]
    fn recoil_recovers_to_zero_after_a_pause() {
        let w = rifle();
        let mut h = HandState::default();
        let mut g = GunState::full(&w);
        run(&mut h, &w, &mut g, Buttons::ATTACK, 0.0, 0.35, 0);
        run(&mut h, &w, &mut g, 0, 0.35, 1.0, Buttons::ATTACK);
        assert_eq!(h.recoil_index_at(&w, 1.0), 0.0);
        let shots = run(&mut h, &w, &mut g, Buttons::ATTACK, 1.0, 1.01, 0);
        assert!(matches!(
            shots[0],
            WeaponEvent::Shot {
                recoil: (0.0, 0.0),
                ..
            }
        ));
    }

    #[test]
    fn semi_auto_needs_a_new_press() {
        let w = WeaponDef {
            automatic: false,
            cycle_time: 0.15,
            deploy_time: 0.0,
            ..Default::default()
        };
        let mut h = HandState::default();
        let mut g = GunState::full(&w);
        let n = run(&mut h, &w, &mut g, Buttons::ATTACK, 0.0, 1.0, 0).len();
        assert_eq!(n, 1);
        run(&mut h, &w, &mut g, 0, 1.0, 1.05, Buttons::ATTACK);
        assert_eq!(
            run(&mut h, &w, &mut g, Buttons::ATTACK, 1.05, 1.1, 0).len(),
            1
        );
    }

    #[test]
    fn reload_cycle() {
        let w = rifle();
        let mut h = HandState::default();
        let mut g = GunState {
            mag: 0,
            reserve: 50,
        };
        let ev = run(&mut h, &w, &mut g, Buttons::ATTACK, 0.0, 0.1, 0);
        assert!(matches!(ev[0], WeaponEvent::ReloadStarted { .. }));
        assert!(
            run(
                &mut h,
                &w,
                &mut g,
                Buttons::ATTACK,
                0.1,
                1.0,
                Buttons::ATTACK
            )
            .is_empty()
        );
        let ev = run(&mut h, &w, &mut g, 0, 1.0, 3.0, Buttons::ATTACK);
        assert!(matches!(ev[0], WeaponEvent::ReloadFinished { .. }));
        assert_eq!(
            g,
            GunState {
                mag: 30,
                reserve: 20
            }
        );
    }

    #[test]
    fn deploy_delays_first_shot() {
        let mut w = rifle();
        w.deploy_time = 0.5;
        let mut h = HandState::deploy(&w, 0.0, None);
        let mut g = GunState::full(&w);
        assert!(run(&mut h, &w, &mut g, Buttons::ATTACK, 0.0, 0.4, 0).is_empty());
        let ev = run(
            &mut h,
            &w,
            &mut g,
            Buttons::ATTACK,
            0.4,
            0.55,
            Buttons::ATTACK,
        );
        assert!(matches!(ev[0], WeaponEvent::Shot { time, .. } if (time - 0.5).abs() < 1e-9));
    }

    #[test]
    fn grenade_throw_strengths() {
        let w = WeaponDef {
            kind: WeaponKind::Grenade,
            deploy_time: 0.0,
            ..Default::default()
        };
        let mut g = GunState::default();
        for (held, s) in [
            (Buttons::ATTACK, 1.0),
            (Buttons::ATTACK2, 0.0),
            (Buttons::ATTACK | Buttons::ATTACK2, 0.5),
        ] {
            let mut h = HandState::default();
            run(&mut h, &w, &mut g, held, 0.0, 0.1, 0);
            let ev = run(&mut h, &w, &mut g, 0, 0.1, 0.2, held);
            assert!(
                matches!(ev[0], WeaponEvent::Throw { strength, .. } if strength == s),
                "{ev:?}"
            );
        }
    }
}
