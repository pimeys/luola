//! Level validation that is not just shape checking: flood-fills the open space,
//! proves the cave is sealed, every objective is reachable, and estimates the
//! flight plan against the level's fuel budget.
//!
//! A cave flyer's levels are negative space, so the failure modes that matter are
//! a chamber that cannot be flown into, a hole in the wall that lets the ship out
//! of the level, and a cave so long that the return leg cannot be fuelled — the
//! genre's classic "you won the objective and cannot leave" trap
//! (`docs/game_mechanics.md` §8.3).

use crate::math::{Rect, V2};
use crate::sim::level::Level;

pub const CELL: f32 = 8.0;
const UNREACHED: i32 = -1;

#[derive(Clone, Debug)]
pub struct Reachability {
    pub cell: f32,
    pub open_cells: usize,
    pub reachable_cells: usize,
    /// True when the flood fill never touched the bounding box border.
    pub sealed: bool,
    /// Reachable area in world pixels squared.
    pub area: f32,
    /// Objectives the ship cannot reach from its spawn.
    pub unreachable: Vec<String>,
    /// Shortest route from the spawn to the nearest exit, in world pixels.
    pub route_px: Option<f32>,
    /// Shortest route for the mission: spawn to the objective and on to the exit.
    /// Falls back to `route_px` on levels with nothing to collect.
    pub plan_px: Option<f32>,
    /// True when the player spawn itself is buried in rock.
    pub spawn_buried: bool,
    /// Sealed pockets of air, largest first: the holes in the map.
    pub pockets: Vec<Pocket>,
}

impl Default for Reachability {
    fn default() -> Self {
        Self {
            cell: CELL,
            open_cells: 0,
            reachable_cells: 0,
            sealed: false,
            area: 0.0,
            unreachable: Vec::new(),
            route_px: None,
            plan_px: None,
            spawn_buried: false,
            pockets: Vec::new(),
        }
    }
}

impl Reachability {
    pub fn reachable_fraction(&self) -> f32 {
        if self.open_cells == 0 {
            0.0
        } else {
            self.reachable_cells as f32 / self.open_cells as f32
        }
    }

    pub fn ok(&self) -> bool {
        self.sealed && !self.spawn_buried && self.unreachable.is_empty()
    }

    /// Path length the mission demands, in pixels.
    pub fn mission_distance(&self) -> Option<f32> {
        self.plan_px.or(self.route_px)
    }

    pub fn describe(&self) -> String {
        format!(
            "open {} cells, reachable {} ({:.0}%), area {:.0} px2, route {}, plan {}, sealed {}, unreachable [{}]",
            self.open_cells,
            self.reachable_cells,
            self.reachable_fraction() * 100.0,
            self.area,
            fmt_dist(self.route_px),
            fmt_dist(self.plan_px),
            if self.sealed { "yes" } else { "NO" },
            self.unreachable.join(", ")
        )
    }
}

fn fmt_dist(d: Option<f32>) -> String {
    d.map(|d| format!("{d:.0}px"))
        .unwrap_or_else(|| "none".to_string())
}

/// Fuel a cautious pilot burns flying `px` of cave, in fuel units.
///
/// Model: 130 px/s cruise, half of the time on thrust (climbing and stopping
/// cost as much as going), plus a 2x allowance for approach mistakes and
/// hovering. Levels must satisfy this with fuel to spare.
pub fn fuel_for_distance(px: f32) -> f32 {
    const CRUISE_PX_S: f32 = 130.0;
    const THRUST_DUTY: f32 = 0.5;
    const MISTAKE_ALLOWANCE: f32 = 2.0;
    px / CRUISE_PX_S * THRUST_DUTY * MISTAKE_ALLOWANCE * crate::sim::tuning::FUEL_BURN
}

/// Fuel available over a whole mission: the tank plus most of what the pods on
/// the route are worth. Pods are discounted because a cautious pilot will not
/// route through all of them.
pub fn fuel_budget(level: &Level) -> f32 {
    const PICKUP_CREDIT: f32 = 0.6;
    let pods: f32 = level.fuel_pods.iter().map(|f| f.amount).sum();
    level.start_fuel + pods * PICKUP_CREDIT
}

/// A grid of the cave's open space plus breadth-first distances.
struct Grid {
    cols: i32,
    rows: i32,
    bounds: Rect,
    open: Vec<bool>,
}

impl Grid {
    fn new(level: &Level) -> Self {
        // The grid covers the terrain's own bounding box, not the inflated
        // playfield: the margin around the cave is open space that is not part of
        // the cave, and counting it would both inflate "open cells" and mask a
        // hole in a wall.
        let bounds = level.terrain.bounds;
        let cols = (bounds.w / CELL).ceil() as i32;
        let rows = (bounds.h / CELL).ceil() as i32;
        let mut open = vec![false; (cols.max(0) * rows.max(0)) as usize];
        for r in 0..rows {
            for c in 0..cols {
                open[(r * cols + c) as usize] = !level.terrain.point_solid(centre(bounds, c, r));
            }
        }
        Self {
            cols,
            rows,
            bounds,
            open,
        }
    }

    fn index(&self, c: i32, r: i32) -> usize {
        (r * self.cols + c) as usize
    }

    fn inside(&self, c: i32, r: i32) -> bool {
        c >= 0 && r >= 0 && c < self.cols && r < self.rows
    }

    fn cell_of(&self, p: V2) -> (i32, i32) {
        (
            ((p.x - self.bounds.x) / CELL).floor() as i32,
            ((p.y - self.bounds.y) / CELL).floor() as i32,
        )
    }

    /// Breadth-first distances in cells from `seeds`; `UNREACHED` elsewhere.
    fn distances(&self, seeds: &[(i32, i32)]) -> (Vec<i32>, usize, bool) {
        let mut dist = vec![UNREACHED; self.open.len()];
        let mut queue = std::collections::VecDeque::new();
        let mut reached = 0usize;
        let mut touches_border = false;
        for &(c, r) in seeds {
            if !self.inside(c, r) || !self.open[self.index(c, r)] {
                continue;
            }
            let i = self.index(c, r);
            if dist[i] == UNREACHED {
                dist[i] = 0;
                reached += 1;
                queue.push_back((c, r));
            }
        }
        while let Some((c, r)) = queue.pop_front() {
            let d = dist[self.index(c, r)];
            if c == 0 || r == 0 || c == self.cols - 1 || r == self.rows - 1 {
                touches_border = true;
            }
            for (dc, dr) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nc, nr) = (c + dc, r + dr);
                if !self.inside(nc, nr) {
                    continue;
                }
                let i = self.index(nc, nr);
                if self.open[i] && dist[i] == UNREACHED {
                    dist[i] = d + 1;
                    reached += 1;
                    queue.push_back((nc, nr));
                }
            }
        }
        (dist, reached, touches_border)
    }

    fn cells_in(&self, rect: &Rect) -> Vec<(i32, i32)> {
        let (c0, r0) = self.cell_of(V2::new(rect.left(), rect.top()));
        let (c1, r1) = self.cell_of(V2::new(rect.right(), rect.bottom()));
        let mut out = Vec::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                if self.inside(c, r) {
                    out.push((c, r));
                }
            }
        }
        out
    }

    fn cell_at(&self, p: V2) -> Option<(i32, i32)> {
        let (c, r) = self.cell_of(p);
        self.inside(c, r).then_some((c, r))
    }

    /// Minimum distance from `source` over a set of cells, in world pixels.
    fn min_distance(&self, dist: &[i32], cells: &[(i32, i32)]) -> Option<f32> {
        cells
            .iter()
            .filter(|(c, r)| self.inside(*c, *r) && dist[self.index(*c, *r)] != UNREACHED)
            .map(|(c, r)| dist[self.index(*c, *r)] as f32 * CELL)
            .fold(None, |acc: Option<f32>, d| {
                Some(acc.map(|a| a.min(d)).unwrap_or(d))
            })
    }
}

fn centre(bounds: Rect, c: i32, r: i32) -> V2 {
    V2::new(
        bounds.x + (c as f32 + 0.5) * CELL,
        bounds.y + (r as f32 + 0.5) * CELL,
    )
}

/// A body of air the ship cannot get to: a hole in the map, or a sealed pocket.
///
/// Reported so an author can go and look at it. A cave authored from discs
/// collects these where a mass leaves air trapped under its flanks, which is
/// invisible on the map until you know where to look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pocket {
    /// Open cells in it.
    pub cells: usize,
    /// Where it is, in world pixels.
    pub rect: Rect,
}

/// The four largest sealed pockets, biggest first.
fn pockets(grid: &Grid, dist: &[i32]) -> Vec<Pocket> {
    let mut seen = vec![false; grid.open.len()];
    let mut out: Vec<Pocket> = Vec::new();
    for r in 0..grid.rows {
        for c in 0..grid.cols {
            let i = grid.index(c, r);
            if !grid.open[i] || dist[i] != UNREACHED || seen[i] {
                continue;
            }
            let mut queue = std::collections::VecDeque::from([(c, r)]);
            seen[i] = true;
            let (mut x0, mut y0, mut x1, mut y1) = (c, r, c, r);
            let mut cells = 0usize;
            while let Some((cc, rr)) = queue.pop_front() {
                cells += 1;
                x0 = x0.min(cc);
                y0 = y0.min(rr);
                x1 = x1.max(cc);
                y1 = y1.max(rr);
                for (dc, dr) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nc, nr) = (cc + dc, rr + dr);
                    if !grid.inside(nc, nr) {
                        continue;
                    }
                    let ni = grid.index(nc, nr);
                    if grid.open[ni] && dist[ni] == UNREACHED && !seen[ni] {
                        seen[ni] = true;
                        queue.push_back((nc, nr));
                    }
                }
            }
            out.push(Pocket {
                cells,
                rect: Rect::new(
                    grid.bounds.x + x0 as f32 * CELL,
                    grid.bounds.y + y0 as f32 * CELL,
                    (x1 - x0 + 1) as f32 * CELL,
                    (y1 - y0 + 1) as f32 * CELL,
                ),
            });
        }
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.cells));
    out.truncate(4);
    out
}

pub fn flood(level: &Level) -> Reachability {
    let grid = Grid::new(level);
    if grid.cols <= 0 || grid.rows <= 0 {
        return Reachability::default();
    }

    let spawn_cell = grid.cell_at(level.player.pos);
    let spawn_buried = match spawn_cell {
        Some((c, r)) => !grid.open[grid.index(c, r)],
        None => true,
    };
    let (dist_spawn, reachable_cells, sealed) = match spawn_cell {
        Some(cell) if !spawn_buried => {
            let (dist, reached, border) = grid.distances(&[cell]);
            (dist, reached, !border)
        }
        _ => (vec![UNREACHED; grid.open.len()], 0, true),
    };
    let open_cells = grid.open.iter().filter(|o| **o).count();

    let reachable_at = |p: V2| -> bool {
        grid.cell_at(p)
            .map(|(c, r)| dist_spawn[grid.index(c, r)] != UNREACHED)
            .unwrap_or(false)
    };
    let reachable_rect = |rect: &Rect| -> bool {
        grid.cells_in(rect)
            .iter()
            .any(|(c, r)| dist_spawn[grid.index(*c, *r)] != UNREACHED)
    };

    let mut unreachable = Vec::new();
    for (i, pod) in level.pods.iter().enumerate() {
        if !reachable_at(pod.pos) {
            unreachable.push(format!("pod #{i}"));
        }
    }
    for (i, reactor) in level.reactors.iter().enumerate() {
        if !reachable_at(reactor.pos) {
            unreachable.push(format!("reactor #{i}"));
        }
    }
    for (i, fuel) in level.fuel_pods.iter().enumerate() {
        if !reachable_at(fuel.pos) {
            unreachable.push(format!("fuel #{i}"));
        }
    }
    for (i, exit) in level.exits.iter().enumerate() {
        if !reachable_rect(&exit.rect) {
            unreachable.push(format!("exit #{i}"));
        }
    }
    for (i, pad) in level.pads.iter().enumerate() {
        if !reachable_rect(&pad.rect) {
            unreachable.push(format!("pad #{i}"));
        }
    }
    if level.require_pod && level.pods.is_empty() {
        unreachable.push("required pod missing".to_string());
    }

    // Exit route, and the full mission route: spawn to the objective, then on to
    // the exit. The second leg is measured with a flood fill from the exit, so a
    // level whose objective is behind the exit is not charged twice for it.
    let exit_cells: Vec<(i32, i32)> = level
        .exits
        .iter()
        .flat_map(|e| grid.cells_in(&e.rect))
        .collect();
    let route_px = grid.min_distance(&dist_spawn, &exit_cells);
    let objective_cells: Vec<(i32, i32)> = if level.require_pod && !level.pods.is_empty() {
        level
            .pods
            .iter()
            .filter_map(|p| grid.cell_at(p.pos))
            .collect()
    } else {
        level
            .reactors
            .iter()
            .filter_map(|r| grid.cell_at(r.pos))
            .collect()
    };
    let plan_px = if objective_cells.is_empty() {
        route_px
    } else {
        let (dist_exit, _, _) = grid.distances(&exit_cells);
        let outbound = grid.min_distance(&dist_spawn, &objective_cells);
        let inbound = grid.min_distance(&dist_exit, &objective_cells);
        match (outbound, inbound) {
            (Some(a), Some(b)) => Some(a + b),
            _ => route_px,
        }
    };

    Reachability {
        cell: CELL,
        open_cells,
        reachable_cells,
        sealed,
        area: reachable_cells as f32 * CELL * CELL,
        unreachable,
        route_px,
        plan_px,
        spawn_buried,
        pockets: pockets(&grid, &dist_spawn),
    }
}
