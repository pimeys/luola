//! The whole visual identity in one place: a vector-arcade palette.
//!
//! The cave is a dark mass, everything that moves is bright and additive, and
//! hazard/objective colours are reserved so they never get reused decoratively.

use crate::render::fb::rgb;

pub const SPACE: u32 = rgb(5, 7, 13);
pub const CAVE_BG: u32 = rgb(8, 12, 22);
pub const WALL_FILL: u32 = rgb(19, 28, 45);
pub const WALL_EDGE: u32 = rgb(96, 140, 186);
pub const WALL_EDGE_HOT: u32 = rgb(150, 196, 240);

// ------------------------------------------------------------- materials --
//
// The cave body is drawn from the material grid: a brown dirt base and a grey
// rock base, each modulated per cell by `Terrain::shade`, so packed earth reads
// as granular rather than as a flat fill.

/// Base colour of a dirt cell (before `shade`).
pub const DIRT: u32 = rgb(126, 84, 52);
/// Dirt under an overhang, where roots hang: the cave's ceiling.
pub const DIRT_DEEP: u32 = rgb(80, 52, 32);
/// Base colour of a rock cell (before `shade`).
pub const ROCK: u32 = rgb(120, 124, 132);
/// Exposed rock face: a boulder's lit cap.
pub const ROCK_TOP: u32 = rgb(156, 162, 172);
/// Rock's shaded underside, so embedded boulders keep an edge.
pub const ROCK_EDGE: u32 = rgb(52, 56, 64);

/// Base colour of a granular wall cell (before `shade`): pale and gritty, so a
/// wall the player must shoot through reads differently from dirt and rock.
pub const GRANULAR: u32 = rgb(150, 132, 96);
/// Lit cap of a granular wall.
pub const GRANULAR_TOP: u32 = rgb(200, 184, 140);
/// Granular wall's underside.
pub const GRANULAR_EDGE: u32 = rgb(88, 76, 54);
/// Dust thrown up when a granular wall is chipped.
pub const GRANULAR_DUST: u32 = rgb(178, 160, 118);

/// Bright top pixel of the grass band on an upward-facing dirt surface.
pub const GRASS_HI: u32 = rgb(140, 226, 82);
/// The band's body.
pub const GRASS: u32 = rgb(86, 176, 58);
/// The band's last pixel, fading into the dirt below.
pub const GRASS_LO: u32 = rgb(52, 118, 44);
/// Hanging root under an overhang.
pub const ROOT: u32 = rgb(74, 54, 34);
/// Decorative mushroom cap and stalk.
pub const MUSHROOM_CAP: u32 = rgb(232, 226, 200);
pub const MUSHROOM_STEM: u32 = rgb(198, 180, 150);

/// Dust thrown up by digging dirt.
pub const DIRT_DUST: u32 = rgb(138, 100, 62);
/// Dust thrown up by striking rock.
pub const ROCK_DUST: u32 = rgb(140, 144, 152);

/// Radar: a cell that was authored solid and has since been dug away.
pub const RADAR_DUG: u32 = rgb(232, 152, 64);

pub const LIQUID_FILL: u32 = rgb(26, 84, 150);
pub const LIQUID_EDGE: u32 = rgb(126, 214, 248);

/// Per-cell brightness modulation for a material base colour.
///
/// `s` is `Terrain::shade`, a static `0..=255` grain value; the mapping clamps
/// both ends so no cell goes black or blows out. This is 8.8 fixed point rather
/// than float, because it runs once per visible pixel. Cameras never enter into
/// it, so a cell's colour is the same wherever the view happens to be.
#[inline]
pub const fn shade(base: u32, s: u8) -> u32 {
    let mut k = (s as u32) * 256 / 170;
    if k < 128 {
        k = 128;
    } else if k > 345 {
        k = 345;
    }
    (shade_ch(base >> 16, k) << 16) | (shade_ch(base >> 8, k) << 8) | shade_ch(base, k)
}

/// One 8.8-scaled, saturated channel of [`shade`].
#[inline]
const fn shade_ch(base: u32, k: u32) -> u32 {
    let v = ((base & 0xFF) * k) >> 8;
    if v > 255 { 255 } else { v }
}

/// `shade` over every grain value, baked at compile time.
const fn ramp(base: u32) -> [u32; 256] {
    let mut out = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        out[i] = shade(base, i as u8);
        i += 1;
    }
    out
}

pub const DIRT_RAMP: [u32; 256] = ramp(DIRT);
pub const DIRT_DEEP_RAMP: [u32; 256] = ramp(DIRT_DEEP);
pub const ROCK_RAMP: [u32; 256] = ramp(ROCK);
pub const ROCK_EDGE_RAMP: [u32; 256] = ramp(ROCK_EDGE);
pub const GRANULAR_RAMP: [u32; 256] = ramp(GRANULAR);
pub const GRANULAR_EDGE_RAMP: [u32; 256] = ramp(GRANULAR_EDGE);
pub const GRASS_LO_RAMP: [u32; 256] = ramp(GRASS_LO);

pub const SHIP: u32 = rgb(232, 242, 255);
pub const SHIP_DIM: u32 = rgb(150, 170, 196);
pub const THRUST_HOT: u32 = rgb(255, 240, 190);
pub const THRUST_COOL: u32 = rgb(255, 138, 40);
pub const SHIELD: u32 = rgb(127, 219, 255);
pub const SHIELD_HIT: u32 = rgb(255, 255, 255);

pub const BULLET: u32 = rgb(174, 241, 255);
pub const BULLET_ENEMY: u32 = rgb(255, 138, 92);
pub const BEAM: u32 = rgb(140, 255, 210);

// ------------------------------------------------------------- weapons ----
//
// One colour per weapon family, so the cave reads at a glance: what is flying
// at you, and what it is going to do when it lands. The simulation knows none
// of this — `sim::weapons` is data, this is the look.

/// The always-mounted gun.
pub const SHOT_GUN: u32 = BULLET;
/// Fast, heavy, straight: ion cannon, dumbfire, torpedo, nuke.
pub const SHOT_HEAVY: u32 = rgb(226, 168, 255);
/// Fire and things that burn: hellfire, fireworks.
pub const SHOT_FLAME: u32 = rgb(255, 156, 64);
/// Things that go off: bombs, grenades, rockets, missiles.
pub const SHOT_BLAST: u32 = rgb(255, 206, 120);
/// Cold: freezer, net.
pub const SHOT_COLD: u32 = rgb(150, 226, 255);
/// Poison and gas.
pub const SHOT_TOXIC: u32 = rgb(168, 255, 120);
/// Dirt and tools: dirtball, digger, gravitor, watercannon.
pub const SHOT_TOOL: u32 = rgb(196, 168, 132);
/// Energy the ship itself throws: shield, teleport, electric blast.
pub const SHOT_FIELD: u32 = rgb(180, 140, 255);

pub const CLOUD_POISON: u32 = rgb(150, 226, 96);
pub const CLOUD_GAS: u32 = rgb(120, 200, 110);
pub const CLOUD_FLAME: u32 = rgb(255, 152, 56);
pub const CLOUD_WATER: u32 = rgb(126, 214, 248);
pub const CLOUD_SPARKS: u32 = rgb(255, 236, 150);

pub const TETHER: u32 = rgb(220, 220, 160);
pub const WELL_CORE: u32 = rgb(180, 140, 255);
pub const TROOPER: u32 = rgb(140, 255, 200);

/// The colour a shot, cloud or gadget of this weapon is drawn in.
///
/// One place, so a weapon reads the same on the ship, in flight, as a cloud and
/// as a HUD pip.
pub fn weapon_color(weapon: crate::sim::weapons::WeaponId) -> u32 {
    use crate::sim::weapons::WeaponId as W;
    match weapon {
        W::Gun => SHOT_GUN,
        W::IonCannon | W::Torpedo | W::Nuke | W::Digger => SHOT_HEAVY,
        W::Hellfire | W::Fireworks => SHOT_FLAME,
        W::Bomb
        | W::GrenadeLauncher
        | W::Splinterbomb
        | W::Rockets
        | W::Missile
        | W::Nucleus
        | W::Bats
        | W::Dumbfire => SHOT_BLAST,
        W::Freezer | W::Net => SHOT_COLD,
        W::Poison | W::PoisonGas => SHOT_TOXIC,
        W::Dirtball | W::Gravitor | W::Watercannon => SHOT_TOOL,
        W::Shield | W::Teleport | W::ElectricBlast => SHOT_FIELD,
        _ => SHOT_GUN,
    }
}

/// The colour of a lingering cloud.
pub fn cloud_color(kind: crate::sim::weapons::CloudKind) -> u32 {
    use crate::sim::weapons::CloudKind as C;
    match kind {
        C::Poison => CLOUD_POISON,
        C::Gas => CLOUD_GAS,
        C::Flame => CLOUD_FLAME,
        C::Water => CLOUD_WATER,
        C::Sparks => CLOUD_SPARKS,
    }
}

pub const POD: u32 = rgb(255, 209, 102);
pub const POD_DEAD: u32 = rgb(96, 84, 60);
pub const REACTOR: u32 = rgb(123, 255, 176);
pub const REACTOR_HOT: u32 = rgb(255, 92, 60);
pub const EXIT: u32 = rgb(110, 231, 255);
pub const EXIT_LOCKED: u32 = rgb(255, 90, 60);
/// Gate/crusher slab body and its hazard edge.
pub const GATE: u32 = rgb(58, 66, 88);
pub const GATE_EDGE: u32 = rgb(214, 172, 74);
pub const GATE_HOT: u32 = rgb(255, 208, 96);
pub const FUEL_POD: u32 = rgb(185, 255, 102);
pub const PAD: u32 = rgb(110, 231, 255);
pub const TURRET: u32 = rgb(255, 107, 107);
pub const TURRET_DEAD: u32 = rgb(70, 70, 78);
pub const DRONE: u32 = rgb(255, 159, 67);
pub const MINE: u32 = rgb(255, 77, 77);

pub const HUD_TEXT: u32 = rgb(207, 227, 255);
pub const HUD_DIM: u32 = rgb(74, 107, 138);
pub const HUD_ACCENT: u32 = rgb(110, 231, 255);
pub const HUD_WARNING: u32 = rgb(255, 204, 0);
pub const HUD_BAD: u32 = rgb(255, 90, 60);
pub const HUD_GOOD: u32 = rgb(123, 255, 176);
pub const HUD_PANEL: u32 = rgb(9, 14, 24);
pub const HUD_PANEL_EDGE: u32 = rgb(40, 66, 94);
