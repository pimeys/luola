//! Data-driven level definitions and validation.
//!
//! Levels are TOML: terrain as brushes (polygons, discs, swept chains and lumpy
//! blobs), furniture as object lists, plus per-level physics properties
//! (gravity, starting fuel, wind). Everything an editor would eventually write
//! lives in this file format, so the runtime never needs a format migration
//! (`docs/game_mechanics.md` §12).

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::math::{Rect, V2};
use crate::sim::terrain::{CASING, MAT_DIRT, MAT_GRANULAR, MAT_ROCK, Terrain, Wall};
use crate::sim::water::WaterParams;
use crate::sim::weapons::WeaponId;

const DEFAULT_GRAVITY: f32 = 90.0;
const DEFAULT_FUEL: f32 = 100.0;
const DEFAULT_FUEL_AMOUNT: f32 = 40.0;
const DEFAULT_PAD_SIZE: [f32; 2] = [56.0, 10.0];
/// A pad's rectangle is the surface it is bolted to; the landing zone reaches
/// this far up from it, because the ship has to hover with its centre inside and
/// its hull clear of the rock.
const PAD_ZONE_HEIGHT: f32 = 26.0;
const DEFAULT_EXIT_SIZE: [f32; 2] = [56.0, 56.0];

#[derive(Debug)]
pub struct LevelError {
    pub path: Option<PathBuf>,
    pub message: String,
}

impl LevelError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            path: None,
            message: message.into(),
        }
    }

    fn at(mut self, path: &Path) -> Self {
        self.path = Some(path.to_path_buf());
        self
    }
}

impl fmt::Display for LevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(p) => write!(f, "{}: {}", p.display(), self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for LevelError {}

fn default_gravity() -> f32 {
    DEFAULT_GRAVITY
}

fn default_fuel() -> f32 {
    DEFAULT_FUEL
}

fn default_fuel_amount() -> f32 {
    DEFAULT_FUEL_AMOUNT
}

fn default_pad_size() -> [f32; 2] {
    DEFAULT_PAD_SIZE
}

fn default_exit_size() -> [f32; 2] {
    DEFAULT_EXIT_SIZE
}

fn default_gate_size() -> [f32; 2] {
    [crate::sim::tuning::GATE_SIZE; 2]
}

fn default_gate_speed() -> f32 {
    crate::sim::tuning::GATE_SPEED
}

fn default_water_density() -> f32 {
    crate::sim::tuning::WATER_DENSITY
}

fn default_water_drag() -> f32 {
    crate::sim::tuning::WATER_DRAG
}

/// Terrain material of a brush: `dirt` digs, `rock` does not.
fn parse_material(name: Option<&str>) -> Result<u8, LevelError> {
    match name.unwrap_or("dirt") {
        "dirt" | "soil" => Ok(MAT_DIRT),
        "rock" | "stone" => Ok(MAT_ROCK),
        "granular" | "gravel" | "sand" => Ok(MAT_GRANULAR),
        other => Err(LevelError::new(format!(
            "unknown material `{other}`; use `dirt`, `granular` or `rock`"
        ))),
    }
}

/// Turns one authored brush into geometry, checking that the kind got the fields
/// it needs and no others: a disc with `points` is a load error, not a silently
/// empty brush.
fn to_wall(def: &PolyDef, material: u8) -> Result<Wall, String> {
    let kind = BrushKind::parse(def.kind.as_deref())?;
    // A kind that quietly ignores a field is a brush that silently does nothing,
    // which is the worst thing an authoring format can do to level design.
    if !kind.takes_points() && !def.points.is_empty() {
        return Err(format!("has kind `{kind}`, which takes no `points`"));
    }
    if kind != BrushKind::Disc && def.center.is_some() {
        return Err(format!("has kind `{kind}`, which takes no `center`"));
    }
    if !kind.takes_radius() && def.radius.is_some() {
        return Err(format!("has kind `{kind}`, which takes no `radius`"));
    }
    if kind != BrushKind::Border && def.thickness.is_some() {
        return Err(format!("has kind `{kind}`, which takes no `thickness`"));
    }
    if kind != BrushKind::Blob && (def.lumps != 0 || def.seed != 0) {
        return Err(format!(
            "has kind `{kind}`, which takes no `lumps` or `seed`"
        ));
    }

    let to_points = || -> Result<Vec<V2>, String> {
        if let Some(bad) = def
            .points
            .iter()
            .find(|p| !p[0].is_finite() || !p[1].is_finite())
        {
            return Err(format!("has a non-finite point {:?}", (bad[0], bad[1])));
        }
        Ok(def.points.iter().map(|p| V2::new(p[0], p[1])).collect())
    };
    let radius = || -> Result<f32, String> {
        match def.radius {
            Some(r) if r.is_finite() && r > 0.0 => Ok(r),
            Some(r) => Err(format!("has radius {r}; a radius must be positive")),
            None => Err("has no `radius`".to_string()),
        }
    };
    match kind {
        BrushKind::Poly => {
            let points = to_points()?;
            if points.len() < 3 {
                return Err(format!(
                    "has {} point(s); a polygon needs at least 3",
                    points.len()
                ));
            }
            Ok(Wall::new(points, material))
        }
        BrushKind::Disc => {
            let c = def.center.ok_or_else(|| "has no `center`".to_string())?;
            if !c[0].is_finite() || !c[1].is_finite() {
                return Err(format!("has a non-finite centre {:?}", (c[0], c[1])));
            }
            Ok(Wall::disc(V2::new(c[0], c[1]), radius()?, material))
        }
        BrushKind::Border => {
            let thickness = def
                .thickness
                .filter(|t| t.is_finite() && *t > 0.0)
                .ok_or_else(|| "has no positive `thickness`".to_string())?;
            Ok(Wall::border(thickness, material))
        }
        swept @ (BrushKind::Chain | BrushKind::Blob) => {
            let points = to_points()?;
            if points.len() < 2 {
                return Err(format!(
                    "has {} point(s); a `{swept}` is swept along at least 2",
                    points.len()
                ));
            }
            let r = radius()?;
            Ok(if swept == BrushKind::Chain {
                Wall::chain(points, r, material)
            } else {
                Wall::blob(points, r, def.lumps, def.seed, material)
            })
        }
    }
}

/// The four shapes a brush can be, parsed once so the geometry match below is
/// exhaustive and an unknown name fails with the list of names that work.
#[derive(Clone, Copy, Debug, PartialEq)]
enum BrushKind {
    Poly,
    Disc,
    Chain,
    Blob,
    Border,
}

impl BrushKind {
    fn parse(name: Option<&str>) -> Result<Self, String> {
        match name.unwrap_or("poly") {
            "poly" => Ok(Self::Poly),
            "disc" => Ok(Self::Disc),
            "chain" => Ok(Self::Chain),
            "blob" => Ok(Self::Blob),
            "border" => Ok(Self::Border),
            other => Err(format!(
                "has kind `{other}`; expected `poly`, `disc`, `chain`, `blob` or `border`"
            )),
        }
    }

    fn takes_points(self) -> bool {
        matches!(self, Self::Poly | Self::Chain | Self::Blob)
    }

    fn takes_radius(self) -> bool {
        matches!(self, Self::Disc | Self::Chain | Self::Blob)
    }
}

impl std::fmt::Display for BrushKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Poly => "poly",
            Self::Disc => "disc",
            Self::Chain => "chain",
            Self::Blob => "blob",
            Self::Border => "border",
        })
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct LevelDef {
    pub name: String,
    #[serde(default)]
    pub briefing: Option<String>,
    #[serde(default = "default_gravity")]
    pub gravity: f32,
    #[serde(default = "default_fuel")]
    pub fuel: f32,
    #[serde(default)]
    pub wind: [f32; 2],
    /// The payload must be carried to the exit to finish the run.
    #[serde(default)]
    pub require_pod: bool,
    /// The exit stays shut until the reactor is destroyed.
    #[serde(default)]
    pub exit_locked: bool,
    pub player: ObjectDef,
    #[serde(default)]
    pub wall: Vec<PolyDef>,
    #[serde(default)]
    pub liquid: Vec<PoolDef>,
    /// How the water in this level feels to a submerged body.
    #[serde(default = "default_water_density")]
    pub water_density: f32,
    #[serde(default = "default_water_drag")]
    pub water_drag: f32,
    #[serde(default)]
    pub turret: Vec<ObjectDef>,
    #[serde(default)]
    pub drone: Vec<ObjectDef>,
    #[serde(default)]
    pub mine: Vec<ObjectDef>,
    #[serde(default)]
    pub reactor: Vec<ObjectDef>,
    #[serde(default)]
    pub pod: Vec<ObjectDef>,
    #[serde(default)]
    pub fuel_pod: Vec<FuelDef>,
    #[serde(default)]
    pub pad: Vec<PadDef>,
    #[serde(default)]
    pub exit: Vec<ExitDef>,
    /// Moving geometry: gates and crushers.
    #[serde(default)]
    pub gate: Vec<GateDef>,
    /// The level's signal graph.
    #[serde(default)]
    pub signal: Vec<SignalDef>,
    /// Which of Wings' specials this cave allows, and which one the ship starts
    /// with. Absent means all 33, starting at the first (`docs/design.md` §13).
    #[serde(default)]
    pub weapons: Option<WeaponsDef>,
}

/// `[weapons]` — the level's `W_SELECT.DAT`: the weapons a run may use, and the
/// one flagged "1" in Wings' own table, the one already mounted at launch.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct WeaponsDef {
    #[serde(default)]
    pub available: Vec<String>,
    #[serde(default)]
    pub start: Option<String>,
}

/// One terrain brush.
///
/// `kind` picks the geometry: `poly` (the default) for cut rooms and shafts,
/// `disc` for the blobs a cave is built from, `chain` for a disc swept along a
/// spine, `blob` for a lumpy mass, `border` for the grid's own margin. Which
/// fields each kind needs — and which it must not carry — is checked in
/// [`Level::from_def`].
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct PolyDef {
    /// `poly` (the default), `disc`, `chain`, `blob` or `border`.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    /// A `disc`'s centre.
    #[serde(default)]
    pub center: Option<[f32; 2]>,
    /// A `disc`'s radius, or a `chain`/`blob`'s sweep radius.
    #[serde(default)]
    pub radius: Option<f32>,
    /// How deep a `border` fills the grid's margin, in pixels.
    #[serde(default)]
    pub thickness: Option<f32>,
    /// How many lumps a `blob` scatters along its spine; 0 sizes them from it.
    #[serde(default)]
    pub lumps: u32,
    /// Which lumps a `blob` scatters, so an author can reroll a mass's outline
    /// without moving it.
    #[serde(default)]
    pub seed: u32,
    /// `dirt` (the default, and diggable) or `rock` (permanent).
    #[serde(default)]
    pub material: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ObjectDef {
    pub pos: [f32; 2],
    #[serde(default)]
    pub angle: f32,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct FuelDef {
    pub pos: [f32; 2],
    #[serde(default = "default_fuel_amount")]
    pub amount: f32,
}

/// Landing pad: wide and flat by default.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct PadDef {
    pub pos: [f32; 2],
    #[serde(default = "default_pad_size")]
    pub size: [f32; 2],
}

/// Exit zone: square by default, so a ship can arrive from any direction.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ExitDef {
    pub pos: [f32; 2],
    #[serde(default = "default_exit_size")]
    pub size: [f32; 2],
    /// A hidden exit is drawn (and usable) only once a signal reveals it.
    #[serde(default)]
    pub hidden: bool,
}

/// A gate or crusher: a solid slab that slides between `pos` and `to`.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GateDef {
    /// Start centre.
    pub pos: [f32; 2],
    #[serde(default = "default_gate_size")]
    pub size: [f32; 2],
    /// End centre; omitted means a stationary slab.
    #[serde(default)]
    pub to: Option<[f32; 2]>,
    #[serde(default = "default_gate_speed")]
    pub speed: f32,
    /// `always` (the default) or `reactor`: what powers the gate.
    #[serde(default)]
    pub trigger: Option<String>,
    /// A hidden gate is invisible until it activates.
    #[serde(default)]
    pub hidden: bool,
    /// Starting position along the path, 0..1.
    #[serde(default)]
    pub phase: f32,
}

/// One edge of the level's signal graph: when `when`, do `action` to `index`.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct SignalDef {
    /// `reactor` (the trigger the simulation raises).
    pub when: String,
    /// `open_exit`, `reveal_exit`, `start_gate` or `power_down`.
    pub action: String,
    /// Which exit or gate the action applies to; 0 for `power_down`.
    #[serde(default)]
    pub index: usize,
}

/// What brings a gate (or a signal) to life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// Live from the first tick.
    Always,
    /// Powered when the level's reactor is destroyed.
    Reactor,
}

impl Trigger {
    pub fn parse(name: Option<&str>) -> Result<Self, String> {
        match name.unwrap_or("always") {
            "always" | "start" => Ok(Self::Always),
            "reactor" => Ok(Self::Reactor),
            other => Err(format!(
                "unknown trigger `{other}`; expected `always` or `reactor`"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Reactor => "reactor",
        }
    }
}

/// What a signal does when its trigger fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalAction {
    /// Opens and reveals an exit.
    OpenExit,
    /// Reveals an exit without necessarily opening it.
    RevealExit,
    /// Powers a gate on.
    StartGate,
    /// Cuts power to every turret.
    PowerDown,
}

impl SignalAction {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "open_exit" | "open" => Ok(Self::OpenExit),
            "reveal_exit" | "reveal" => Ok(Self::RevealExit),
            "start_gate" | "start" => Ok(Self::StartGate),
            "power_down" | "power" => Ok(Self::PowerDown),
            other => Err(format!(
                "unknown action `{other}`; expected `open_exit`, `reveal_exit`, `start_gate` or `power_down`"
            )),
        }
    }
}

/// One gate, resolved from `[[gate]]` and ready for the simulation.
#[derive(Clone, Copy, Debug)]
pub struct GateSpawn {
    pub from: V2,
    pub to: V2,
    pub size: V2,
    pub speed: f32,
    pub trigger: Trigger,
    pub hidden: bool,
    pub phase: f32,
}

/// One resolved signal-graph edge.
#[derive(Clone, Copy, Debug)]
pub struct Signal {
    pub when: Trigger,
    pub action: SignalAction,
    pub index: usize,
}

/// A body of water: the cells inside this polygon start full.
///
/// Water then behaves like water — a pool only stays where the rock holds it
/// (`sim::water`).
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct PoolDef {
    pub points: Vec<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spawn {
    pub pos: V2,
    pub angle: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub rect: Rect,
}

#[derive(Clone, Debug)]
pub struct FuelSpawn {
    pub pos: V2,
    pub amount: f32,
}

#[derive(Clone, Debug)]
pub struct Level {
    pub source: Option<PathBuf>,
    pub name: String,
    pub briefing: String,
    pub gravity: f32,
    pub start_fuel: f32,
    pub wind: V2,
    pub require_pod: bool,
    pub exit_locked: bool,
    pub terrain: Terrain,
    /// Authored water bodies; the runtime seeds these into `World::water`.
    pub pools: Vec<Vec<V2>>,
    pub water: WaterParams,
    pub player: Spawn,
    pub turrets: Vec<Spawn>,
    pub drones: Vec<Spawn>,
    pub mines: Vec<Spawn>,
    pub reactors: Vec<Spawn>,
    pub pods: Vec<Spawn>,
    pub fuel_pods: Vec<FuelSpawn>,
    pub pads: Vec<Area>,
    pub exits: Vec<Area>,
    /// Per-exit authored hidden flag, parallel to `exits`.
    pub exit_hidden: Vec<bool>,
    /// Gates and crushers, resolved from the level's brushes.
    pub gates: Vec<GateSpawn>,
    /// The signal graph the reactor (and future triggers) drive.
    pub signals: Vec<Signal>,
    /// The specials a base in this level will hand out, in roster order.
    pub weapons: Vec<WeaponId>,
    /// The special already mounted at launch (Wings' weapon flagged "1").
    pub start_weapon: WeaponId,
    /// Playfield bounds: terrain bbox inflated by a margin. Used by the camera,
    /// the radar and the off-screen indicators.
    pub bounds: Rect,
}

impl Level {
    pub fn parse(text: &str) -> Result<Level, LevelError> {
        let def: LevelDef =
            toml::from_str(text).map_err(|e| LevelError::new(format!("parse error: {e}")))?;
        Level::from_def(def)
    }

    pub fn load(path: &Path) -> Result<Level, LevelError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| LevelError::new(format!("cannot read: {e}")).at(path))?;
        let mut level = Level::parse(&text).map_err(|e| e.at(path))?;
        level.source = Some(path.to_path_buf());
        Ok(level)
    }

    pub fn from_def(def: LevelDef) -> Result<Level, LevelError> {
        if def.name.trim().is_empty() {
            return Err(LevelError::new("`name` must not be empty"));
        }
        if !def.gravity.is_finite() || def.gravity <= 0.0 {
            return Err(LevelError::new("`gravity` must be a positive number"));
        }
        if !def.fuel.is_finite() || def.fuel <= 0.0 {
            return Err(LevelError::new("`fuel` must be a positive number"));
        }
        if def.wall.is_empty() {
            return Err(LevelError::new("the level has no `[[wall]]` brushes"));
        }
        for (i, wall) in def.wall.iter().enumerate() {
            to_wall(wall, MAT_DIRT).map_err(|e| LevelError::new(format!("wall #{i} {e}")))?;
        }
        if def.exit.is_empty() {
            return Err(LevelError::new("the level has no `[[exit]]` area"));
        }
        if def.require_pod && def.pod.is_empty() {
            return Err(LevelError::new(
                "`require_pod = true` but the level has no `[[pod]]`",
            ));
        }
        if def.exit_locked && def.reactor.is_empty() {
            return Err(LevelError::new(
                "`exit_locked = true` but the level has no `[[reactor]]`",
            ));
        }

        let to_vec = |points: &[[f32; 2]]| -> Vec<V2> {
            points.iter().map(|p| V2::new(p[0], p[1])).collect()
        };

        let mut walls = Vec::with_capacity(def.wall.len());
        for (i, wall) in def.wall.iter().enumerate() {
            let material = parse_material(wall.material.as_deref())?;
            walls.push(
                to_wall(wall, material).map_err(|e| LevelError::new(format!("wall #{i} {e}")))?,
            );
        }
        let terrain = Terrain::new(&walls, CASING);
        let player = to_spawn(&def.player);

        if terrain.point_solid(player.pos) {
            return Err(LevelError::new(format!(
                "player spawn {:?} is inside solid terrain",
                (player.pos.x, player.pos.y)
            )));
        }

        if !def.water_density.is_finite() || def.water_density < 0.0 {
            return Err(LevelError::new(
                "`water_density` must be a non-negative number",
            ));
        }
        if !def.water_drag.is_finite() || def.water_drag < 0.0 {
            return Err(LevelError::new(
                "`water_drag` must be a non-negative number",
            ));
        }

        let mut pools = Vec::with_capacity(def.liquid.len());
        for (i, pool) in def.liquid.iter().enumerate() {
            if pool.points.len() < 3 {
                return Err(LevelError::new(format!(
                    "liquid #{i} has {} point(s); a pool needs at least 3",
                    pool.points.len()
                )));
            }
            if pool
                .points
                .iter()
                .any(|p| !p[0].is_finite() || !p[1].is_finite())
            {
                return Err(LevelError::new(format!(
                    "liquid #{i} has a non-finite point"
                )));
            }
            let poly = to_vec(&pool.points);
            if !pool_holds_water(&terrain, &poly) {
                return Err(LevelError::new(format!(
                    "liquid #{i} is buried in solid terrain and would be invisible"
                )));
            }
            pools.push(poly);
        }

        let bounds = terrain.bounds.inflate(96.0);
        let (weapons, start_weapon) = resolve_weapons(def.weapons.as_ref())?;

        let exit_hidden: Vec<bool> = def.exit.iter().map(|e| e.hidden).collect();
        let mut gates = Vec::with_capacity(def.gate.len());
        for (i, g) in def.gate.iter().enumerate() {
            let size = V2::new(g.size[0], g.size[1]);
            if !(size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0) {
                return Err(LevelError::new(format!("gate #{i} has an invalid `size`")));
            }
            if !g.speed.is_finite() || g.speed < 0.0 {
                return Err(LevelError::new(format!("gate #{i} has an invalid `speed`")));
            }
            let from = V2::new(g.pos[0], g.pos[1]);
            let to = g.to.map(|t| V2::new(t[0], t[1])).unwrap_or(from);
            if !from.is_finite() || !to.is_finite() {
                return Err(LevelError::new(format!(
                    "gate #{i} has a non-finite endpoint"
                )));
            }
            let trigger = Trigger::parse(g.trigger.as_deref())
                .map_err(|e| LevelError::new(format!("gate #{i} {e}")))?;
            gates.push(GateSpawn {
                from,
                to,
                size,
                speed: g.speed,
                trigger,
                hidden: g.hidden,
                phase: g.phase,
            });
        }

        let mut signals = Vec::with_capacity(def.signal.len());
        for (i, s) in def.signal.iter().enumerate() {
            let when = Trigger::parse(Some(s.when.as_str()))
                .map_err(|e| LevelError::new(format!("signal #{i} `when`: {e}")))?;
            if when != Trigger::Reactor {
                return Err(LevelError::new(format!(
                    "signal #{i} `when` is `{}`; only `reactor` is wired up",
                    s.when
                )));
            }
            let action = SignalAction::parse(&s.action)
                .map_err(|e| LevelError::new(format!("signal #{i} `action`: {e}")))?;
            match action {
                SignalAction::OpenExit | SignalAction::RevealExit => {
                    if s.index >= def.exit.len() {
                        return Err(LevelError::new(format!(
                            "signal #{i} targets exit {} but the level has {}",
                            s.index,
                            def.exit.len()
                        )));
                    }
                }
                SignalAction::StartGate => {
                    if s.index >= gates.len() {
                        return Err(LevelError::new(format!(
                            "signal #{i} targets gate {} but the level has {}",
                            s.index,
                            gates.len()
                        )));
                    }
                }
                SignalAction::PowerDown => {}
            }
            signals.push(Signal {
                when,
                action,
                index: s.index,
            });
        }
        // A hidden exit nothing can reveal traps the pilot in a cave whose way
        // out does not exist. Refuse it at load, not at play.
        for (i, hidden) in exit_hidden.iter().enumerate() {
            if !*hidden {
                continue;
            }
            let revealed = signals.iter().any(|s| {
                matches!(s.action, SignalAction::OpenExit | SignalAction::RevealExit)
                    && s.index == i
            });
            if !revealed {
                return Err(LevelError::new(format!(
                    "exit #{i} is hidden but no signal reveals it"
                )));
            }
        }

        let level = Level {
            source: None,
            name: def.name,
            briefing: def.briefing.unwrap_or_default(),
            gravity: def.gravity,
            start_fuel: def.fuel,
            wind: V2::new(def.wind[0], def.wind[1]),
            require_pod: def.require_pod,
            exit_locked: def.exit_locked,
            terrain,
            pools,
            water: WaterParams {
                density: def.water_density,
                drag: def.water_drag,
            },
            player,
            turrets: def.turret.iter().map(to_spawn).collect(),
            drones: def.drone.iter().map(to_spawn).collect(),
            mines: def.mine.iter().map(to_spawn).collect(),
            reactors: def.reactor.iter().map(to_spawn).collect(),
            pods: def.pod.iter().map(to_spawn).collect(),
            fuel_pods: def
                .fuel_pod
                .iter()
                .map(|f| FuelSpawn {
                    pos: V2::new(f.pos[0], f.pos[1]),
                    amount: f.amount,
                })
                .collect(),
            pads: def.pad.iter().map(|a| pad_area(a.pos, a.size)).collect(),
            exits: def.exit.iter().map(|a| to_area(a.pos, a.size)).collect(),
            exit_hidden,
            gates,
            signals,
            weapons,
            start_weapon,
            bounds,
        };
        level.check_entities_are_in_the_open()?;
        Ok(level)
    }

    /// Nothing may be spawned inside a mass.
    ///
    /// A cave authored from discs is drawn over its furniture rather than around
    /// it, so this is the mistake that shape of authoring makes: a turret walled
    /// into rock, a core nobody can ever reach, a fuel pod that is simply not
    /// there. The validator names the object and the position.
    fn check_entities_are_in_the_open(&self) -> Result<(), LevelError> {
        let mut named: Vec<(&str, V2)> = Vec::new();
        named.extend(self.turrets.iter().map(|s| ("turret", s.pos)));
        named.extend(self.drones.iter().map(|s| ("drone", s.pos)));
        named.extend(self.mines.iter().map(|s| ("mine", s.pos)));
        named.extend(self.reactors.iter().map(|s| ("reactor", s.pos)));
        named.extend(self.pods.iter().map(|s| ("pod", s.pos)));
        named.extend(self.fuel_pods.iter().map(|f| ("fuel pod", f.pos)));
        for (what, pos) in named {
            if self.terrain.point_solid(pos) {
                return Err(LevelError::new(format!(
                    "{what} at ({:.0}, {:.0}) is inside solid terrain",
                    pos.x, pos.y
                )));
            }
        }
        Ok(())
    }

    /// Short human-readable summary used by `--validate`.
    pub fn summary(&self) -> String {
        format!(
            "{:<28} g={:<6.1} fuel={:<6.1} walls={} cells={} water={} turrets={} drones={} mines={} fuel={} pads={} exits={} gates={} signals={} pod={} reactor={} weapons={} start={} size={}x{}",
            self.name,
            self.gravity,
            self.start_fuel,
            self.terrain.walls.len(),
            self.terrain.solid_cells(),
            self.pools.len(),
            self.turrets.len(),
            self.drones.len(),
            self.mines.len(),
            self.fuel_pods.len(),
            self.pads.len(),
            self.exits.len(),
            self.gates.len(),
            self.signals.len(),
            self.pods.len(),
            self.reactors.len(),
            self.weapons.len(),
            crate::sim::weapons::spec(self.start_weapon).name,
            self.bounds.w.round() as i32,
            self.bounds.h.round() as i32,
        )
    }
}

/// Turns `[weapons]` into the list a base cycles and the one the ship starts
/// with.
///
/// Absent means "everything Wings ships": all 33 specials, starting with the
/// first. A name that is not in the roster is a load error rather than a level
/// where one weapon silently never appears, and a `start` that is not in
/// `available` is a level whose pilot launches with a weapon the cave forbids.
fn resolve_weapons(def: Option<&WeaponsDef>) -> Result<(Vec<WeaponId>, WeaponId), LevelError> {
    let Some(def) = def else {
        return Ok((
            crate::sim::weapons::SPECIALS.to_vec(),
            crate::sim::weapons::SPECIALS[0],
        ));
    };
    if def.available.is_empty() {
        return Err(LevelError::new(
            "`[weapons]` has an empty `available` list; drop the table to allow every weapon",
        ));
    }
    let mut weapons = Vec::with_capacity(def.available.len());
    for name in &def.available {
        let id = crate::sim::weapons::parse(name).ok_or_else(|| {
            LevelError::new(format!(
                "`[weapons] available` names `{name}`, which is not a weapon; try one of: {}",
                crate::sim::weapons::roster()
            ))
        })?;
        if weapons.contains(&id) {
            return Err(LevelError::new(format!(
                "`[weapons] available` lists `{name}` twice"
            )));
        }
        weapons.push(id);
    }
    let start = match def.start.as_deref() {
        Some(name) => {
            let id = crate::sim::weapons::parse(name).ok_or_else(|| {
                LevelError::new(format!(
                    "`[weapons] start` names `{name}`, which is not a weapon"
                ))
            })?;
            if !weapons.contains(&id) {
                return Err(LevelError::new(format!(
                    "`[weapons] start` is `{name}`, which `available` does not list"
                )));
            }
            id
        }
        None => weapons[0],
    };
    Ok((weapons, start))
}

/// True when a pool polygon covers at least one open cell, so it can hold water.
fn pool_holds_water(terrain: &Terrain, poly: &[V2]) -> bool {
    let origin = V2::new(terrain.bounds.x, terrain.bounds.y);
    let mut open = 0usize;
    crate::sim::terrain::scanline_cells(
        poly,
        origin,
        terrain.width(),
        terrain.height(),
        &mut |x, y| {
            if !terrain.solid_index(x, y) {
                open += 1;
            }
        },
    );
    open > 0
}

fn to_spawn(o: &ObjectDef) -> Spawn {
    Spawn {
        pos: V2::new(o.pos[0], o.pos[1]),
        angle: o.angle,
    }
}

/// Pads keep their authored surface and gain the height a ship needs to land.
fn pad_area(pos: [f32; 2], size: [f32; 2]) -> Area {
    let surface = Rect::centered(V2::new(pos[0], pos[1]), size[0], size[1]);
    let height = size[1].max(PAD_ZONE_HEIGHT);
    Area {
        rect: Rect::new(surface.x, surface.bottom() - height, size[0], height),
    }
}

fn to_area(pos: [f32; 2], size: [f32; 2]) -> Area {
    Area {
        rect: Rect::centered(V2::new(pos[0], pos[1]), size[0], size[1]),
    }
}
