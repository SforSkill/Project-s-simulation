//! Simple server-side bots. They produce ordinary `UserCmd`s (same rules as
//! humans: no aim assistance, they counter-strafe before shooting).

use crate::player::Player;
use crate::sim::{BombState, Sim};
use engine_core::cmd::{Buttons, UserCmd};
use engine_core::math::{forward_to_angles, wrap_degrees};
use engine_core::{DetRng, PlayerId, SimTime, Team, ViewAngles};
use engine_weapons::{Slot, WeaponKind};
use game_rules::{Mode, Phase};
use glam::Vec3;

#[derive(Debug, Clone)]
pub struct BotBrain {
    pub id: PlayerId,
    rng: DetRng,
    /// 0 = easy, 1 = hard.
    pub skill: f32,
    path: Vec<Vec3>,
    path_goal: Option<Vec3>,
    repath_at: f64,
    target: Option<PlayerId>,
    seen_since: f64,
    aim_error: (f32, f32),
    last_pos: Vec3,
    stuck_since: f64,
    strafe_until: f64,
    strafe_dir: i8,
    fire_toggle: bool,
    site_pref: u8,
}

impl BotBrain {
    pub fn new(id: PlayerId, seed: u64, skill: f32) -> BotBrain {
        let mut rng = DetRng::for_key(seed, 0xB07, id.0 as u64);
        let site_pref = rng.below(2) as u8;
        BotBrain {
            id,
            rng,
            skill: skill.clamp(0.0, 1.0),
            path: Vec::new(),
            path_goal: None,
            repath_at: 0.0,
            target: None,
            seen_since: 0.0,
            aim_error: (0.0, 0.0),
            last_pos: Vec3::ZERO,
            stuck_since: 0.0,
            strafe_until: 0.0,
            strafe_dir: 1,
            fire_toggle: false,
            site_pref,
        }
    }

    fn reaction_time(&self) -> f64 {
        0.45 - 0.3 * self.skill as f64
    }

    fn turn_rate(&self) -> f32 {
        // Degrees per second.
        260.0 + 520.0 * self.skill
    }

    fn enemy_of(sim: &Sim, me: &Player, o: &Player) -> bool {
        o.id != me.id
            && o.alive
            && (sim.mode() == Mode::Deathmatch || (o.team != me.team && o.team.is_playing()))
    }

    /// Visible enemy closest to the crosshair (within the field of view).
    fn find_target(&self, sim: &Sim, me: &Player) -> Option<(PlayerId, Vec3)> {
        let cfg = &sim.data.movement;
        let eye = me.mv.eye(cfg);
        let fwd = me.view.forward();
        let mut best: Option<(f32, PlayerId, Vec3)> = None;
        for o in sim.iter_players() {
            if !Self::enemy_of(sim, me, o) {
                continue;
            }
            let chest = o.mv.origin + Vec3::Y * (o.mv.eye_height(cfg) - 14.0);
            let head = o.mv.eye(cfg);
            let d = chest - eye;
            let dist = d.length();
            if dist > 3500.0 {
                continue;
            }
            let dot = fwd.dot(d / dist.max(1.0));
            // Already-tracked target stays visible at any angle.
            if dot < 0.35 && self.target != Some(o.id) {
                continue;
            }
            let point = if sim.visible(eye, chest) {
                chest
            } else if sim.visible(eye, head) {
                head
            } else {
                continue;
            };
            let score = dist * (2.0 - dot);
            if best.is_none_or(|b| score < b.0) {
                best = Some((score, o.id, point));
            }
        }
        best.map(|b| (b.1, b.2))
    }

    fn goal(&mut self, sim: &Sim, me: &Player) -> Option<Vec3> {
        let def = &sim.level.def;
        match sim.mode() {
            Mode::Deathmatch | Mode::Duel | Mode::Training => {
                // Head toward the nearest enemy (or a random DM spawn).
                let enemy = sim
                    .iter_players()
                    .filter(|o| Self::enemy_of(sim, me, o))
                    .min_by(|a, b| {
                        a.mv.origin
                            .distance(me.mv.origin)
                            .total_cmp(&b.mv.origin.distance(me.mv.origin))
                    });
                if let Some(e) = enemy {
                    return Some(e.mv.origin);
                }
                let pts = if def.dm_spawns.is_empty() {
                    &def.spawns
                } else {
                    &def.dm_spawns
                };
                pts.get(self.rng.below(pts.len().max(1) as u32) as usize)
                    .map(|s| s.pos)
            }
            Mode::Competitive => {
                let site_center = |i: usize| {
                    def.bomb_sites
                        .get(i)
                        .map(|z| z.aabb().center() - Vec3::Y * (z.aabb().half_extents().y - 1.0))
                };
                match (&sim.bomb, me.team) {
                    (BombState::Planted { pos, .. }, Team::Defense) => Some(*pos),
                    (BombState::Planted { pos, .. }, Team::Attack) => {
                        Some(*pos + Vec3::new(120.0, 0.0, 60.0))
                    }
                    (BombState::Dropped(pos), Team::Attack) => Some(*pos),
                    (BombState::Carried(c), Team::Attack) if *c != me.id => {
                        // Escort the carrier toward the chosen site.
                        site_center(self.site_pref as usize)
                    }
                    _ => site_center(self.site_pref as usize),
                }
            }
        }
    }

    fn command(
        &self,
        sim: &Sim,
        me: &Player,
        buttons: Buttons,
        view: ViewAngles,
        select: u8,
    ) -> UserCmd {
        UserCmd {
            tick: sim.tick,
            buttons_start: buttons,
            view_start: me.view,
            view_end: view.normalized(),
            events: Vec::new(),
            weapon_select: select,
            view_time: SimTime::new(sim.tick, 0.0),
        }
    }

    /// Movement keys toward `dir` (world, horizontal) relative to the view yaw.
    fn move_keys(view: ViewAngles, dir: Vec3) -> Buttons {
        let mut b = Buttons(0);
        let d = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();
        if d == Vec3::ZERO {
            return b;
        }
        let f = view.forward_flat().dot(d);
        let r = view.right_flat().dot(d);
        if f > 0.38 {
            b = b.with(Buttons::FORWARD, true);
        } else if f < -0.38 {
            b = b.with(Buttons::BACK, true);
        }
        if r > 0.38 {
            b = b.with(Buttons::RIGHT, true);
        } else if r < -0.38 {
            b = b.with(Buttons::LEFT, true);
        }
        b
    }

    /// Counter-strafe: press the keys opposite to the current velocity.
    fn stop_keys(view: ViewAngles, vel: Vec3) -> Buttons {
        let v = Vec3::new(vel.x, 0.0, vel.z);
        if v.length() < 30.0 {
            return Buttons(0);
        }
        Self::move_keys(view, -v)
    }

    fn turn_toward(&self, from: ViewAngles, to: ViewAngles, dt: f32) -> ViewAngles {
        let max = self.turn_rate() * dt;
        let dy = wrap_degrees(to.yaw - from.yaw);
        let dp = to.pitch - from.pitch;
        // Fast flick when far, slower settle when close.
        let k = |d: f32| {
            let step = (d.abs() * 0.35).max(max * 0.15).min(max);
            d.signum() * step.min(d.abs())
        };
        ViewAngles::new(from.pitch + k(dp), from.yaw + k(dy))
    }

    pub fn think(&mut self, sim: &Sim) -> Option<UserCmd> {
        let me = sim.player(self.id)?;
        if !me.alive || !me.team.is_playing() {
            self.target = None;
            self.path.clear();
            return None;
        }
        let now = sim.now();
        let dt = sim.dt() as f32;
        let cfg = &sim.data.movement;
        let cat = &sim.data.catalog;
        let eye = me.mv.eye(cfg);
        let frozen = sim.mode().has_rounds() && sim.rules.players_frozen();
        let mut select = 0u8;
        // Prefer the best gun.
        let wanted = if me.inv.primary.is_some() {
            Slot::Primary
        } else if me.inv.secondary.is_some() {
            Slot::Secondary
        } else {
            Slot::Melee
        };
        let carrying_bomb = me.inv.bomb && sim.bomb == BombState::Carried(me.id);
        let in_site = sim
            .level
            .def
            .bomb_sites
            .iter()
            .any(|z| z.aabb().contains(me.mv.origin + Vec3::Y));

        // --- combat
        let target = if frozen {
            None
        } else {
            self.find_target(sim, me)
        };
        match target {
            Some((id, _)) if self.target != Some(id) => {
                self.target = Some(id);
                self.seen_since = now;
                let spread = 6.0 * (1.0 - self.skill) + 1.0;
                self.aim_error = (
                    self.rng.range_f32(-spread, spread) * 0.5,
                    self.rng.range_f32(-spread, spread),
                );
            }
            None => self.target = None,
            _ => {}
        }
        if let Some((_, point)) = target {
            if me.active != wanted && me.active != Slot::Bomb
                || (me.active == Slot::Bomb && me.planting_since.is_none())
            {
                select = match wanted {
                    Slot::Primary => 1,
                    Slot::Secondary => 2,
                    _ => 3,
                };
                if me.active == wanted {
                    select = 0;
                }
            }
            // Aim error shrinks while tracking.
            let tracked = (now - self.seen_since) as f32;
            let decay = (1.0 - tracked * (1.5 + 2.5 * self.skill)).max(0.0);
            let mut aim = forward_to_angles((point - eye).normalize_or_zero());
            aim.pitch += self.aim_error.0 * decay;
            aim.yaw += self.aim_error.1 * decay;
            // Compensate the recoil like a player pulling down (imperfectly).
            if let Some(w) = me.active_weapon() {
                let (rp, ry) = me.hand.recoil_now(cat.get(w), now);
                aim.pitch -= rp * (0.5 + 0.4 * self.skill);
                aim.yaw -= ry * (0.5 + 0.4 * self.skill);
            }
            let view = self.turn_toward(me.view, aim, dt);
            let err = wrap_degrees(view.yaw - aim.yaw).abs() + (view.pitch - aim.pitch).abs();
            let reacted = now - self.seen_since >= self.reaction_time();
            let def = me.active_def(cat);
            let melee = def.is_some_and(|d| d.kind == WeaponKind::Knife);
            let mut b;
            let dist = (point - eye).length();
            if melee {
                b = Self::move_keys(view, point - eye);
                if dist < 70.0 && reacted {
                    b = b.with(Buttons::ATTACK, true);
                }
            } else {
                // Stop (counter-strafe), then shoot when accurate.
                b = Self::stop_keys(view, me.mv.velocity);
                let accurate = def.is_none_or(|d| {
                    me.mv.horizontal_speed() <= d.max_speed * d.accurate_fraction + 1.0
                });
                let sniper = def.is_some_and(|d| !d.zoom.levels.is_empty());
                if sniper && me.hand.zoom == 0 && dist > 400.0 {
                    b = b.with(Buttons::ATTACK2, !self.fire_toggle);
                }
                let semi = def.is_some_and(|d| !d.automatic);
                if reacted && accurate && err < 2.5 + 1500.0 / dist.max(100.0) {
                    let press = !semi || !self.fire_toggle;
                    b = b.with(Buttons::ATTACK, press);
                }
                self.fire_toggle = !self.fire_toggle;
            }
            return Some(self.command(sim, me, b, view, select));
        }

        // --- objectives
        if carrying_bomb && in_site && sim.rules.phase == Phase::Live && me.mv.on_ground {
            if me.active != Slot::Bomb {
                return Some(self.command(sim, me, Buttons(0), me.view, 5));
            }
            return Some(self.command(sim, me, Buttons(Buttons::ATTACK), me.view, 0));
        }
        if me.team == Team::Defense
            && let BombState::Planted { pos, .. } = sim.bomb
            && (me.mv.origin - pos).length() < sim.rules.cfg.bomb.defuse_range
        {
            return Some(self.command(sim, me, Buttons(Buttons::USE), me.view, 0));
        }
        if me.active != wanted && me.active != Slot::Bomb {
            select = match wanted {
                Slot::Primary => 1,
                Slot::Secondary => 2,
                _ => 3,
            };
        }
        if frozen {
            return Some(self.command(sim, me, Buttons(0), me.view, select));
        }

        // --- navigation
        let goal = if self.path.is_empty() || now >= self.repath_at {
            self.goal(sim, me)
        } else {
            self.path_goal
        };
        if let Some(g) = goal
            && (self.path.is_empty() || now >= self.repath_at)
        {
            self.path = sim.level.nav.path(me.mv.origin, g).unwrap_or_default();
            self.path_goal = Some(g);
            self.repath_at = now + 2.0 + self.rng.next_f32() as f64;
            if !self.path.is_empty() {
                self.path.remove(0);
            }
        }
        while let Some(n) = self.path.first() {
            let d = Vec3::new(n.x - me.mv.origin.x, 0.0, n.z - me.mv.origin.z).length();
            if d < 24.0 {
                self.path.remove(0);
            } else {
                break;
            }
        }
        // Stuck detection: random strafe for a moment.
        if (me.mv.origin - self.last_pos).length() > 24.0 {
            self.last_pos = me.mv.origin;
            self.stuck_since = now;
        }
        let stuck = now - self.stuck_since > 1.2 && !self.path.is_empty();
        if stuck && now > self.strafe_until {
            self.strafe_until = now + 0.5;
            self.strafe_dir = if self.rng.chance(0.5) { 1 } else { -1 };
            self.stuck_since = now;
            self.repath_at = now;
        }
        let Some(next) = self.path.first().copied() else {
            return Some(self.command(sim, me, Buttons(0), me.view, select));
        };
        // Look ahead along the path (a few nodes) for smooth turning.
        let look = self.path.get(3).copied().unwrap_or(next);
        let mut want = forward_to_angles((look + Vec3::Y * 64.0 - eye).normalize_or_zero());
        want.pitch = want.pitch.clamp(-10.0, 10.0);
        let view = self.turn_toward(me.view, want, dt);
        let mut b = Self::move_keys(view, next - me.mv.origin);
        if now < self.strafe_until {
            b = b
                .with(
                    if self.strafe_dir > 0 {
                        Buttons::RIGHT
                    } else {
                        Buttons::LEFT
                    },
                    true,
                )
                .with(Buttons::JUMP, true);
        }
        // Jump up ledges higher than a step.
        if next.y - me.mv.origin.y > 20.0 {
            b = b.with(Buttons::JUMP, true);
        }
        // Reload when calm.
        if let Some((_, g)) = me
            .inv
            .primary
            .as_ref()
            .filter(|_| me.active == Slot::Primary)
            && let Some(w) = me.active_def(cat)
            && g.mag < w.mag_size / 2
            && g.reserve > 0
        {
            b = b.with(Buttons::RELOAD, true);
        }
        Some(self.command(sim, me, b, view, select))
    }
}

/// Bot purchases at the start of a round (simple economy logic).
pub fn bot_buy(sim: &mut Sim, id: PlayerId) {
    let Some(p) = sim.player(id) else { return };
    if !sim.rules.cfg.economy.enabled {
        return;
    }
    let team = p.team;
    let money = p.money;
    let has_primary = p.inv.primary.is_some();
    let mut list: Vec<&str> = Vec::new();
    if !has_primary {
        if money >= 4700 + 1000 + 1500 && id.0.is_multiple_of(5) {
            list.push("sniper_heavy");
        } else if money >= 2700 + 650 {
            list.push("rifle");
        } else if (1250 + 650..2700).contains(&money) {
            list.push("smg");
        }
    }
    list.push("kevlar_helmet");
    list.push("kevlar");
    if team == Team::Defense {
        list.push("defuse_kit");
    }
    list.push("smoke");
    list.push("flash");
    list.push("he");
    for item in list {
        let _ = sim.buy(id, item);
    }
}
