//! Static collision world with a uniform grid broadphase.

use crate::brush::Brush;
use crate::material::MaterialId;
use engine_core::Aabb;
use glam::Vec3;

/// Result of a swept box or ray trace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trace {
    /// Portion of the move completed, in [0, 1].
    pub fraction: f32,
    pub end: Vec3,
    pub normal: Vec3,
    pub start_solid: bool,
    pub all_solid: bool,
    /// Brush index that stopped the trace.
    pub brush: Option<u32>,
}

impl Trace {
    pub fn hit(&self) -> bool {
        self.fraction < 1.0 || self.start_solid
    }

    pub fn clear(end: Vec3) -> Trace {
        Trace {
            fraction: 1.0,
            end,
            normal: Vec3::ZERO,
            start_solid: false,
            all_solid: false,
            brush: None,
        }
    }
}

/// One crossed solid along a ray, for penetration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaySegment {
    pub t_enter: f32,
    pub t_exit: f32,
    pub normal: Vec3,
    pub material: MaterialId,
    pub brush: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceMask {
    /// Player movement: everything that blocks players.
    Players,
    /// Bullets, grenades and line of sight.
    Bullets,
}

const CELL: f32 = 256.0;

#[derive(Debug, Clone)]
pub struct CollisionWorld {
    pub brushes: Vec<Brush>,
    origin: Vec3,
    dims: (usize, usize),
    cells: Vec<Vec<u32>>,
    bounds: Aabb,
}

impl CollisionWorld {
    pub fn new(brushes: Vec<Brush>) -> Self {
        let mut bounds = brushes
            .first()
            .map(|b| b.bounds)
            .unwrap_or(Aabb::new(Vec3::ZERO, Vec3::ONE));
        for b in &brushes {
            bounds = bounds.union(&b.bounds);
        }
        let origin = bounds.min;
        let size = bounds.size();
        let nx = ((size.x / CELL).ceil() as usize).max(1);
        let nz = ((size.z / CELL).ceil() as usize).max(1);
        let mut cells = vec![Vec::new(); nx * nz];
        for (i, b) in brushes.iter().enumerate() {
            let (x0, z0, x1, z1) = Self::cell_range_of(origin, (nx, nz), &b.bounds);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    cells[z * nx + x].push(i as u32);
                }
            }
        }
        Self {
            brushes,
            origin,
            dims: (nx, nz),
            cells,
            bounds,
        }
    }

    pub fn bounds(&self) -> Aabb {
        self.bounds
    }

    fn cell_range_of(origin: Vec3, dims: (usize, usize), a: &Aabb) -> (usize, usize, usize, usize) {
        let c = |v: f32, o: f32, n: usize| (((v - o) / CELL).floor().max(0.0) as usize).min(n - 1);
        (
            c(a.min.x, origin.x, dims.0),
            c(a.min.z, origin.z, dims.1),
            c(a.max.x, origin.x, dims.0),
            c(a.max.z, origin.z, dims.1),
        )
    }

    /// Calls `f` once for every brush whose grid cells overlap `area`.
    fn for_candidates(&self, area: &Aabb, mut f: impl FnMut(u32, &Brush)) {
        if !area.intersects(&self.bounds) {
            return;
        }
        let (x0, z0, x1, z1) = Self::cell_range_of(self.origin, self.dims, area);
        // Brushes spanning several cells must be visited once: small stack bitmap.
        let mut seen_small = [0u64; 32];
        let mut seen_big: Vec<u32> = Vec::new();
        let n = self.brushes.len();
        for z in z0..=z1 {
            for x in x0..=x1 {
                for &bi in &self.cells[z * self.dims.0 + x] {
                    if n <= 2048 {
                        let (w, bit) = ((bi / 64) as usize, bi % 64);
                        if seen_small[w] & (1 << bit) != 0 {
                            continue;
                        }
                        seen_small[w] |= 1 << bit;
                    } else if seen_big.contains(&bi) {
                        continue;
                    } else {
                        seen_big.push(bi);
                    }
                    let b = &self.brushes[bi as usize];
                    if b.bounds.intersects(area) {
                        f(bi, b);
                    }
                }
            }
        }
    }

    fn mask_ok(b: &Brush, mask: TraceMask) -> bool {
        match mask {
            TraceMask::Players => b.blocks_players,
            TraceMask::Bullets => b.blocks_bullets,
        }
    }

    /// Swept box trace. `mins`/`maxs` are relative to the moving origin.
    pub fn trace_box(
        &self,
        start: Vec3,
        end: Vec3,
        mins: Vec3,
        maxs: Vec3,
        mask: TraceMask,
    ) -> Trace {
        let sweep = Aabb::from_corners(start, end);
        let area = Aabb::new(sweep.min + mins - Vec3::ONE, sweep.max + maxs + Vec3::ONE);
        let mut best = Trace::clear(end);
        self.for_candidates(&area, |bi, b| {
            if !Self::mask_ok(b, mask) {
                return;
            }
            if let Some(h) = b.trace(start, end, mins, maxs) {
                if h.start_solid {
                    best.start_solid = true;
                    if h.all_solid {
                        best.all_solid = true;
                        best.fraction = 0.0;
                        best.brush = Some(bi);
                    }
                } else if h.fraction < best.fraction {
                    best.fraction = h.fraction;
                    best.normal = h.normal;
                    best.brush = Some(bi);
                }
            }
        });
        best.end = start + (end - start) * best.fraction;
        best
    }

    pub fn trace_ray(&self, start: Vec3, end: Vec3, mask: TraceMask) -> Trace {
        self.trace_box(start, end, Vec3::ZERO, Vec3::ZERO, mask)
    }

    /// True if nothing that blocks bullets lies between `a` and `b`.
    pub fn line_of_sight(&self, a: Vec3, b: Vec3) -> bool {
        !self.trace_ray(a, b, TraceMask::Bullets).hit()
    }

    /// Every bullet-blocking solid crossed by the segment, sorted by entry.
    pub fn ray_segments(&self, origin: Vec3, dir: Vec3, max_dist: f32, out: &mut Vec<RaySegment>) {
        out.clear();
        let d = dir.normalize_or_zero();
        let area = Aabb::from_corners(origin, origin + d * max_dist).expanded(Vec3::ONE);
        self.for_candidates(&area, |bi, b| {
            if !b.blocks_bullets {
                return;
            }
            if let Some((t0, t1, n)) = b.ray_interval(origin, d, max_dist)
                && t1 > t0
            {
                out.push(RaySegment {
                    t_enter: t0,
                    t_exit: t1,
                    normal: n,
                    material: b.material,
                    brush: bi,
                });
            }
        });
        out.sort_by(|a, b| a.t_enter.total_cmp(&b.t_enter).then(a.brush.cmp(&b.brush)));
    }

    /// True if a box at `pos` overlaps player-blocking solids.
    pub fn box_in_solid(&self, pos: Vec3, mins: Vec3, maxs: Vec3) -> bool {
        self.trace_box(pos, pos, mins, maxs, TraceMask::Players)
            .start_solid
    }

    pub fn point_in_solid(&self, p: Vec3) -> bool {
        let mut solid = false;
        self.for_candidates(&Aabb::new(p, p), |_, b| {
            if b.blocks_bullets && b.contains_point(p) {
                solid = true;
            }
        });
        solid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> CollisionWorld {
        let floor = Brush::from_box(
            Aabb::new(
                Vec3::new(-1000.0, -16.0, -1000.0),
                Vec3::new(1000.0, 0.0, 1000.0),
            ),
            MaterialId(0),
        );
        let wall = Brush::from_box(
            Aabb::new(
                Vec3::new(100.0, 0.0, -500.0),
                Vec3::new(108.0, 128.0, 500.0),
            ),
            MaterialId(1),
        );
        let wall2 = Brush::from_box(
            Aabb::new(
                Vec3::new(300.0, 0.0, -500.0),
                Vec3::new(316.0, 128.0, 500.0),
            ),
            MaterialId(2),
        );
        CollisionWorld::new(vec![floor, wall, wall2])
    }

    #[test]
    fn floor_and_walls() {
        let w = world();
        let t = w.trace_box(
            Vec3::new(0.0, 50.0, 0.0),
            Vec3::new(0.0, -50.0, 0.0),
            Vec3::new(-16.0, 0.0, -16.0),
            Vec3::new(16.0, 72.0, 16.0),
            TraceMask::Players,
        );
        assert!(
            t.hit() && (t.end.y - 0.03125).abs() < 1e-3 && t.normal == Vec3::Y,
            "{t:?}"
        );
        assert!(!w.line_of_sight(Vec3::new(0.0, 64.0, 0.0), Vec3::new(200.0, 64.0, 0.0)));
        assert!(w.line_of_sight(Vec3::new(0.0, 64.0, 0.0), Vec3::new(90.0, 64.0, 0.0)));
    }

    #[test]
    fn ray_segments_sorted_with_thickness() {
        let w = world();
        let mut segs = Vec::new();
        w.ray_segments(Vec3::new(0.0, 64.0, 0.0), Vec3::X, 1000.0, &mut segs);
        assert_eq!(segs.len(), 2);
        assert!((segs[0].t_exit - segs[0].t_enter - 8.0).abs() < 1e-4);
        assert!((segs[1].t_exit - segs[1].t_enter - 16.0).abs() < 1e-4);
        assert_eq!(segs[1].material, MaterialId(2));
    }

    #[test]
    fn grid_dedup_large_brush() {
        // The floor spans many cells: a downward trace must still see it once and hit.
        let w = world();
        for x in [-900.0, -300.0, 0.0, 450.0, 900.0] {
            let t = w.trace_ray(
                Vec3::new(x, 10.0, x),
                Vec3::new(x, -10.0, x),
                TraceMask::Bullets,
            );
            assert!(t.hit());
        }
    }
}
