//! Water: a mass of water per terrain cell, flowing down and levelling out.
//!
//! Water shares the terrain grid, so one cell can hold both a material and up to
//! [`FULL`] units of water. A pool is seeded from the level's authored polygons
//! and then lives its own life: blast away the floor under it and it pours
//! through the hole (`docs/game_mechanics.md` §2 lists destructible, fluid-filled
//! caves as the genre's own trick — AUTS and Gravity Force 2 both let you open
//! one).
//!
//! The simulation is integer-only and order-fixed, on purpose:
//!
//! * Every transfer is a whole number of mass units, so the run is bit-exact
//!   whatever the machine (see `docs/design.md` on replay verification).
//! * Cells are visited bottom-up, serpentine per row, and only cells inside
//!   *active* tiles are visited at all. A settled pool is therefore free: the
//!   step loop walks the tile flags and touches nothing else.
//! * There is no pressure model (see the non-goals in `docs/design.md`): water
//!   falls, spreads sideways and levels out, but it will not push itself up a
//!   pipe. That is the behaviour the genre shipped with, and it keeps the model
//!   free of the iteration-to-convergence that would make the tick cost vary.
//!
//! Each rule moves at most [`FALL`] units per tick, so a surge takes a moment to
//! settle instead of teleporting — the water *pours*, which is the whole point.

use crate::math::{Rect, V2};
use crate::sim::fnv::Fnv;
use crate::sim::terrain::{Terrain, scanline_cells};

/// Water units that fill a cell.
pub const FULL: u8 = 8;
/// Mass units that may drop into one cell per tick.
const FALL: u8 = 8;
/// Mass units that may level out between neighbouring cells per tick.
///
/// Same order of magnitude as `FALL`, on purpose: a pool that is opened at one
/// end has to level out in seconds, not minutes, and the half-the-difference cap
/// below means a fast rate can never overshoot into an oscillation.
const SPREAD: u8 = 8;
/// Activity is tracked per tile, so a settled pool costs nothing.
const TILE: i32 = 32;
/// Tiles visited per tick, worst case. A surge bigger than this flows over
/// several ticks instead of stalling the frame; which tiles wait is decided by
/// tile index, so the result is still deterministic.
const MAX_ACTIVE_TILES: usize = 256;

/// How water feels to a body inside it: gravity scaling and extra drag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterParams {
    pub density: f32,
    pub drag: f32,
}

impl Default for WaterParams {
    fn default() -> Self {
        Self {
            density: crate::sim::tuning::WATER_DENSITY,
            drag: crate::sim::tuning::WATER_DRAG,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Water {
    bounds: Rect,
    w: i32,
    h: i32,
    mass: Vec<u8>,
    tiles_x: i32,
    tiles_y: i32,
    active: Vec<bool>,
    next: Vec<bool>,
    tile_sig: Vec<u64>,
    /// Water in each column, in mass units. Sideways flow compares these: a
    /// purely local rule lets every column keep one unit more than its neighbour
    /// and that accumulates into a slope you can see across a wide pool, while
    /// two columns with equal totals level out at the same height.
    col_mass: Vec<u32>,
    params: WaterParams,
    /// Cells that received or lost water on the last step.
    moved: u32,
}

impl Water {
    /// An empty body of water on the same grid as `terrain`.
    pub fn new(terrain: &Terrain, params: WaterParams) -> Self {
        let w = terrain.width();
        let h = terrain.height();
        let tiles_x = ((w + TILE - 1) / TILE).max(1);
        let tiles_y = ((h + TILE - 1) / TILE).max(1);
        let tiles = (tiles_x as usize) * (tiles_y as usize);
        let mut water = Water {
            bounds: terrain.bounds,
            w,
            h,
            mass: vec![0; (w as usize) * (h as usize)],
            tiles_x,
            tiles_y,
            active: vec![false; tiles],
            next: vec![false; tiles],
            tile_sig: vec![0; tiles],
            col_mass: vec![0; w as usize],
            params,
            moved: 0,
        };
        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                water.rehash_tile(tx, ty);
            }
        }
        water
    }

    pub fn params(&self) -> WaterParams {
        self.params
    }

    /// True when no tile has work queued, now or on the next tick.
    pub fn is_settled(&self) -> bool {
        !self.active.iter().any(|a| *a) && !self.next.iter().any(|a| *a)
    }

    /// Cells that moved on the last step.
    pub fn moved(&self) -> u32 {
        self.moved
    }

    /// Total water in the level, in mass units.
    pub fn total(&self) -> u64 {
        self.mass.iter().map(|m| *m as u64).sum()
    }

    /// Water units in a cell, in *world* integer coordinates; zero outside.
    pub fn mass(&self, wx: i32, wy: i32) -> u8 {
        let (ix, iy) = self.to_index(wx, wy);
        match self.index(ix, iy) {
            Some(i) => self.mass[i],
            None => 0,
        }
    }

    pub fn mass_at(&self, p: V2) -> u8 {
        let (ix, iy) = self.cell_of(p);
        self.mass_index(ix, iy)
    }

    /// How submerged a point is, `0.0..=1.0`.
    pub fn submerged_at(&self, p: V2) -> f32 {
        self.mass_at(p) as f32 / FULL as f32
    }

    /// True when the cell holds water with air above it: the surface line.
    pub fn surface(&self, wx: i32, wy: i32) -> bool {
        self.mass(wx, wy) > 0 && self.mass(wx, wy - 1) == 0
    }

    /// Fills the cells inside `poly` that are not solid rock.
    pub fn fill(&mut self, terrain: &Terrain, poly: &[V2]) {
        let origin = V2::new(self.bounds.x, self.bounds.y);
        let w = self.w;
        let h = self.h;
        let mut tx0 = i32::MAX;
        let mut ty0 = i32::MAX;
        let mut tx1 = i32::MIN;
        let mut ty1 = i32::MIN;
        scanline_cells(poly, origin, w, h, &mut |x, y| {
            if terrain.solid_index(x, y) {
                return;
            }
            let i = (y as usize) * (w as usize) + x as usize;
            if self.mass[i] != FULL {
                self.col_mass[x as usize] += FULL as u32 - self.mass[i] as u32;
                self.mass[i] = FULL;
            }
            tx0 = tx0.min(x / TILE);
            ty0 = ty0.min(y / TILE);
            tx1 = tx1.max(x / TILE);
            ty1 = ty1.max(y / TILE);
        });
        if tx0 > tx1 {
            return;
        }
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                self.rehash_tile(tx, ty);
            }
        }
        // A freshly filled pool is already level, but the tiles have to be
        // visited once so partial cells can even out.
        self.wake(
            tx0 * TILE,
            ty0 * TILE,
            tx1 * TILE + TILE - 1,
            ty1 * TILE + TILE - 1,
        );
    }

    /// Wakes the tiles touched by a cell rectangle, every tile above it (water
    /// up there can now pour in) and one tile below.
    ///
    /// `y0` is not read: what matters is everything *above* the carve, since any
    /// water there can now fall into it. Neighbouring columns are woken too, so
    /// water sitting beside a new drain starts moving instead of waiting for a
    /// ripple that will never come.
    pub fn wake(&mut self, x0: i32, _y0: i32, x1: i32, y1: i32) {
        // The rectangle arrives in world cell coordinates; tiles are indexed.
        let ix0 = x0 - self.bounds.x as i32;
        let ix1 = x1 - self.bounds.x as i32;
        let iy1 = y1 - self.bounds.y as i32;
        let tx0 = ((ix0 / TILE) - 1).clamp(0, self.tiles_x - 1);
        let tx1 = ((ix1 / TILE) + 1).clamp(0, self.tiles_x - 1);
        let ty_lo = ((iy1 / TILE) + 1).clamp(0, self.tiles_y - 1);
        for ty in 0..=ty_lo {
            for tx in tx0..=tx1 {
                self.next[(ty as usize) * (self.tiles_x as usize) + tx as usize] = true;
            }
        }
    }

    /// Wakes every tile, for a one-off pass.
    pub fn wake_all(&mut self) {
        self.active.fill(true);
    }

    /// One tick of flow. Returns the number of cell moves.
    ///
    /// The per-tick cap bounds how much work one frame does; it must never *drop*
    /// the tiles it did not get to. A carve wakes every tile above it, and on a
    /// real level that is more than the cap: dropping the rest orphans the water
    /// above the hole, which then sits in the air forever.
    pub fn step(&mut self, terrain: &Terrain) -> u32 {
        let mut changed = 0u32;
        let mut visited = 0usize;
        let mut capped = false;
        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                let ti = (ty as usize) * (self.tiles_x as usize) + tx as usize;
                if !self.active[ti] {
                    continue;
                }
                if capped {
                    self.next[ti] = true;
                    continue;
                }
                visited += 1;
                changed += self.step_tile(terrain, tx, ty);
                if visited == MAX_ACTIVE_TILES {
                    capped = true;
                }
            }
        }
        std::mem::swap(&mut self.active, &mut self.next);
        self.next.fill(false);
        self.moved = changed;
        changed
    }

    /// Digest of the water's current shape, folded into the replay checksum.
    pub fn digest(&self) -> u64 {
        let mut h = Fnv::new();
        for s in &self.tile_sig {
            h.u64(*s);
        }
        h.finish()
    }

    fn step_tile(&mut self, terrain: &Terrain, tx: i32, ty: i32) -> u32 {
        let x0 = tx * TILE;
        let x1 = (x0 + TILE).min(self.w) - 1;
        let y0 = ty * TILE;
        let y1 = (y0 + TILE).min(self.h) - 1;
        if x1 < x0 || y1 < y0 {
            return 0;
        }
        let mut changed = 0u32;
        // Bottom-up, so a cell that just received water is finished with before
        // the cell above it decides where to send the next drop.
        for y in (y0..=y1).rev() {
            let serpentine = (y & 1) == 1;
            for k in 0..=(x1 - x0) {
                let x = if serpentine { x1 - k } else { x0 + k };
                changed += self.step_cell(terrain, x, y);
            }
        }
        if changed > 0 {
            self.rehash_tile(tx, ty);
            self.mark_neighbours(tx, ty);
        }
        changed
    }

    fn step_cell(&mut self, terrain: &Terrain, x: i32, y: i32) -> u32 {
        let i = self.index(x, y).expect("cells come from the tile range");
        let mut m = self.mass[i];
        if m == 0 {
            return 0;
        }
        let mut moves = 0u32;
        let below = terrain.solid_index(x, y + 1);
        let preferred = if (x + y) & 1 == 0 { 1 } else { -1 };

        // 1. Straight down, if the water is falling.
        if !below && let Some(j) = self.open(terrain, x, y + 1) {
            let n = m.min(FULL - self.mass[j]).min(FALL);
            if n > 0 {
                self.mass[i] = m - n;
                self.mass[j] += n;
                m -= n;
                moves += 1;
            }
        }

        // 2. Over a lip: run down the diagonal, preferring one side so a slope
        //    does not send the whole pool the same way every tick.
        if m > 0 && below {
            for k in 0..2 {
                let dx = if k == 0 { preferred } else { -preferred };
                let Some(j) = self.open(terrain, x + dx, y + 1) else {
                    continue;
                };
                let n = m.min(FULL - self.mass[j]).min(FALL);
                if n > 0 {
                    self.mass[i] -= n;
                    self.mass[j] += n;
                    m -= n;
                    moves += 1;
                }
                if m == 0 {
                    return moves;
                }
            }
        }

        // 3. Sideways, from a surface cell, towards the neighbour column that is
        //    carrying less water. Comparing columns rather than cells is what
        //    makes a pool level out exactly: a local rule lets every column keep
        //    one unit more than its neighbour, which over a wide pool becomes a
        //    slope, and comparing *rows* instead lets two equal columns hand the
        //    same water back and forth forever. Half the difference, so a
        //    transfer can never overshoot into a new imbalance.
        if m > 0 {
            for k in 0..2 {
                let dx = if k == 0 { preferred } else { -preferred };
                let nx = x + dx;
                if nx < 0 || nx >= self.w {
                    continue;
                }
                let mine = self.col_mass[x as usize];
                let theirs = self.col_mass[nx as usize];
                if mine < theirs + 2 {
                    continue;
                }
                let Some(_) = self.open(terrain, nx, y) else {
                    continue;
                };
                let j = self.index(nx, y).expect("open checked the bounds");
                let n = ((mine - theirs) / 2) as u8;
                let n = n.min(SPREAD).min(FULL - self.mass[j]).min(m);
                if n > 0 {
                    self.pour(x, y, nx, y, n);
                    m -= n;
                    moves += 1;
                }
                if m == 0 {
                    break;
                }
            }
        }
        moves
    }

    /// Moves `n` units between two cells, both keyed by grid index, and keeps the
    /// column totals that drive the sideways rule current.
    fn pour(&mut self, fx: i32, fy: i32, tx: i32, ty: i32, n: u8) {
        let fi = (fy as usize) * (self.w as usize) + fx as usize;
        let ti = (ty as usize) * (self.w as usize) + tx as usize;
        self.mass[fi] -= n;
        self.mass[ti] += n;
        self.col_mass[fx as usize] -= n as u32;
        self.col_mass[tx as usize] += n as u32;
    }

    /// In-bounds, not solid: the index a transfer may write to.
    fn open(&self, terrain: &Terrain, x: i32, y: i32) -> Option<usize> {
        if terrain.solid_index(x, y) {
            return None;
        }
        self.index(x, y)
    }

    fn index(&self, wx: i32, wy: i32) -> Option<usize> {
        if wx < 0 || wy < 0 || wx >= self.w || wy >= self.h {
            return None;
        }
        Some((wy as usize) * (self.w as usize) + wx as usize)
    }

    /// Water units at a cell index.
    fn mass_index(&self, ix: i32, iy: i32) -> u8 {
        match self.index(ix, iy) {
            Some(i) => self.mass[i],
            None => 0,
        }
    }

    /// Cell index of world integer coordinates.
    fn to_index(&self, wx: i32, wy: i32) -> (i32, i32) {
        (wx - self.bounds.x as i32, wy - self.bounds.y as i32)
    }

    fn cell_of(&self, p: V2) -> (i32, i32) {
        (
            (p.x - self.bounds.x).floor() as i32,
            (p.y - self.bounds.y).floor() as i32,
        )
    }

    /// Keeps the tile itself and its neighbours queued for the next tick.
    fn mark_neighbours(&mut self, tx: i32, ty: i32) {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let nx = tx + dx;
                let ny = ty + dy;
                if nx < 0 || ny < 0 || nx >= self.tiles_x || ny >= self.tiles_y {
                    continue;
                }
                self.next[(ny as usize) * (self.tiles_x as usize) + nx as usize] = true;
            }
        }
    }

    fn rehash_tile(&mut self, tx: i32, ty: i32) {
        let bx = tx * TILE;
        let by = ty * TILE;
        let mut h = Fnv::new();
        for y in by..(by + TILE).min(self.h) {
            for x in bx..(bx + TILE).min(self.w) {
                h.u8(self.mass[(y as usize) * (self.w as usize) + x as usize]);
            }
        }
        self.tile_sig[(ty as usize) * (self.tiles_x as usize) + tx as usize] = h.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::terrain::{MAT_DIRT, Wall};

    fn rect_wall(x0: f32, y0: f32, x1: f32, y1: f32) -> Wall {
        Wall::new(
            vec![
                V2::new(x0, y0),
                V2::new(x1, y0),
                V2::new(x1, y1),
                V2::new(x0, y1),
            ],
            MAT_DIRT,
        )
    }

    /// Two chambers: a basin over a thin floor, and a lower hall beneath it.
    fn basin() -> Terrain {
        Terrain::new(
            &[
                rect_wall(0.0, 100.0, 120.0, 104.0),  // thin floor
                rect_wall(0.0, 140.0, 120.0, 144.0),  // lower floor
                rect_wall(0.0, 40.0, 6.0, 144.0),     // basin wall, left
                rect_wall(114.0, 40.0, 120.0, 144.0), // basin wall, right
            ],
            1,
        )
    }

    fn basin_water(terrain: &Terrain) -> Water {
        let mut water = Water::new(terrain, WaterParams::default());
        water.fill(
            terrain,
            &[
                V2::new(7.0, 60.0),
                V2::new(113.0, 60.0),
                V2::new(113.0, 100.0),
                V2::new(7.0, 100.0),
            ],
        );
        water.wake_all();
        water
    }

    fn settle(water: &mut Water, terrain: &Terrain, ticks: u32) -> u32 {
        let mut last = 0;
        for _ in 0..ticks {
            last = water.step(terrain);
        }
        last
    }

    /// Topmost cell holding water in a column, or `None`.
    fn surface_row(water: &Water, x: i32, y_lo: i32, y_hi: i32) -> Option<i32> {
        (y_lo..=y_hi).find(|y| water.mass(x, *y) > 0)
    }

    /// A wide hall with a thick floor under a pool, big enough that a wake covers
    /// far more tiles than one tick is allowed to visit.
    fn wide_hall() -> (Terrain, Water) {
        let terrain = Terrain::new(
            &[
                rect_wall(0.0, 400.0, 1400.0, 520.0), // the floor, 120 px thick
                rect_wall(0.0, 700.0, 1400.0, 720.0), // the hall's own floor
                rect_wall(0.0, 100.0, 12.0, 720.0),   // left wall
                rect_wall(1388.0, 100.0, 1400.0, 720.0), // right wall
                rect_wall(0.0, 100.0, 1400.0, 112.0), // ceiling
            ],
            1,
        );
        let mut water = Water::new(&terrain, WaterParams::default());
        water.fill(
            &terrain,
            &[
                V2::new(12.0, 200.0),
                V2::new(1388.0, 200.0),
                V2::new(1388.0, 400.0),
                V2::new(12.0, 400.0),
            ],
        );
        water.wake_all();
        (terrain, water)
    }

    /// The per-tick tile cap bounds work; it must not lose tiles. Blasting a floor
    /// wide enough to wake more tiles than the cap leaves the pool sitting in the
    /// air unless the skipped tiles stay queued.
    #[test]
    fn a_wake_bigger_than_the_tick_cap_still_drains() {
        let (mut terrain, mut water) = wide_hall();
        let mut guard = 0;
        while !water.is_settled() && guard < 100_000 {
            water.step(&terrain);
            guard += 1;
        }
        let total_before = water.total();
        assert!(water.mass(700, 380) > 0, "the pool sits on the floor");

        // Blast a hole wide enough that the wake covers every tile above it.
        let mut removed = 0;
        for x in (100..1300).step_by(100) {
            let carve = terrain.carve(V2::new(x as f32, 460.0), 70.0);
            removed += carve.removed;
            water.wake(carve.x0, carve.y0, carve.x1, carve.y1);
        }
        assert!(removed > 0, "the floor is dirt");

        let mut guard = 0;
        while !water.is_settled() && guard < 100_000 {
            water.step(&terrain);
            guard += 1;
        }
        assert!(
            guard < 100_000,
            "the water never came to rest after the blast"
        );
        assert_eq!(
            water.total(),
            total_before,
            "water is conserved when it pours"
        );
        assert!(
            water.mass(700, 500) > 0,
            "the hall below the blast is still empty: the wake dropped its tiles"
        );
        assert!(
            water.mass(700, 380) == 0,
            "the pool is still sitting above the hole"
        );
    }

    #[test]
    fn a_fresh_pool_settles_and_then_costs_nothing() {
        let terrain = basin();
        let mut water = basin_water(&terrain);
        assert_eq!(water.total(), (113 - 7) as u64 * 40 * FULL as u64);
        let moved = settle(&mut water, &terrain, 400);
        assert_eq!(moved, 0, "a level pool must come to rest");
        assert!(water.is_settled());
        assert_eq!(water.step(&terrain), 0, "and stay free once it is");
    }

    #[test]
    fn blasting_the_floor_under_a_pool_drains_it_downward() {
        let mut terrain = basin();
        let mut water = basin_water(&terrain);
        settle(&mut water, &terrain, 400);
        let before = surface_row(&water, 60, 50, 99).expect("water in the basin");
        let total_before = water.total();

        let carve = terrain.carve(V2::new(60.0, 102.0), 6.0);
        assert!(carve.hit_terrain(), "the floor is dirt");
        water.wake(carve.x0, carve.y0, carve.x1, carve.y1);

        settle(&mut water, &terrain, 1200);
        assert_eq!(
            water.total(),
            total_before,
            "water is conserved when it pours"
        );
        assert!(
            water.mass(60, 130) > 0,
            "the lower hall should have taken water"
        );
        // The whole basin drops as the water leaves, not just the column above
        // the hole, so measure well clear of the crater.
        let after = surface_row(&water, 20, 50, 99).expect("some water stays above");
        assert!(
            after > before,
            "the basin level must drop: {before} -> {after}"
        );
        assert!(
            water.is_settled(),
            "and the pool has to come to rest again afterwards"
        );
    }

    /// Water in a column, in mass units.
    fn column_water(water: &Water, x: i32) -> u64 {
        (40..=120).map(|y| water.mass(x, y) as u64).sum()
    }

    #[test]
    fn water_spreads_across_a_basin_from_one_end() {
        let terrain = basin();
        let mut water = Water::new(&terrain, WaterParams::default());
        // A tall block of water on the left of an empty basin.
        water.fill(
            &terrain,
            &[
                V2::new(7.0, 60.0),
                V2::new(40.0, 60.0),
                V2::new(40.0, 100.0),
                V2::new(7.0, 100.0),
            ],
        );
        water.wake_all();
        assert_eq!(
            settle(&mut water, &terrain, 3000),
            0,
            "it has to come to rest"
        );

        assert!(
            surface_row(&water, 110, 50, 99).is_some(),
            "the water must reach the far side of the basin"
        );
        // The model levels by comparing whole columns, so neighbouring columns
        // never differ by more than one unit of water: a pool cannot step down,
        // and a slope cannot grow with the pool's width. It is not a pressure
        // solver, so a pool *can* sit a fraction of a cell deeper at one end
        // (`docs/design.md` §12).
        for x in 8..112 {
            let (a, b) = (column_water(&water, x), column_water(&water, x + 1));
            assert!(
                a.abs_diff(b) <= 1,
                "columns {x} and {} differ by {} units of water",
                x + 1,
                a.abs_diff(b)
            );
        }
    }

    #[test]
    fn solid_cells_never_hold_water() {
        let terrain = basin();
        let mut water = Water::new(&terrain, WaterParams::default());
        // A pool polygon that overlaps the floor and both walls.
        water.fill(
            &terrain,
            &[
                V2::new(0.0, 95.0),
                V2::new(120.0, 95.0),
                V2::new(120.0, 145.0),
                V2::new(0.0, 145.0),
            ],
        );
        water.wake_all();
        settle(&mut water, &terrain, 200);
        for y in 100..104 {
            for x in 0..120 {
                assert_eq!(water.mass(x, y), 0, "water inside the floor at {x},{y}");
            }
        }
        assert!(water.mass(60, 98) > 0, "the open cells did fill");
    }

    #[test]
    fn digest_follows_the_water() {
        let terrain = basin();
        let mut water = basin_water(&terrain);
        settle(&mut water, &terrain, 400);
        let before = water.digest();
        assert_eq!(before, water.clone().digest());
        let (ix, iy) = water.to_index(60, 90);
        let i = water.index(ix, iy).unwrap();
        water.mass[i] = 0;
        water.rehash_tile(ix / TILE, iy / TILE);
        assert_ne!(before, water.digest());
    }
}
