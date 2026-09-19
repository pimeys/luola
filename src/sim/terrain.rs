//! Terrain: a destructible material grid.
//!
//! The cave used to be a union of wall polygons, which is exactly how the genre
//! authors its levels — big caves built from coarse overlapping brushes
//! (`docs/game_mechanics.md` §8.7). It is now a per-cell material grid, because
//! the genre's walls are *diggable* (§2: Gravity Force 2 and AUTS both ship
//! destructible granular walls) and because water needs cells to live in
//! (`water.rs`).
//!
//! Levels are authored as brushes and rasterized into cells once, at load;
//! everything after that works on the grid. A point is solid when its cell is
//! not empty, so the union semantics fall out of rasterizing the brushes in
//! order with "rock wins over dirt".
//!
//! A brush is a polygon, a disc, a swept chain of discs, or a lumpy blob — the
//! genre's caves are piles of discs, and a level that is a stack of rectangles
//! does not read as a cave (`docs/game_mechanics.md` §8.7). The grid also
//! carries a rock casing of its own ([`CASING`]), so the cave's outer wall is
//! structural: an author cannot forget it and a level cannot leak.
//!
//! Three materials matter to the simulation:
//!
//! * `MAT_DIRT` — the body of the cave. Bullets and blasts remove it, which is
//!   how the player tunnels, opens water and digs out a blocked route.
//! * `MAT_GRANULAR` — a granular wall (AUTS' *rakeinen seinä*). It has hit
//!   points, so a shot chips it instead of opening it at once, and the ship
//!   that flies into one is held by it until it shoots itself free.
//! * `MAT_ROCK` — authored stone. Blasts never move it, so a level's layout
//!   still means something once the player can dig. Dirt does not fall when it
//!   is undermined (see `docs/design.md`, non-goals), so a dug tunnel holds its
//!   shape.
//!
//! Every non-rock solid cell carries a hit-point value in a parallel grid, so
//! "terrain takes damage" is a property of the floor and not only of blasts:
//! dirt has one point, granular walls many, and rock is indestructible.
//!
//! Collision marches the swept segment through the grid, sampling every half
//! cell of travel, so a body cannot tunnel through a wall however fast it
//! travels. A body that has ended up *inside* the rock is pushed back out by
//! [`Terrain::penetration`], the way it came in.

use crate::math::{Rect, V2};
use crate::sim::fnv::Fnv;

pub const MAT_EMPTY: u8 = 0;
pub const MAT_DIRT: u8 = 1;
pub const MAT_ROCK: u8 = 2;
/// A granular wall: eats fire for several hits and traps the ship that touches
/// it until it is shot free (AUTS, `docs/game_mechanics.md` §2, §7).
pub const MAT_GRANULAR: u8 = 3;

/// Rasterization precedence: rock always wins, then granular, then dirt, so a
/// level's brushes overlap in a defined order regardless of their authored
/// position in the file.
fn mat_rank(m: u8) -> u8 {
    match m {
        MAT_ROCK => 3,
        MAT_GRANULAR => 2,
        MAT_DIRT => 1,
        _ => 0,
    }
}

/// Rasterization precedence as a public predicate, for callers outside this
/// module that need the same rule.
pub fn material_overrides(new: u8, existing: u8) -> bool {
    mat_rank(new) > mat_rank(existing)
}

/// Side of the tiles the incremental digests are kept in.
const TILE: i32 = 32;
/// A swept segment is sampled every `1 / MARCH` cells of travel.
const MARCH: f32 = 2.0;
/// Longer segments are line-of-sight queries, not contacts; they can afford a
/// coarser sample so a 3000 px ray stays cheap.
const FINE_LEN: f32 = 512.0;
const COARSE_MARCH: f32 = 0.5;
/// How far `penetration` looks for a way out of the rock, in cells.
const MAX_RING: i32 = 96;
const MAX_CANDIDATES: usize = 8;
/// Texture seed for the per-cell grain. Fixed, so dirt looks the same every run.
const SHADE_SEED: u32 = 0x1a2b_3c4d;
/// Thickness of the rock casing the grid carries around everything the level
/// authored, in cells. The cave's outer wall is structural, not authored: it
/// cannot leak, and an author cannot forget to draw it.
pub const CASING: i32 = 20;
/// A blob's spine is always drawn at this fraction of its lumpy radius, so the
/// lumps sit on a connected core and a mass never comes out in pieces.
const BLOB_CORE: f32 = 0.55;
/// Spacing of the discs stamped along a swept spine, as a fraction of the radius.
///
/// It has to be small: a chain is a row of tangent discs, and at the very edge of
/// one the disc only covers a short patch of the row — `sqrt(r^2 - (r - 1)^2)`,
/// about `r / 7` either side. Space them wider than that and the chain's outline
/// is scalloped, which leaves one-cell pockets of air along any wall that has to
/// meet the casing.
const SWEEP_STEP: f32 = 0.12;

/// One authored brush's geometry.
///
/// The organic shapes are the point: a cave is a pile of discs. `Poly` stays for
/// machine-cut rooms, shafts and flat floors, where the straight line is the
/// design rather than a limitation.
#[derive(Clone, Debug)]
pub enum Shape {
    /// A polygon.
    Poly(Vec<V2>),
    /// One disc: the building block of every organic mass.
    Disc { center: V2, radius: f32 },
    /// A disc swept along a polyline: walls, tunnels, arches.
    Chain { points: Vec<V2>, radius: f32 },
    /// A lumpy mass: discs of varying radius scattered along a spine.
    Blob {
        points: Vec<V2>,
        radius: f32,
        /// How many lumps to scatter; 0 sizes them from the spine's length.
        lumps: u32,
        /// Which lumps to scatter. Fixed per brush, so the cave is the same
        /// cave on every machine and every run.
        seed: u32,
    },
    /// The grid's own margin, filled to `thickness`.
    ///
    /// The outer wall of a cave, as a parameter, and *square*: an organic ring
    /// wall drawn from discs is round at the corners, and a round wall inside a
    /// rectangular grid traps air in all four of them. This fills the margin
    /// behind whatever the author drew, so no sliver of unreachable air survives
    /// outside the wall and there is nothing to remember per level.
    Border { thickness: f32 },
}

impl Shape {
    /// Bounding box of the shape in world space; empty for a border, which is
    /// defined by the grid rather than the other way round.
    pub fn bounds(&self) -> Rect {
        match self {
            Shape::Border { .. } => Rect::default(),
            Shape::Poly(points) => bbox_of(points),
            Shape::Disc { center, radius } => Rect::new(
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            ),
            Shape::Chain { points, radius } | Shape::Blob { points, radius, .. } => {
                let b = bbox_of(points);
                Rect::new(
                    b.x - radius,
                    b.y - radius,
                    b.w + radius * 2.0,
                    b.h + radius * 2.0,
                )
            }
        }
    }
}

/// One authored brush, tagged with the material it paints.
#[derive(Clone, Debug)]
pub struct Wall {
    pub shape: Shape,
    pub material: u8,
}

impl Wall {
    /// A polygon brush.
    pub fn new(points: Vec<V2>, material: u8) -> Self {
        Self {
            shape: Shape::Poly(points),
            material,
        }
    }

    /// A disc brush.
    pub fn disc(center: V2, radius: f32, material: u8) -> Self {
        Self {
            shape: Shape::Disc { center, radius },
            material,
        }
    }

    /// A swept-disc brush along `points`.
    pub fn chain(points: Vec<V2>, radius: f32, material: u8) -> Self {
        Self {
            shape: Shape::Chain { points, radius },
            material,
        }
    }

    /// A lumpy mass along `points`.
    pub fn blob(points: Vec<V2>, radius: f32, lumps: u32, seed: u32, material: u8) -> Self {
        Self {
            shape: Shape::Blob {
                points,
                radius,
                lumps,
                seed,
            },
            material,
        }
    }

    /// A border brush: the grid's margin, `thickness` deep.
    pub fn border(thickness: f32, material: u8) -> Self {
        Self {
            shape: Shape::Border { thickness },
            material,
        }
    }

    pub fn bounds(&self) -> Rect {
        self.shape.bounds()
    }
}

/// Per-column vertical spans of solid terrain, used by the software renderer.
///
/// Filled cells are runs of solid material with no holes, so the cave's fill can
/// be blitted a run at a time. The runs are in *world* y, matching what the
/// camera hands the renderer, and they are re-baked for the affected columns
/// whenever a carve changes the grid.
#[derive(Clone, Debug, Default)]
pub struct Spans {
    pub x0: i32,
    pub cols: Vec<Vec<(i32, i32)>>,
}

impl Spans {
    pub fn column(&self, wx: i32) -> &[(i32, i32)] {
        let idx = wx - self.x0;
        if idx < 0 {
            &[]
        } else {
            self.cols
                .get(idx as usize)
                .map(|c| c.as_slice())
                .unwrap_or(&[])
        }
    }
}

/// A swept collision against wall geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub p: V2,
    /// Surface normal, oriented against the direction of travel.
    pub normal: V2,
    /// How far the body was inside the rock; zero for a swept crossing.
    pub depth: f32,
    /// The material of the cell that was hit. Collision reads this to tell a
    /// granular wall (which grabs) from everything else (which kills).
    pub material: u8,
}

/// What a carve actually removed, and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carve {
    /// Cells removed. Zero when the blast only touched rock or open air.
    pub removed: u32,
    /// Cells that took damage but survived (a granular wall being chipped).
    pub damaged: u32,
    /// Deepest material removed; `MAT_EMPTY` when nothing was.
    pub material: u8,
    /// The cell rectangle that was touched, in world cell coordinates.
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Carve {
    /// Nothing removed.
    pub const NONE: Carve = Carve {
        removed: 0,
        damaged: 0,
        material: MAT_EMPTY,
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    };

    pub fn hit_terrain(&self) -> bool {
        self.removed > 0
    }

    /// True when the crater removed a cell *or* only chipped one.
    pub fn touched(&self) -> bool {
        self.removed > 0 || self.damaged > 0
    }
}

/// The cave, as cells.
#[derive(Clone, Debug)]
pub struct Terrain {
    /// Grid rectangle in world space: cell `(x, y)` covers the unit square with
    /// its top-left corner at `bounds.x + x`, `bounds.y + y`.
    pub bounds: Rect,
    /// The brushes this grid was rasterized from.
    pub walls: Vec<Wall>,
    w: i32,
    h: i32,
    cells: Vec<u8>,
    /// Hit points per cell, parallel to `cells`. Zero for empty and rock (rock
    /// is indestructible, so its value is never read).
    hp: Vec<u8>,
    /// Static per-cell brightness, for the dirt/rock texture. Only solid cells
    /// are filled; digging exposes cells that were already painted.
    shade: Vec<u8>,
    spans: Spans,
    tiles_x: i32,
    tiles_y: i32,
    tile_sig: Vec<u64>,
}

impl Terrain {
    /// Rasterizes `walls` into a fresh grid.
    pub fn new(walls: &[Wall], casing: i32) -> Self {
        let mut bbox: Option<Rect> = None;
        for wall in walls {
            let r = wall.bounds();
            if !r.x.is_finite() || !r.y.is_finite() || !r.w.is_finite() || !r.h.is_finite() {
                continue;
            }
            // A border brush is the grid's margin: it has no extent to add.
            if r.w <= 0.0 || r.h <= 0.0 {
                continue;
            }
            bbox = Some(match bbox {
                Some(b) => b.union(&r),
                None => r,
            });
        }
        // Everything the level authored, plus the rock casing, plus one cell of
        // margin: a brush touching the edge still has a neighbour to compare
        // against for the surface and roof tests.
        let bbox = bbox.unwrap_or_default();
        let casing = casing.max(0);
        let x0 = bbox.x.floor() as i32 - casing;
        let y0 = bbox.y.floor() as i32 - casing;
        let x1 = bbox.right().ceil() as i32 + casing;
        let y1 = bbox.bottom().ceil() as i32 + casing;
        let w = (x1 - x0).max(1);
        let h = (y1 - y0).max(1);
        let bounds = Rect::new(x0 as f32, y0 as f32, w as f32, h as f32);

        let origin = V2::new(bounds.x, bounds.y);
        let mut cells = vec![MAT_EMPTY; (w as usize) * (h as usize)];
        for wall in walls {
            rasterize(&mut cells, w, h, origin, wall);
        }
        paint_casing(&mut cells, w, h, casing);

        let shade = make_shade(&cells, w, h);
        let hp = make_hp(&cells);
        let spans = bake_spans(&cells, w, h, origin);
        let tiles_x = ((w + TILE - 1) / TILE).max(1);
        let tiles_y = ((h + TILE - 1) / TILE).max(1);
        let mut terrain = Terrain {
            bounds,
            walls: walls.to_vec(),
            w,
            h,
            cells,
            hp,
            shade,
            spans,
            tiles_x,
            tiles_y,
            tile_sig: vec![0; (tiles_x as usize) * (tiles_y as usize)],
        };
        for ty in 0..terrain.tiles_y {
            for tx in 0..terrain.tiles_x {
                terrain.rehash_tile(tx, ty);
            }
        }
        terrain
    }

    pub fn width(&self) -> i32 {
        self.w
    }

    pub fn height(&self) -> i32 {
        self.h
    }

    pub fn spans(&self) -> &Spans {
        &self.spans
    }

    /// Material of a cell, in *world* integer coordinates; `MAT_EMPTY` outside.
    pub fn cell(&self, wx: i32, wy: i32) -> u8 {
        let (ix, iy) = self.to_index(wx, wy);
        match self.index(ix, iy) {
            Some(i) => self.cells[i],
            None => MAT_EMPTY,
        }
    }

    pub fn solid(&self, wx: i32, wy: i32) -> bool {
        self.cell(wx, wy) != MAT_EMPTY
    }

    /// Material of the cell containing `p`; `MAT_EMPTY` outside.
    pub fn material_at(&self, p: V2) -> u8 {
        let (ix, iy) = self.cell_of(p);
        match self.index(ix, iy) {
            Some(i) => self.cells[i],
            None => MAT_EMPTY,
        }
    }

    /// Remaining hit points of the cell containing `p`.
    pub fn hp_at(&self, p: V2) -> u8 {
        let (ix, iy) = self.cell_of(p);
        match self.index(ix, iy) {
            Some(i) => self.hp[i],
            None => 0,
        }
    }

    /// Hit points of a cell in *world* integer coordinates.
    pub fn hp_cell(&self, wx: i32, wy: i32) -> u8 {
        let (ix, iy) = self.to_index(wx, wy);
        match self.index(ix, iy) {
            Some(i) => self.hp[i],
            None => 0,
        }
    }

    /// Solid test in cell-index space, for callers that walk the grid.
    pub fn solid_index(&self, ix: i32, iy: i32) -> bool {
        matches!(self.index(ix, iy), Some(i) if self.cells[i] != MAT_EMPTY)
    }

    pub fn solid_at(&self, p: V2) -> bool {
        let (ix, iy) = self.cell_of(p);
        self.solid_index(ix, iy)
    }

    /// True when the point lies inside solid terrain.
    pub fn point_solid(&self, p: V2) -> bool {
        self.solid_at(p)
    }

    /// Static texture brightness of a cell, in world coordinates.
    pub fn shade(&self, wx: i32, wy: i32) -> u8 {
        let (ix, iy) = self.to_index(wx, wy);
        match self.index(ix, iy) {
            Some(i) => self.shade[i],
            None => 0,
        }
    }

    /// A solid cell with open air above it: the grass line.
    pub fn surface(&self, wx: i32, wy: i32) -> bool {
        self.solid(wx, wy) && !self.solid(wx, wy - 1)
    }

    /// A solid cell with open air below it: an overhang, where roots hang.
    pub fn roof(&self, wx: i32, wy: i32) -> bool {
        self.solid(wx, wy) && !self.solid(wx, wy + 1)
    }

    /// Cell index containing `p`.
    pub fn cell_of(&self, p: V2) -> (i32, i32) {
        (
            (p.x - self.bounds.x).floor() as i32,
            (p.y - self.bounds.y).floor() as i32,
        )
    }

    /// Centre of a cell, from *world* integer coordinates.
    pub fn cell_center(&self, wx: i32, wy: i32) -> V2 {
        V2::new(wx as f32 + 0.5, wy as f32 + 0.5)
    }

    /// Centre of a cell, from cell indices.
    fn center_of_index(&self, ix: i32, iy: i32) -> V2 {
        V2::new(
            self.bounds.x + ix as f32 + 0.5,
            self.bounds.y + iy as f32 + 0.5,
        )
    }

    /// Cells currently holding terrain — handy for level reports and tests.
    pub fn solid_cells(&self) -> usize {
        self.cells.iter().filter(|c| **c != MAT_EMPTY).count()
    }

    /// Nearest wall intersection along the swept segment `a`-`b`, if any.
    pub fn segment_hit(&self, a: V2, b: V2) -> Option<Hit> {
        let d = b - a;
        let len = d.len();
        if len.is_nan() || len <= 1e-6 || !d.is_finite() {
            return None;
        }
        let step = if len <= FINE_LEN {
            1.0 / MARCH
        } else {
            COARSE_MARCH
        };
        let steps = (len / step).ceil().max(1.0) as i32;
        let inv = 1.0 / steps as f32;
        for i in 1..=steps {
            let t = i as f32 * inv;
            let q = a + d * t;
            if self.solid_at(q) {
                // Report the crossing just before the first solid sample.
                let hp = a + d * (t - inv * 0.5).max(0.0);
                return Some(Hit {
                    p: hp,
                    normal: self.surface_normal(q, d),
                    depth: 0.0,
                    material: self.material_at(q),
                });
            }
        }
        None
    }

    /// When a point has ended up *inside* the wall mass, returns the surface
    /// point on the way out and the direction that leads back out of the rock.
    ///
    /// Swept tests only catch crossings; a body that penetrated the mass can
    /// start its next sweep already inside, where there is nothing to cross.
    /// This is the containment net that stops that, and it is only reached when
    /// a body really is inside solid rock.
    pub fn penetration(&self, p: V2, away_from: V2) -> Option<Hit> {
        if !self.solid_at(p) {
            return None;
        }
        let travel = if away_from.len_sq() > 1e-9 {
            away_from.normalized()
        } else {
            V2::new(0.0, 1.0)
        };
        let (px, py) = self.cell_of(p);

        // Candidate exits: the open cells around `p`, nearest first. The nearest
        // one sets the scale; anything much further is ignored, so a body that
        // entered at speed is not teleported out of the far side of the wall.
        let mut nearest = f32::MAX;
        let mut nearest_ring = MAX_RING + 1;
        let mut cands: [(f32, i32, i32); MAX_CANDIDATES] = [(f32::MAX, 0, 0); MAX_CANDIDATES];
        for r in 1..=MAX_RING {
            if r > nearest_ring + 4 {
                break;
            }
            for k in 0..(8 * r) {
                let (x, y) = ring_point(px, py, r, k);
                if self.solid_index(x, y) || self.is_margin(x, y) {
                    continue;
                }
                let d = self.center_of_index(x, y).dist(p);
                if d < nearest {
                    nearest = d;
                    nearest_ring = r;
                }
                if d < cands[MAX_CANDIDATES - 1].0 {
                    cands[MAX_CANDIDATES - 1] = (d, x, y);
                    cands
                        .sort_by(|m, n| m.0.partial_cmp(&n.0).unwrap_or(std::cmp::Ordering::Equal));
                }
            }
        }

        // Which way out: the open side, not the way the body happens to be
        // moving. Velocity was the old proxy for "the face it came from", and it
        // points at the far face the moment a bounced body is thrust back into
        // the rock — which is how a buried ship ends up outside the level.
        let window = if nearest.is_finite() {
            nearest * 1.6 + 4.0
        } else {
            MAX_RING as f32
        };
        let mut best: Option<(u32, f32, i32, i32)> = None;
        for (d, x, y) in cands {
            if d > window {
                continue;
            }
            let open = self.openness(x, y);
            let better = match best {
                None => true,
                Some((bo, bd, _, _)) => open > bo || (open == bo && d < bd - 0.5),
            };
            if better {
                best = Some((open, d, x, y));
            }
        }
        let mut dir = match best {
            Some((_, _, x, y)) => (self.center_of_index(x, y) - p).normalized(),
            None => V2::ZERO,
        };
        if dir.len_sq() < 1e-9 {
            dir = -travel;
        }

        // Walk out along `dir` to the last solid sample: that is the surface.
        let limit = if nearest.is_finite() {
            nearest + 2.0
        } else {
            MAX_RING as f32
        };
        let mut q = p;
        let mut t = 0.0f32;
        while t <= limit {
            let s = p + dir * t;
            if !self.solid_at(s) {
                break;
            }
            q = s;
            t += 0.5;
        }
        Some(Hit {
            p: q,
            normal: dir,
            depth: q.dist(p),
            material: self.material_at(p),
        })
    }

    /// Removes dirt within `radius` of `p`, leaving rock and open air alone.
    ///
    /// Returns what changed, so the caller can wake the water above the hole and
    /// spawn the right debris. A blast is damage enough to destroy any cell, so
    /// this is `damage` at its maximum.
    pub fn carve(&mut self, p: V2, radius: f32) -> Carve {
        self.damage(p, radius, u8::MAX)
    }

    /// Applies `amount` hit points of damage to every solid cell within
    /// `radius` of `p`. Dirt (one hit point) is removed at once; granular walls
    /// need several hits; rock never moves.
    ///
    /// Returns what changed, so the caller can wake the water above a hole and
    /// spawn the right debris for the material that broke.
    pub fn damage(&mut self, p: V2, radius: f32, amount: u8) -> Carve {
        let r = radius.max(0.0);
        let cx0 = ((p.x - r) - self.bounds.x).floor() as i32;
        let cx1 = ((p.x + r) - self.bounds.x).ceil() as i32;
        let cy0 = ((p.y - r) - self.bounds.y).floor() as i32;
        let cy1 = ((p.y + r) - self.bounds.y).ceil() as i32;
        let x0 = cx0.clamp(0, self.w - 1);
        let x1 = cx1.clamp(0, self.w - 1);
        let y0 = cy0.clamp(0, self.h - 1);
        let y1 = cy1.clamp(0, self.h - 1);

        let mut removed = 0u32;
        let mut damaged = 0u32;
        let mut material = MAT_EMPTY;
        if r > 0.0 && amount > 0 && cx1 >= 0 && cy1 >= 0 && cx0 < self.w && cy0 < self.h {
            let r2 = r * r;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = self.center_of_index(x, y);
                    let (dx, dy) = (c.x - p.x, c.y - p.y);
                    if dx * dx + dy * dy > r2 {
                        continue;
                    }
                    let i = self.index(x, y).expect("clamped to the grid");
                    let m = self.cells[i];
                    if m == MAT_EMPTY || m == MAT_ROCK {
                        continue;
                    }
                    if self.hp[i] <= amount {
                        self.cells[i] = MAT_EMPTY;
                        self.hp[i] = 0;
                        removed += 1;
                        material = material.max(m);
                    } else {
                        self.hp[i] -= amount;
                        damaged += 1;
                    }
                }
            }
        }

        if removed > 0 || damaged > 0 {
            if removed > 0 {
                self.rebuild_columns(x0, x1);
            }
            self.rehash_tiles(x0, y0, x1, y1);
        }
        let bx = self.bounds.x as i32;
        let by = self.bounds.y as i32;
        Carve {
            removed,
            damaged,
            material,
            x0: bx + x0,
            y0: by + y0,
            x1: bx + x1,
            y1: by + y1,
        }
    }

    /// Adds dirt within `radius` of `p` — the craters' inverse.
    ///
    /// Only open cells are filled: a blast hole can be plugged, but rock stays
    /// rock and already-solid dirt is left alone. Returns how many cells became
    /// solid, so the caller can wake the water that the new mass displaced.
    pub fn fill(&mut self, p: V2, radius: f32) -> u32 {
        let r = radius.max(0.0);
        let cx0 = ((p.x - r) - self.bounds.x).floor() as i32;
        let cx1 = ((p.x + r) - self.bounds.x).ceil() as i32;
        let cy0 = ((p.y - r) - self.bounds.y).floor() as i32;
        let cy1 = ((p.y + r) - self.bounds.y).ceil() as i32;
        let x0 = cx0.clamp(0, self.w - 1);
        let x1 = cx1.clamp(0, self.w - 1);
        let y0 = cy0.clamp(0, self.h - 1);
        let y1 = cy1.clamp(0, self.h - 1);

        let mut added = 0u32;
        if r > 0.0 && cx1 >= 0 && cy1 >= 0 && cx0 < self.w && cy0 < self.h {
            let r2 = r * r;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = self.center_of_index(x, y);
                    let (dx, dy) = (c.x - p.x, c.y - p.y);
                    if dx * dx + dy * dy > r2 {
                        continue;
                    }
                    let i = self.index(x, y).expect("clamped to the grid");
                    if self.cells[i] != MAT_EMPTY {
                        continue;
                    }
                    self.cells[i] = MAT_DIRT;
                    self.hp[i] = crate::sim::tuning::DIRT_HP;
                    added += 1;
                }
            }
        }

        if added > 0 {
            self.rebuild_columns(x0, x1);
            self.rehash_tiles(x0, y0, x1, y1);
        }
        added
    }

    /// Digest of the cave's current shape, folded into the replay checksum.
    pub fn digest(&self) -> u64 {
        let mut h = Fnv::new();
        for s in &self.tile_sig {
            h.u64(*s);
        }
        h.finish()
    }

    /// How much open space surrounds a cell: the cave side of a wall is wide
    /// open, the sliver between a wall and the edge of the grid is not. Cells
    /// outside the grid count as closed, because the void is not play space.
    fn openness(&self, cx: i32, cy: i32) -> u32 {
        let mut open = 0;
        for dy in -3..=3 {
            for dx in -3..=3 {
                if let Some(i) = self.index(cx + dx, cy + dy)
                    && self.cells[i] == MAT_EMPTY
                {
                    open += 1;
                }
            }
        }
        open
    }

    /// Normal of the face the segment entered through, oriented against travel.
    fn surface_normal(&self, q: V2, travel: V2) -> V2 {
        let (cx, cy) = self.cell_of(q);
        let mut n = V2::ZERO;
        if !self.solid_index(cx - 1, cy) {
            n.x -= 1.0;
        }
        if !self.solid_index(cx + 1, cy) {
            n.x += 1.0;
        }
        if !self.solid_index(cx, cy - 1) {
            n.y -= 1.0;
        }
        if !self.solid_index(cx, cy + 1) {
            n.y += 1.0;
        }
        if n.len_sq() < 1e-6 {
            // Fully embedded: come out towards the open side. Deliberately not
            // flipped against travel — the escape direction has to point out of
            // the rock, and a body already on its way out is exactly the case
            // where flipping it would drive it back in.
            return self.embedded_escape(q, travel);
        }
        if n.dot(travel) > 0.0 {
            n = -n;
        }
        n.normalized()
    }

    /// The one-cell ring the grid carries around the level's brushes. It is
    /// always empty, and it is not play space: treating it as an exit is how a
    /// body buried in the boundary rock ends up outside the level.
    fn is_margin(&self, ix: i32, iy: i32) -> bool {
        ix <= 0 || iy <= 0 || ix >= self.w - 1 || iy >= self.h - 1
    }

    /// Which way is out of a cell that has no open neighbour at all: the
    /// direction whose first opening leads to the most open space.
    fn embedded_escape(&self, q: V2, travel: V2) -> V2 {
        let (cx, cy) = self.cell_of(q);
        let mut best: Option<(u32, V2)> = None;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let mut x = cx;
            let mut y = cy;
            let mut steps = 0;
            while steps < MAX_RING && (self.solid_index(x, y) || self.is_margin(x, y)) {
                x += dx;
                y += dy;
                steps += 1;
            }
            if self.solid_index(x, y) || self.is_margin(x, y) {
                continue;
            }
            let open = self.openness(x, y);
            if best.map(|(bo, _)| open > bo).unwrap_or(true) {
                best = Some((open, V2::new(dx as f32, dy as f32)));
            }
        }
        match best {
            Some((_, dir)) => dir,
            None => -travel,
        }
    }

    /// Cell index of world integer coordinates. The grid is integer-aligned, so
    /// this is a subtraction, never a rounding.
    fn to_index(&self, wx: i32, wy: i32) -> (i32, i32) {
        (wx - self.bounds.x as i32, wy - self.bounds.y as i32)
    }

    fn index(&self, wx: i32, wy: i32) -> Option<usize> {
        if wx < 0 || wy < 0 || wx >= self.w || wy >= self.h {
            return None;
        }
        Some((wy as usize) * (self.w as usize) + wx as usize)
    }

    fn rebuild_columns(&mut self, x0: i32, x1: i32) {
        for x in x0..=x1 {
            let mut spans: Vec<(i32, i32)> = Vec::new();
            let mut y = 0;
            while y < self.h {
                if self.cells[(y as usize) * (self.w as usize) + x as usize] != MAT_EMPTY {
                    let start = y;
                    while y + 1 < self.h
                        && self.cells[((y + 1) as usize) * (self.w as usize) + x as usize]
                            != MAT_EMPTY
                    {
                        y += 1;
                    }
                    spans.push((self.bounds.y as i32 + start, self.bounds.y as i32 + y));
                }
                y += 1;
            }
            self.spans.cols[x as usize] = spans;
        }
    }

    fn rehash_tiles(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let tx0 = (x0 / TILE).clamp(0, self.tiles_x - 1);
        let tx1 = (x1 / TILE).clamp(0, self.tiles_x - 1);
        let ty0 = (y0 / TILE).clamp(0, self.tiles_y - 1);
        let ty1 = (y1 / TILE).clamp(0, self.tiles_y - 1);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                self.rehash_tile(tx, ty);
            }
        }
    }

    fn rehash_tile(&mut self, tx: i32, ty: i32) {
        let bx = tx * TILE;
        let by = ty * TILE;
        let mut h = Fnv::new();
        for y in by..(by + TILE).min(self.h) {
            for x in bx..(bx + TILE).min(self.w) {
                let i = (y as usize) * (self.w as usize) + x as usize;
                h.u8(self.cells[i]);
                h.u8(self.hp[i]);
            }
        }
        self.tile_sig[(ty as usize) * (self.tiles_x as usize) + tx as usize] = h.finish();
    }
}

/// Paints one brush's cells, with rock overriding granular overriding dirt.
fn rasterize(cells: &mut [u8], w: i32, h: i32, origin: V2, wall: &Wall) {
    if wall.material == MAT_EMPTY {
        return;
    }
    let material = wall.material;
    let mut paint = |x: i32, y: i32| {
        let i = (y as usize) * (w as usize) + x as usize;
        if material_overrides(material, cells[i]) {
            cells[i] = material;
        }
    };
    match &wall.shape {
        Shape::Poly(points) => scanline_cells(points, origin, w, h, &mut paint),
        Shape::Disc { center, radius } => {
            stamp_disc(&mut paint, w, h, *center - origin, *radius);
        }
        Shape::Chain { points, radius } => {
            for s in sweep(points, (radius * SWEEP_STEP).max(1.0)) {
                stamp_disc(&mut paint, w, h, s - origin, *radius);
            }
        }
        Shape::Border { thickness } => {
            let t = thickness.round() as i32;
            for y in 0..h {
                for x in 0..w {
                    if x < t || y < t || x >= w - t || y >= h - t {
                        paint(x, y);
                    }
                }
            }
        }
        Shape::Blob {
            points,
            radius,
            lumps,
            seed,
        } => {
            // The core keeps the mass in one piece; the lumps give it its outline.
            for s in sweep(points, (radius * SWEEP_STEP).max(1.0)) {
                stamp_disc(&mut paint, w, h, s - origin, radius * BLOB_CORE);
            }
            let len = polyline_length(points);
            let n = match *lumps {
                0 => (len / (radius * 0.45)).ceil().max(3.0) as u32,
                n => n,
            };
            for i in 0..=n {
                let t = i as f32 / n as f32;
                let (p, dir) = point_at(points, t * len);
                let perp = V2::new(-dir.y, dir.x);
                let (j1, j2, j3) = (
                    hash01(*seed, i as i32, 1),
                    hash01(*seed, i as i32, 2),
                    hash01(*seed, i as i32, 3),
                );
                let c = p + perp * ((j2 - 0.5) * radius * 0.9) + dir * ((j3 - 0.5) * radius * 0.5);
                stamp_disc(&mut paint, w, h, c - origin, radius * (0.45 + 0.55 * j1));
            }
        }
    }
}

/// Fills the grid's outer ring with rock: the cave's structural casing.
fn paint_casing(cells: &mut [u8], w: i32, h: i32, casing: i32) {
    for y in 0..h {
        for x in 0..w {
            if x >= casing && y >= casing && x < w - casing && y < h - casing {
                continue;
            }
            cells[(y as usize) * (w as usize) + x as usize] = MAT_ROCK;
        }
    }
}

/// Paints one disc: every cell whose *centre* lies within `radius` of `c`.
///
/// `c` is in cell-index space, like the scanline rasterizer's coordinates: the
/// cell with index `i` has its centre at `i + 0.5`.
fn stamp_disc(paint: &mut impl FnMut(i32, i32), w: i32, h: i32, c: V2, radius: f32) {
    if !(c.is_finite() && radius > 0.0) {
        return;
    }
    let row0 = ((c.y - radius).ceil() as i32).max(0);
    let row1 = ((c.y + radius).floor() as i32).min(h - 1);
    for y in row0..=row1 {
        let dy = (y as f32 + 0.5) - c.y;
        let half = radius * radius - dy * dy;
        if half <= 0.0 {
            continue;
        }
        let half = half.sqrt();
        let x0 = ((c.x - half - 0.5).ceil() as i32).max(0);
        let x1 = ((c.x + half - 0.5).floor() as i32).min(w - 1);
        for x in x0..=x1 {
            paint(x, y);
        }
    }
}

/// World positions along a polyline, `step` apart, both ends included.
fn sweep(points: &[V2], step: f32) -> Vec<V2> {
    let len = polyline_length(points);
    let step = step.max(0.5);
    let mut out = Vec::with_capacity((len / step) as usize + 2);
    let mut s = 0.0;
    while s < len {
        out.push(point_at(points, s).0);
        s += step;
    }
    out.push(point_at(points, len).0);
    out
}

/// Total length of a polyline.
fn polyline_length(points: &[V2]) -> f32 {
    let mut len = 0.0;
    for w in points.windows(2) {
        len += (w[1] - w[0]).len();
    }
    len
}

/// World position and unit tangent at arc length `s` along a polyline.
fn point_at(points: &[V2], s: f32) -> (V2, V2) {
    let n = points.len();
    if n == 0 {
        return (V2::default(), V2::new(1.0, 0.0));
    }
    if n == 1 {
        return (points[0], V2::new(1.0, 0.0));
    }
    let mut left = s.max(0.0);
    for i in 0..n - 1 {
        let (a, b) = (points[i], points[i + 1]);
        let seg = (b - a).len();
        if left <= seg || i == n - 2 {
            if seg <= 0.0 {
                return (a, V2::new(1.0, 0.0));
            }
            return (a + (b - a) * (left / seg).clamp(0.0, 1.0), (b - a) / seg);
        }
        left -= seg;
    }
    (points[n - 1], V2::new(1.0, 0.0))
}

/// One lump's pseudo-random number in `[0, 1)`. Integer hashing only, so a
/// mass's lumps are the same on every machine and every run.
fn hash01(seed: u32, i: i32, k: u32) -> f32 {
    let mut h = Fnv::new();
    h.u32(seed);
    h.u32(i as u32);
    h.u32(k);
    (h.finish() >> 40) as f32 / (1u32 << 24) as f32
}

/// Visits every cell whose centre lies inside `poly`, row by row.
///
/// Levels, terrain materials and water pools all rasterize through this, so
/// there is exactly one implementation of "what does this polygon cover".
pub fn scanline_cells<F: FnMut(i32, i32)>(poly: &[V2], origin: V2, w: i32, h: i32, f: &mut F) {
    if poly.len() < 3 {
        return;
    }
    let mut lo = f32::MAX;
    let mut hi = -f32::MAX;
    for p in poly {
        if !p.is_finite() {
            return;
        }
        lo = lo.min(p.y);
        hi = hi.max(p.y);
    }
    let row0 = ((lo - origin.y).ceil() as i32).max(0);
    let row1 = (hi - origin.y).floor() as i32;
    let row1 = row1.min(h - 1);
    if row1 < row0 {
        return;
    }

    let mut xs: Vec<f32> = Vec::new();
    for row in row0..=row1 {
        let y = origin.y + row as f32 + 0.5;
        xs.clear();
        for i in 0..poly.len() {
            let a = poly[i];
            let b = poly[(i + 1) % poly.len()];
            if (a.y > y) != (b.y > y) {
                xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
        xs.sort_by(|m, n| m.partial_cmp(n).unwrap_or(std::cmp::Ordering::Equal));
        let mut k = 0;
        while k + 1 < xs.len() {
            // A cell belongs to the polygon when its *centre* is inside it, so
            // the ends come back half a cell before the crossing.
            let left = ((xs[k] - origin.x - 0.5).ceil() as i32).max(0);
            let right = ((xs[k + 1] - origin.x - 0.5).floor() as i32).min(w - 1);
            for x in left..=right {
                f(x, row);
            }
            k += 2;
        }
    }
}

/// Per-column solid runs, in world y.
fn bake_spans(cells: &[u8], w: i32, h: i32, origin: V2) -> Spans {
    let mut cols = Vec::with_capacity(w as usize);
    for x in 0..w {
        let mut spans: Vec<(i32, i32)> = Vec::new();
        let mut y = 0;
        while y < h {
            if cells[(y as usize) * (w as usize) + x as usize] != MAT_EMPTY {
                let start = y;
                while y + 1 < h
                    && cells[((y + 1) as usize) * (w as usize) + x as usize] != MAT_EMPTY
                {
                    y += 1;
                }
                spans.push((origin.y as i32 + start, origin.y as i32 + y));
            }
            y += 1;
        }
        cols.push(spans);
    }
    Spans {
        x0: origin.x as i32,
        cols,
    }
}

/// Brightness for every solid cell: fine grain over a coarse clump, so packed
/// earth reads as dirt rather than a flat fill.
fn make_shade(cells: &[u8], w: i32, h: i32) -> Vec<u8> {
    let mut out = vec![0u8; cells.len()];
    for y in 0..h {
        for x in 0..w {
            let i = (y as usize) * (w as usize) + x as usize;
            if cells[i] == MAT_EMPTY {
                continue;
            }
            out[i] = grain(x, y);
        }
    }
    out
}

/// Hit points for every cell: dirt one, granular walls many, rock and air zero.
fn make_hp(cells: &[u8]) -> Vec<u8> {
    cells
        .iter()
        .map(|m| match *m {
            MAT_DIRT => crate::sim::tuning::DIRT_HP,
            MAT_GRANULAR => crate::sim::tuning::GRANULAR_HP,
            _ => 0,
        })
        .collect()
}

fn grain(x: i32, y: i32) -> u8 {
    let fine = hash2(x, y, SHADE_SEED) & 0xFF;
    let clump = hash2(x >> 3, y >> 3, SHADE_SEED ^ 0x9e37_79b9) & 0xFF;
    let v = (fine * 2 + clump) / 3;
    (40 + v * 3 / 4) as u8
}

fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b1)
        ^ (y as u32).wrapping_mul(0x85eb_ca6b)
        ^ seed.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_f491);
    h ^= h >> 13;
    h
}

/// The `k`-th of the `8 * r` cells on the square ring of radius `r`.
fn ring_point(cx: i32, cy: i32, r: i32, k: i32) -> (i32, i32) {
    let side = 2 * r;
    match k / side {
        0 => (cx - r + k % side, cy - r),
        1 => (cx + r, cy - r + k % side),
        2 => (cx + r - k % side, cy + r),
        _ => (cx - r, cy + r - k % side),
    }
}

/// Bounding box of a polygon.
pub fn bbox_of(poly: &[V2]) -> Rect {
    let mut r = Rect::from_corners(poly[0], poly[0]);
    for p in poly.iter().skip(1) {
        r = r.union(&Rect::from_corners(*p, *p));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_wall(x0: f32, y0: f32, x1: f32, y1: f32, material: u8) -> Wall {
        Wall::new(
            vec![
                V2::new(x0, y0),
                V2::new(x1, y0),
                V2::new(x1, y1),
                V2::new(x0, y1),
            ],
            material,
        )
    }

    /// A floor from x=0..200 at y=100..120, with a rock blade driven through it.
    ///
    /// The ceiling and the lower floor are there so the grid has open air above
    /// and below the floor: the casing closes the grid's outer ring, so a
    /// fixture that is all floor has no room left to test a surface or a roof in.
    fn test_terrain() -> Terrain {
        Terrain::new(
            &[
                rect_wall(0.0, 80.0, 200.0, 82.0, MAT_ROCK),
                rect_wall(0.0, 100.0, 200.0, 120.0, MAT_DIRT),
                rect_wall(90.0, 100.0, 110.0, 120.0, MAT_ROCK),
                rect_wall(0.0, 138.0, 200.0, 140.0, MAT_ROCK),
            ],
            1,
        )
    }

    #[test]
    fn rasterizes_brushes_with_rock_over_dirt() {
        let t = test_terrain();
        assert!(t.point_solid(V2::new(50.0, 110.0)));
        assert!(!t.point_solid(V2::new(50.0, 90.0)));
        assert_eq!(t.cell(50, 110), MAT_DIRT);
        assert_eq!(t.cell(100, 110), MAT_ROCK);
        assert_eq!(t.cell(50, 90), MAT_EMPTY);
    }

    #[test]
    fn carving_removes_dirt_and_leaves_rock() {
        let mut t = test_terrain();
        let carved = t.carve(V2::new(60.0, 110.0), 6.0);
        assert!(carved.hit_terrain());
        assert_eq!(carved.material, MAT_DIRT);
        assert!(!t.point_solid(V2::new(60.0, 110.0)));

        let rock = t.carve(V2::new(100.0, 110.0), 6.0);
        assert_eq!(rock.removed, 0, "blasts must not move stone");
        assert!(t.point_solid(V2::new(100.0, 110.0)));
    }

    #[test]
    fn carving_updates_the_render_spans() {
        let mut t = test_terrain();
        let before: Vec<(i32, i32)> = t.spans().column(60).to_vec();
        assert!(!before.is_empty());
        t.carve(V2::new(60.5, 105.0), 10.0);
        let after = t.spans().column(60).to_vec();
        assert_ne!(before, after, "the filled column must be re-baked");
        assert!(!t.point_solid(V2::new(60.5, 105.0)));
    }

    #[test]
    fn surface_and_roof_follow_the_dig() {
        let mut t = test_terrain();
        assert!(t.surface(50, 100), "the floor's top cell is exposed");
        assert!(t.roof(50, 119), "the floor's bottom cell is exposed");
        assert!(!t.surface(50, 110));
        t.carve(V2::new(50.5, 100.5), 4.0);
        assert!(!t.solid(50, 100));
    }

    #[test]
    fn a_fast_segment_cannot_tunnel_through_a_wall() {
        let t = test_terrain();
        // 40 px of travel in one tick through a 20 px slab.
        let hit = t.segment_hit(V2::new(50.0, 90.0), V2::new(50.0, 130.0));
        let hit = hit.expect("the floor is in the way");
        assert!(
            hit.p.y <= 100.5,
            "hit at {:?}, expected the near face",
            hit.p
        );
        assert!(hit.normal.y < 0.0, "normal points back at the traveller");
        assert!(
            t.segment_hit(V2::new(50.0, 90.0), V2::new(50.0, 95.0))
                .is_none()
        );
    }

    #[test]
    fn penetration_pushes_a_buried_body_back_the_way_it_came() {
        let t = test_terrain();
        // Just inside the top face, moving down.
        let hit = t
            .penetration(V2::new(50.0, 102.0), V2::new(0.0, 120.0))
            .expect("inside the rock");
        assert!(
            hit.normal.y < 0.0,
            "must escape upwards, got {:?}",
            hit.normal
        );
        assert!(hit.p.y >= 100.0 && hit.p.y <= 120.0);
        assert!(hit.depth > 0.0);
        assert!(t.penetration(V2::new(50.0, 90.0), V2::ZERO).is_none());
    }

    #[test]
    fn digest_tracks_the_shape_of_the_cave() {
        let mut t = test_terrain();
        let before = t.digest();
        assert_eq!(before, t.clone().digest());
        t.carve(V2::new(60.0, 110.0), 5.0);
        assert_ne!(before, t.digest());
        let after = t.digest();
        // Digging rock changes nothing, so the digest must not move either.
        t.carve(V2::new(100.0, 110.0), 5.0);
        assert_eq!(after, t.digest());
    }

    #[test]
    fn grain_is_stable_and_spans_the_expected_range() {
        assert_eq!(grain(31, 47), grain(31, 47));
        assert_ne!(grain(31, 47), grain(32, 47));
        for i in 0..512 {
            let g = grain(i, i * 7);
            assert!((40..=231).contains(&g), "grain out of range: {g}");
        }
    }

    /// Open air around a mass, so the casing does not answer for it.
    fn cave(brush: Wall) -> Terrain {
        Terrain::new(&[rect_wall(0.0, 0.0, 20.0, 20.0, MAT_ROCK), brush], 1)
    }

    #[test]
    fn a_disc_brush_is_round() {
        let t = cave(Wall::disc(V2::new(100.0, 100.0), 30.0, MAT_DIRT));
        assert!(t.point_solid(V2::new(100.5, 100.5)), "the centre is filled");
        assert!(t.point_solid(V2::new(128.5, 100.5)), "out to the radius");
        assert!(!t.point_solid(V2::new(132.0, 100.5)), "and no further");
        // The corners of the bounding box stay open: this is a disc, not a box.
        assert!(!t.point_solid(V2::new(121.0, 121.0)));
    }

    #[test]
    fn a_chain_brush_joins_its_ends_and_a_blob_is_lumpy() {
        let ends = [V2::new(40.0, 40.0), V2::new(160.0, 90.0)];
        let chain = cave(Wall::chain(ends.to_vec(), 10.0, MAT_DIRT));
        for p in ends {
            assert!(chain.point_solid(p), "a chain covers {p:?}");
        }
        assert!(
            chain.point_solid(V2::new(100.0, 65.5)),
            "a chain covers the span between its ends"
        );

        // A blob is the same spine with lumps: thicker than the chain somewhere,
        // and never in pieces.
        let blob = cave(Wall::blob(ends.to_vec(), 30.0, 24, 7, MAT_DIRT));
        let width = |t: &Terrain, x: i32| {
            (0..300)
                .filter(|y| t.point_solid(V2::new(x as f32, *y as f32 + 0.5)))
                .count()
        };
        let widths: Vec<usize> = (40..160).step_by(5).map(|x| width(&blob, x)).collect();
        assert!(
            widths.iter().all(|w| *w > 20),
            "the blob has a body everywhere: {widths:?}"
        );
        assert!(
            widths.iter().min() != widths.iter().max(),
            "a blob's outline varies along its spine: {widths:?}"
        );
        // Lumps are seeded, not random per run.
        let again = cave(Wall::blob(ends.to_vec(), 30.0, 24, 7, MAT_DIRT));
        assert_eq!(blob.digest(), again.digest());
        let rerolled = cave(Wall::blob(ends.to_vec(), 30.0, 24, 8, MAT_DIRT));
        assert_ne!(blob.digest(), rerolled.digest(), "the seed picks the lumps");
    }

    #[test]
    fn the_casing_seals_the_grid_and_is_never_dug() {
        let mut t = cave(Wall::disc(V2::new(100.0, 100.0), 20.0, MAT_DIRT));
        let b = t.bounds;
        let (x0, y0) = (b.x as i32, b.y as i32);
        let (x1, y1) = (b.right() as i32 - 1, b.bottom() as i32 - 1);
        for x in x0..=x1 {
            assert_eq!(t.cell(x, y0), MAT_ROCK, "outer ring cell ({x}, {y0})");
            assert_eq!(t.cell(x, y1), MAT_ROCK, "outer ring cell ({x}, {y1})");
        }
        for y in y0..=y1 {
            assert_eq!(t.cell(x0, y), MAT_ROCK, "outer ring cell ({x0}, {y})");
            assert_eq!(t.cell(x1, y), MAT_ROCK, "outer ring cell ({x1}, {y})");
        }
        // And a blast against it removes nothing: the way out is not dug.
        let carved = t.carve(V2::new(b.x + 2.0, b.y + 2.0), 40.0);
        assert_eq!(carved.removed, 0, "the casing is stone");
    }

    #[test]
    fn granular_walls_take_several_hits_while_dirt_opens_at_once() {
        let mut t = Terrain::new(
            &[
                rect_wall(0.0, 0.0, 400.0, 60.0, MAT_DIRT),
                rect_wall(0.0, 60.0, 400.0, 140.0, MAT_GRANULAR),
            ],
            1,
        );
        let dirt = V2::new(200.0, 30.0);
        let gran = V2::new(200.0, 100.0);
        assert_eq!(t.material_at(dirt), MAT_DIRT);
        assert_eq!(t.material_at(gran), MAT_GRANULAR);

        // One point of damage opens dirt...
        let c = t.damage(dirt, 3.0, 1);
        assert!(c.removed > 0, "dirt opens on the first hit");
        assert_eq!(t.material_at(dirt), MAT_EMPTY);

        // ...but only chips a granular wall, which needs its full hit points.
        let hp = t.hp_at(gran);
        assert_eq!(hp, crate::sim::tuning::GRANULAR_HP);
        let first = t.damage(gran, 6.0, 1);
        assert_eq!(first.removed, 0, "the wall survives one hit");
        assert!(first.damaged > 0, "but it does take the damage");
        assert_eq!(t.hp_at(gran), hp - 1);
        assert!(t.solid_at(gran), "and it is still there");
        for _ in 0..(hp - 1) {
            t.damage(gran, 6.0, 1);
        }
        assert_eq!(t.material_at(gran), MAT_EMPTY, "enough hits open it");
    }

    #[test]
    fn rock_never_takes_damage() {
        let mut t = Terrain::new(&[rect_wall(0.0, 0.0, 200.0, 200.0, MAT_ROCK)], 1);
        let c = t.damage(V2::new(100.0, 100.0), 40.0, u8::MAX);
        assert_eq!(c.removed, 0);
        assert_eq!(c.damaged, 0);
        assert!(t.solid_at(V2::new(100.0, 100.0)));
    }

    #[test]
    fn the_digest_follows_chipped_walls() {
        let mut t = Terrain::new(&[rect_wall(0.0, 0.0, 200.0, 120.0, MAT_GRANULAR)], 1);
        let before = t.digest();
        t.damage(V2::new(100.0, 60.0), 8.0, 1);
        assert_ne!(before, t.digest(), "a chipped wall is a different cave");
        assert!(
            t.solid_at(V2::new(100.0, 60.0)),
            "the cell survived the chip"
        );
    }
}
