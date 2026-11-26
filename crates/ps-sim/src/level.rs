//! A loaded map: collision world and navigation graph.

use engine_movement::MovementConfig;
use engine_movement::controller::hull_for;
use engine_physics::map::MapDef;
use engine_physics::material::MaterialTable;
use engine_physics::world::TraceMask;
use engine_physics::{CollisionWorld, MapError};
use glam::Vec3;
use std::collections::{BinaryHeap, HashMap, VecDeque};

pub struct Level {
    pub def: MapDef,
    pub world: CollisionWorld,
    pub nav: NavGraph,
}

impl Level {
    pub fn build(
        def: MapDef,
        mats: &MaterialTable,
        mv: &MovementConfig,
    ) -> Result<Level, MapError> {
        let brushes = def.build_brushes(mats)?;
        let world = CollisionWorld::new(brushes);
        let (mins, maxs) = hull_for(mv, false);
        for (i, s) in def.spawns.iter().chain(def.dm_spawns.iter()).enumerate() {
            if world.box_in_solid(s.pos, mins, maxs) {
                return Err(MapError::SpawnInSolid(i, s.pos));
            }
        }
        let seeds: Vec<Vec3> = def
            .spawns
            .iter()
            .chain(def.dm_spawns.iter())
            .map(|s| s.pos)
            .collect();
        let nav = NavGraph::build(&world, &def, mv, &seeds);
        Ok(Level { def, world, nav })
    }
}

/// Walkable grid graph built by flood fill from the spawns (so it follows the real
/// reachable floor, including under ceilings and on platforms).
#[derive(Debug, Clone, Default)]
pub struct NavGraph {
    pub cell: f32,
    pub nodes: Vec<Vec3>,
    pub edges: Vec<Vec<(u32, f32)>>,
    index: HashMap<(i32, i32, i32), u32>,
    columns: HashMap<(i32, i32), Vec<u32>>,
}

pub const NAV_CELL: f32 = 32.0;

impl NavGraph {
    pub fn build(
        world: &CollisionWorld,
        def: &MapDef,
        mv: &MovementConfig,
        seeds: &[Vec3],
    ) -> NavGraph {
        let mut g = NavGraph {
            cell: NAV_CELL,
            ..Default::default()
        };
        let (mins, maxs) = hull_for(mv, false);
        // Slightly thinner hull: nodes may sit close to walls.
        let mins = mins + Vec3::new(4.0, 0.0, 4.0);
        let maxs = maxs - Vec3::new(4.0, 0.0, 4.0);
        let bounds = def.bounds();
        let mut queue = VecDeque::new();
        let drop = |x: f32, y: f32, z: f32| -> Option<Vec3> {
            let start = Vec3::new(x, y, z);
            if world.box_in_solid(start, mins, maxs) {
                return None;
            }
            let tr = world.trace_box(
                start,
                start - Vec3::Y * 80.0,
                mins,
                maxs,
                TraceMask::Players,
            );
            if tr.fraction >= 1.0 || tr.normal.y < mv.walkable_normal {
                return None;
            }
            Some(tr.end)
        };
        for s in seeds {
            let (i, j) = g.key_xz(*s);
            let c = g.center(i, j);
            if let Some(p) = drop(c.x, s.y + 2.0, c.z) {
                g.add(p, &mut queue);
            }
        }
        let max_nodes = 200_000;
        while let Some(id) = queue.pop_front() {
            if g.nodes.len() > max_nodes {
                break;
            }
            let p = g.nodes[id as usize];
            let (i, j) = g.key_xz(p);
            for (di, dj) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let c = g.center(i + di, j + dj);
                if !bounds.contains(Vec3::new(c.x, bounds.center().y, c.z)) {
                    continue;
                }
                let Some(q) = drop(c.x, p.y + mv.step_size + 1.0, c.z) else {
                    continue;
                };
                if (q.y - p.y).abs() > mv.step_size + 1.0 && q.y > p.y {
                    continue;
                }
                // Falls of more than 64 units are one-way: skip them for bots.
                if p.y - q.y > 64.0 {
                    continue;
                }
                // Walkable connection: horizontal trace at step height.
                let lift = Vec3::Y * (mv.step_size + 1.0);
                let a = p + lift;
                let b = Vec3::new(q.x, a.y.max(q.y + 1.0), q.z);
                if world
                    .trace_box(a, b, mins, maxs, TraceMask::Players)
                    .fraction
                    < 1.0
                {
                    continue;
                }
                let qid = g.add(q, &mut queue);
                let cost = p.distance(q);
                if !g.edges[id as usize].iter().any(|e| e.0 == qid) {
                    g.edges[id as usize].push((qid, cost));
                }
            }
        }
        g
    }

    fn key_xz(&self, p: Vec3) -> (i32, i32) {
        (
            (p.x / self.cell).floor() as i32,
            (p.z / self.cell).floor() as i32,
        )
    }

    fn center(&self, i: i32, j: i32) -> Vec3 {
        Vec3::new(
            (i as f32 + 0.5) * self.cell,
            0.0,
            (j as f32 + 0.5) * self.cell,
        )
    }

    fn add(&mut self, p: Vec3, queue: &mut VecDeque<u32>) -> u32 {
        let (i, j) = self.key_xz(p);
        let k = (i, j, (p.y / 24.0).round() as i32);
        if let Some(&id) = self.index.get(&k) {
            return id;
        }
        let id = self.nodes.len() as u32;
        self.nodes.push(p);
        self.edges.push(Vec::new());
        self.index.insert(k, id);
        self.columns.entry((i, j)).or_default().push(id);
        queue.push_back(id);
        id
    }

    /// Node closest (vertically) to `p` in its column or a neighbour column.
    pub fn nearest(&self, p: Vec3) -> Option<u32> {
        let (i, j) = self.key_xz(p);
        let mut best: Option<(f32, u32)> = None;
        for r in 0i32..4 {
            for di in -r..=r {
                for dj in -r..=r {
                    if di.abs() != r && dj.abs() != r {
                        continue;
                    }
                    if let Some(ids) = self.columns.get(&(i + di, j + dj)) {
                        for &id in ids {
                            let n = self.nodes[id as usize];
                            let d = n.distance_squared(p) + (n.y - p.y).powi(2) * 4.0;
                            if best.is_none_or(|b| d < b.0) {
                                best = Some((d, id));
                            }
                        }
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        best.map(|b| b.1)
    }

    /// A* path between two positions, as a list of node positions.
    pub fn path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let s = self.nearest(from)?;
        let t = self.nearest(to)?;
        let goal = self.nodes[t as usize];
        #[derive(PartialEq)]
        struct Item(f32, u32);
        impl Eq for Item {}
        impl Ord for Item {
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
            }
        }
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        let mut g_cost: HashMap<u32, f32> = HashMap::new();
        let mut came: HashMap<u32, u32> = HashMap::new();
        let mut open = BinaryHeap::new();
        g_cost.insert(s, 0.0);
        open.push(Item(self.nodes[s as usize].distance(goal), s));
        let mut expanded = 0;
        while let Some(Item(_, n)) = open.pop() {
            if n == t {
                let mut out = vec![self.nodes[n as usize]];
                let mut c = n;
                while let Some(&p) = came.get(&c) {
                    out.push(self.nodes[p as usize]);
                    c = p;
                }
                out.reverse();
                return Some(out);
            }
            expanded += 1;
            if expanded > 60_000 {
                return None;
            }
            let gn = g_cost[&n];
            for &(m, w) in &self.edges[n as usize] {
                let ng = gn + w;
                if g_cost.get(&m).is_none_or(|&old| ng < old) {
                    g_cost.insert(m, ng);
                    came.insert(m, n);
                    open.push(Item(ng + self.nodes[m as usize].distance(goal), m));
                }
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
