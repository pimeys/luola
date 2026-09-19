//! Every feel number lives here.
//!
//! Values marked `[proposal]` come from `docs/game_mechanics.md` §11: the genre's
//! adjustment cheatsheet. They are starting points for iteration, not constants
//! quoted from a shipped game.

use crate::math::V2;

/// Fixed timestep. Determinism beats sub-millisecond smoothing (§12).
pub const TICK_HZ: u32 = 120;
pub const DT: f32 = 1.0 / TICK_HZ as f32;
/// Maximum simulation steps per rendered frame before time is dropped.
pub const MAX_STEPS_PER_FRAME: u32 = 8;

// ---------------------------------------------------------------- ship ----

/// Thrust-to-gravity ratio: the single most important number (§4.1).
/// `T/g = 2` keeps the cave a threat while still allowing a climb.
pub const THRUST_RATIO: f32 = 2.0;
/// Deliberately slow rotation: the skill is planning the attitude, not reacting.
pub const ROT_RATE: f32 = 3.0;
/// Modern scheme rotates the hull visually towards the aim faster than classic.
pub const MODERN_TURN_RATE: f32 = 6.5;
/// Fuel burnt per second of thrust. Hovering costs the same as climbing.
pub const FUEL_BURN: f32 = 4.0;
/// Hull: a V, the AUTS shape — a nose, two swept wingtips aft and a concave
/// tail notch between them, so the trailing edge cuts forward on the centre
/// line instead of closing flat. Collision uses all four vertices.
pub const HULL_APEX: V2 = V2::new(11.0, 0.0);
pub const HULL_WING_L: V2 = V2::new(-8.0, -7.0);
/// The notch: closer to the nose than the wingtips, which is what makes the
/// rear edge a V rather than a straight base.
pub const HULL_TAIL: V2 = V2::new(-3.0, 0.0);
pub const HULL_WING_R: V2 = V2::new(-8.0, 7.0);
/// Radius used for pickups, beam range checks and bullet hits.
pub const SHIP_RADIUS: f32 = 8.0;

// ------------------------------------------------------------- weapons ----

pub const BULLET_SPEED: f32 = 430.0;
pub const BULLET_LIFE: f32 = 1.3;
pub const FIRE_COOLDOWN: f32 = 0.14;
/// Bullets keep the launcher's velocity: aiming under inertia stays a skill.
pub const BULLET_INHERIT_VELOCITY: bool = true;

// The Wings loadout (`docs/finnish_cave_flyer_weapons.md` §4): one gun that
// never runs out, plus one special that does.
/// Hard cap on shots in flight. Salvos (shotgun 6, multicannon 8, rockets 6,
/// bats 6) and fragment bursts make the old cap of ten far too small.
pub const MAX_SHOTS: usize = 160;
/// Cap on lingering clouds; a stream weapon makes a few per second.
pub const MAX_CLOUDS: usize = 96;
/// Cap on laid gadgets (charges, wells, troopers).
pub const MAX_GADGETS: usize = 48;
/// Cap on mines the player has laid, so a level cannot fill with them.
pub const MAX_LAID_MINES: usize = 24;
/// Holding a turn key at a base steps one weapon per this many seconds.
pub const WEAPON_CYCLE_COOLDOWN: f32 = 0.18;
/// How hard the harpoon reel pulls, px/s², and the speed it settles at.
pub const TETHER_PULL: f32 = 900.0;
pub const TETHER_MAX_SPEED: f32 = 260.0;
/// Time a freshly laid mine stays inert, so it cannot detonate on its owner.
pub const GADGET_ARM_TIME: f32 = 0.6;
/// How often a damaging cloud applies its damage and its dig.
pub const CLOUD_TICK: f32 = 0.25;
/// A blast this close hurts the ship that fired it: bombs are not a melee weapon.
pub const SELF_BLAST_FRACTION: f32 = 0.45;
/// A trooper shoots at hostiles this far away.
pub const TROOPER_RANGE: f32 = 240.0;
pub const TROOPER_FIRE_COOLDOWN: f32 = 1.1;
pub const TROOPER_BULLET_SPEED: f32 = 380.0;
/// Pull the gravitor well exerts on drones and enemy fire, px/s².
pub const WELL_PULL: f32 = 260.0;
/// A netted drone keeps its net this long.
pub const NET_TIME: f32 = 6.0;
/// A frozen turret or drone stays frozen this long (Freezer overrides it).
pub const FREEZE_TIME: f32 = 4.0;
/// Turrets knocked out by an electric blast stay dark for this long.
pub const EMP_TIME: f32 = 6.0;
/// Blink this close to a wall is refused: the teleporter needs somewhere to go.
pub const BLINK_MIN: f32 = 40.0;
/// A blinking ship arrives this far clear of whatever stopped the blink.
pub const BLINK_CLEARANCE: f32 = 6.0;
/// A `Bouncer` keeps this much of its speed off each wall.
pub const BOUNCE_RESTITUTION: f32 = 0.92;
/// How much rock one bite of `Hellfire` takes out.
pub const FLAME_CARVE: f32 = 3.0;

// -------------------------------------------------------------- shield ----
// Gravity Ace's frame-level generosity, copied exactly (§4.3).

/// After a hit the shield keeps absorbing collisions for this long.
pub const SHIELD_ABSORB: f32 = 0.5;
/// Then it stays down before the recharge animation starts.
pub const SHIELD_DOWN: f32 = 0.9;
/// Recharge animation; the shield already absorbs from its first frame.
pub const SHIELD_RECHARGE: f32 = 0.5;
/// Shield stops working when the tank is dry.
pub const SHIELD_NEEDS_FUEL: bool = true;

// ---------------------------------------------------------------- beam ----

/// A pod this close is captured while the beam key is held.
pub const BEAM_RANGE: f32 = 70.0;
/// Towed beyond this the rod snaps.
pub const BEAM_BREAK: f32 = 170.0;
/// Masses used by the rod solver. The payload is light enough that the ship can
/// still climb with it — towing costs most of the climb rate and adds the swing,
/// but a payload that made ascent impossible would make every pod level
/// unwinnable, and the genre's payload missions are all about the return leg.
pub const SHIP_MASS: f32 = 1.0;
pub const POD_MASS: f32 = 0.4;
/// Restitution when the shield bounces the ship off a wall.
pub const SHIELD_BOUNCE: f32 = 0.35;
/// Radius used for pod/terrain contact and for the exit delivery test.
pub const POD_RADIUS: f32 = 7.0;
/// Impact speed at which a *landing* payload makes a noise instead of a quiet
/// settle. Only contact on the beam is lethal, so an unhitched payload parked on
/// a ledge is a legitimate tactic rather than a countdown.
pub const POD_REST_SPEED: f32 = 90.0;
/// The rod is anchored this far behind the hull, which is what makes the pair
/// spin around each other instead of behaving like a trailer.
pub const ROD_ANCHOR: f32 = 9.0;
/// Ship rotational inertia; small values let the payload yank the nose around.
pub const SHIP_INV_INERTIA: f32 = 0.03;

// ------------------------------------------------------------- hazards ----

/// Turret slew rate and cadence.
pub const TURRET_TURN_RATE: f32 = 1.5;
pub const TURRET_FIRE_COOLDOWN: f32 = 1.5;
pub const TURRET_RANGE: f32 = 560.0;
pub const TURRET_LEAD: f32 = 0.30;
pub const TURRET_HP: i32 = 2;
pub const TURRET_RADIUS: f32 = 9.0;
pub const TURRET_BULLET_SPEED: f32 = 330.0;

pub const DRONE_TURN_RATE: f32 = 2.2;
pub const DRONE_THRUST_RATIO: f32 = 1.35;
pub const DRONE_RADIUS: f32 = 7.0;
pub const DRONE_HP: i32 = 1;

pub const MINE_PROXIMITY: f32 = 48.0;
/// Hit points a mine takes out of whatever is standing in its blast.
pub const MINE_BLAST_DAMAGE: i32 = 2;
pub const MINE_BLAST_RADIUS: f32 = 68.0;
pub const MINE_RADIUS: f32 = 5.0;

pub const REACTOR_HP: i32 = 4;
pub const REACTOR_RADIUS: f32 = 14.0;
/// Time to leave once a reactor goes critical (Thrust's escape leg).
pub const ESCAPE_LIMIT: f32 = 45.0;

// --------------------------------------------------------------- water ----

/// Gravity scaling while submerged: a dive is survivable but slow — the genre's
/// "liquid the ship can dive into" (Turboraketti via `docs/game_mechanics.md` §2).
pub const WATER_DENSITY: f32 = 0.15;
/// Drag a submerged body fights, on top of the reduced gravity.
pub const WATER_DRAG: f32 = 2.6;
/// Water stops a bullet within a pool or two of depth.
pub const WATER_BULLET_DRAG: f32 = 5.0;
/// A shot dragged below this speed in water has stopped: it dies where it is.
pub const WATER_SHOT_FLOOR: f32 = 60.0;

// --------------------------------------------------------- destruction ----

/// Crater a bullet digs out of dirt. Big enough to tunnel with, small enough
/// that digging a route costs real time.
pub const CARVE_BULLET: f32 = 3.5;
/// Crater radius of a blast at `power = 1.0`; blasts scale with their power.
pub const CARVE_BLAST: f32 = 30.0;
/// Blasts below this radius do not disturb the terrain at all.
pub const CARVE_MIN: f32 = 1.0;
/// How long a dig leaves dust hanging in the air, in seconds.
pub const CARVE_DUST_TIME: f32 = 0.55;

// --------------------------------------------------------------- misc -----
/// Click-radius for fuel pods and pads.
pub const PICKUP_RADIUS: f32 = 15.0;
/// Landing on a pad only works below this speed, so pads are a manoeuvre.
pub const PAD_LANDING_SPEED: f32 = 42.0;
/// Cruise speed the camera leads the ship by.
pub const CAMERA_LEAD: f32 = 0.35;

// ------------------------------------------------------- mass economy ----
// Carried mass changes agility: a full tank and a full magazine fly heavier
// than a stripped one, and a base's rack is what you pay for (AUTS and
// Turboraketti both let a pilot leave ammo and fuel behind for speed).
//
// The reference mass is the ship at *full* tank and *full* magazine, so the
// thrust-to-gravity feel the levels were budgeted around is exactly what a
// freshly launched ship gets; burning or jettisoning load only ever makes it
// quicker. The rod keeps its own relative mass ratio (`SHIP_MASS`/`POD_MASS`),
// so the payload's behaviour is unchanged.

/// Mass a unit of fuel adds.
pub const FUEL_MASS: f32 = 0.0025;
/// Mass one special-ammo round adds.
pub const AMMO_MASS: f32 = 0.0012;
/// Cap on the handling bonus a light load can buy, so a stripped racer is
/// quicker but the levels' thrust budgets still mean something.
pub const MASS_AGILITY_MAX: f32 = 1.30;

/// Hit points a cell of dirt has: one, so a bullet's damage removes it exactly
/// as before. Granular walls take several.
pub const DIRT_HP: u8 = 1;
/// Hit points a cell of granular wall has. The AUTS "rakeinen seinä": it eats
/// fire for a while, and it holds a ship until it is shot free.
pub const GRANULAR_HP: u8 = 6;
/// Fraction of its velocity a ship keeps per tick while a granular wall holds
/// it. Small enough to be a trap, not zero, so thrust can still struggle.
pub const GRAB_DAMP: f32 = 0.10;
/// Impact speed above which a granular wall bounces the ship out instead of
/// grabbing it — slamming one at speed still reads as a crash.
pub const GRAB_MAX_SPEED: f32 = 220.0;

// ------------------------------------------------------ moving geometry ----
/// Default size of an authored `[[gate]]`, px.
pub const GATE_SIZE: f32 = 40.0;
/// Default travel speed of a gate, px/s.
pub const GATE_SPEED: f32 = 56.0;

// ------------------------------------------------------- score weights ----

pub const SCORE_FUEL: i32 = 12;
pub const SCORE_TURRET: i32 = 250;
pub const SCORE_DRONE: i32 = 175;
pub const SCORE_MINE: i32 = 75;
pub const SCORE_REACTOR: i32 = 2000;
pub const SCORE_POD: i32 = 1500;
pub const SCORE_ESCAPE: i32 = 1000;
pub const SCORE_PAR_TIME: f32 = 90.0;
pub const SCORE_TIME_BONUS: i32 = 25;
