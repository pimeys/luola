//! World rendering: the cave, its furniture, the ship and the payload.
//!
//! Everything is drawn as vector primitives on top of the baked terrain spans,
//! which is both the genre's historical look and the cheapest way to render a
//! multi-screen level without a GPU.

use crate::math::V2;
use crate::render::camera::Camera;
use crate::render::fb::Framebuffer;
use crate::render::palette as pal;
use crate::sim::entities::Bullet;
use crate::sim::ship::Shield;
use crate::sim::terrain::{MAT_DIRT, MAT_GRANULAR, MAT_ROCK};
use crate::sim::tuning;
use crate::sim::weapons::{self, CloudKind, GadgetKind, WeaponId};
use crate::sim::world::World;

pub fn draw_world(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    draw_backdrop(fb, cam);
    draw_terrain(fb, world, cam);
    draw_water(fb, world, cam);
    draw_exits(fb, world, cam);
    draw_pads(fb, world, cam);
    draw_fuel_pods(fb, world, cam);
    draw_reactors(fb, world, cam);
    draw_turrets(fb, world, cam);
    draw_gates(fb, world, cam);
    draw_mines(fb, world, cam);
    draw_gadgets(fb, world, cam);
    draw_drones(fb, world, cam);
    draw_pods(fb, world, cam);
    draw_clouds(fb, world, cam);
    draw_beam(fb, world, cam);
    draw_bullets(fb, world, cam);
    draw_tether(fb, world, cam);
    draw_ship(fb, world, cam);
}

/// Deep-space gradient plus a faint grid, so motion is readable even in empty
/// chambers.
fn draw_backdrop(fb: &mut Framebuffer, cam: &Camera) {
    fb.clear(pal::SPACE);
    let h = fb.height();
    let w = fb.width();
    for y in 0..h {
        let t = y as f32 / h as f32;
        let c = pal::CAVE_BG;
        let (r, g, b) = ((c >> 16) as u8, (c >> 8) as u8, c as u8);
        let shade = crate::render::fb::rgb(
            (r as f32 * (1.0 - t * 0.6)) as u8,
            (g as f32 * (1.0 - t * 0.6)) as u8,
            (b as f32 * (1.0 - t * 0.6)) as u8,
        );
        fb.vspan(0, y, y, shade);
    }
    const GRID: f32 = 128.0;
    let o = cam.offset();
    let x0 = (o.x / GRID).floor() as i32;
    let y0 = (o.y / GRID).floor() as i32;
    let cols = (w as f32 / GRID).ceil() as i32 + 1;
    let rows = (h as f32 / GRID).ceil() as i32 + 1;
    let grid_color = crate::render::fb::rgb(14, 22, 38);
    for i in 0..cols {
        for j in 0..rows {
            let wx = (x0 + i) as f32 * GRID;
            let wy = (y0 + j) as f32 * GRID;
            let (x, y) = cam.world_to_screen(V2::new(wx, wy));
            fb.set(x.round() as i32, y.round() as i32, grid_color);
        }
    }
}

/// Per-cell integer hash, salted per decoration kind.
///
/// Deterministic in the cell coordinates alone, so a given cave cell always
/// grows the same tuft, mushroom or root whatever the camera is doing.
#[inline]
fn cell_hash(x: i32, y: i32, salt: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b1)
        ^ (y as u32).wrapping_mul(0x85eb_ca6b)
        ^ salt.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_f491);
    h ^= h >> 13;
    h
}

const SALT_TUFT: u32 = 0x51ed_2701;
const SALT_MUSHROOM: u32 = 0x7b4a_9f13;
const SALT_ROOT: u32 = 0x2c1b_7ad5;

/// Textured pass over the material grid.
///
/// The cave is no longer a set of polygons: every visible cell is coloured from
/// its material, its static `shade` grain, and whether it faces up (grass) or
/// down (a dark overhang underside that roots hang from). Colours depend only on
/// the cell, never on where the camera happens to be.
fn draw_terrain(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let terrain = &world.terrain;
    let o = cam.offset();
    let w = fb.width();
    let h = fb.height();
    let base_x = o.x.floor() as i32;
    let base_y = o.y.floor() as i32;
    let lo = base_y;
    let hi = base_y + h - 1;
    let stride = w as usize;
    let buf = fb.pixels_mut();
    for x in 0..w {
        let wx = base_x + x;
        let col = x as usize;
        for &(wy0, wy1) in terrain.spans().column(wx) {
            if wy1 < lo || wy0 > hi {
                continue;
            }
            let y0 = wy0.max(lo) - base_y;
            let y1 = wy1.min(hi) - base_y;
            // The spans are maximal solid runs, so the run's first cell always
            // faces air (it is a `surface`) and its last always faces air below
            // (a `roof`). Everything in between is packed material.
            let top_mat = terrain.cell(wx, wy0);
            for y in y0..=y1 {
                let wy = base_y + y;
                let d = wy - wy0;
                let mat = terrain.cell(wx, wy);
                let s = terrain.shade(wx, wy) as usize;
                let mut c = match mat {
                    MAT_ROCK => pal::ROCK_RAMP[s],
                    MAT_GRANULAR => pal::GRANULAR_RAMP[s],
                    _ => pal::DIRT_RAMP[s],
                };
                if d == 0 {
                    // Upward-facing face: grass on dirt, a lit cap elsewhere.
                    c = match mat {
                        MAT_ROCK => pal::ROCK_TOP,
                        MAT_GRANULAR => pal::GRANULAR_TOP,
                        _ => pal::GRASS_HI,
                    };
                } else if mat == MAT_DIRT && top_mat == MAT_DIRT && d <= 2 {
                    // The grass band's body, fading into the dirt beneath.
                    c = if d == 1 {
                        pal::GRASS
                    } else {
                        pal::GRASS_LO_RAMP[s]
                    };
                }
                if wy == wy1 && wy1 != wy0 {
                    // Ceiling: darker, so overhangs read as depth.
                    c = match mat {
                        MAT_ROCK => pal::ROCK_EDGE_RAMP[s],
                        MAT_GRANULAR => pal::GRANULAR_EDGE_RAMP[s],
                        _ => pal::DIRT_DEEP_RAMP[s],
                    };
                }
                buf[y as usize * stride + col] = c;
            }
        }
    }
    draw_decor(fb, world, base_x, base_y);
}

/// Cosmetic dressing for the grass line: short tufts on upward faces, the odd
/// mushroom, and roots hanging under overhangs.
///
/// Sparse by construction — one decoration per `TUFT_STEP` / `MUSH_STEP` /
/// `ROOT_STEP` cells at most — and derived purely from the cell coordinates, so
/// it is identical frame to frame and behind every sprite the world draws next.
fn draw_decor(fb: &mut Framebuffer, world: &World, base_x: i32, base_y: i32) {
    const TUFT_STEP: u32 = 16;
    const MUSH_STEP: u32 = 211;
    const ROOT_STEP: u32 = 19;
    let terrain = &world.terrain;
    let w = fb.width();
    let h = fb.height();
    for x in 0..w {
        let wx = base_x + x;
        for &(wy0, wy1) in terrain.spans().column(wx) {
            let top = wy0 - base_y;
            let bot = wy1 - base_y;
            if bot < -4 || top > h + 4 {
                continue;
            }
            let dirt_top = terrain.cell(wx, wy0) == MAT_DIRT;
            if dirt_top {
                let m = cell_hash(wx, wy0, SALT_MUSHROOM);
                if m.is_multiple_of(MUSH_STEP) {
                    // A stalk two pixels tall under a small cap.
                    fb.set(x, top - 1, pal::MUSHROOM_STEM);
                    fb.set(x, top - 2, pal::MUSHROOM_CAP);
                    fb.set(x - 1, top - 2, pal::MUSHROOM_CAP);
                    fb.set(x + 1, top - 2, pal::MUSHROOM_CAP);
                    fb.set(x, top - 3, pal::MUSHROOM_CAP);
                } else {
                    let t = cell_hash(wx, wy0, SALT_TUFT);
                    if t.is_multiple_of(TUFT_STEP) {
                        let tall = (t >> 8) & 1 == 1;
                        fb.set(x, top - 1, pal::GRASS);
                        if tall {
                            fb.set(x, top - 2, pal::GRASS_HI);
                        }
                    }
                }
            }
            if wy1 > wy0 && terrain.cell(wx, wy1) == MAT_DIRT {
                let r = cell_hash(wx, wy1, SALT_ROOT);
                if r.is_multiple_of(ROOT_STEP) {
                    let len = 1 + ((r >> 8) % 3) as i32;
                    for k in 1..=len {
                        fb.set(
                            x,
                            bot + k,
                            if k == len { pal::ROOT } else { pal::DIRT_DEEP },
                        );
                    }
                }
            }
        }
    }
}

/// Translucent water over the dug cave, under the ship and the furniture.
///
/// One cell of mass is one cell of opacity: `mass / 8` of the body colour, a
/// bright line on `Water::surface` cells, and a slow wobble driven by the
/// simulation clock — never the wall clock, so screenshots stay reproducible.
fn draw_water(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let water = &world.water;
    let terrain = &world.terrain;
    let t = world.elapsed();
    let o = cam.offset();
    let w = fb.width();
    let h = fb.height();
    let base_x = o.x.floor() as i32;
    let base_y = o.y.floor() as i32;
    let lo = base_y;
    let hi = base_y + h - 1;
    for x in 0..w {
        let wx = base_x + x;
        // Water only lives in open cells, so walk the gaps between the solid
        // runs instead of testing every pixel on screen.
        let mut next = lo;
        for &(wy0, wy1) in terrain.spans().column(wx) {
            if wy1 < lo {
                continue;
            }
            if wy0 > hi {
                break;
            }
            let gap_end = (wy0 - 1).min(hi);
            if gap_end >= next {
                draw_water_gap(fb, water, x, next, gap_end, base_y, wx, t);
            }
            next = (wy1 + 1).max(next);
            if next > hi {
                break;
            }
        }
        if next <= hi {
            draw_water_gap(fb, water, x, next, hi, base_y, wx, t);
        }
    }
}

/// One open column run: the cells between two solid spans, or either side of
/// the visible band.
#[allow(clippy::too_many_arguments)]
fn draw_water_gap(
    fb: &mut Framebuffer,
    water: &crate::sim::water::Water,
    x: i32,
    wy0: i32,
    wy1: i32,
    base_y: i32,
    wx: i32,
    t: f32,
) {
    for wy in wy0..=wy1 {
        let m = water.mass(wx, wy);
        if m == 0 {
            continue;
        }
        let y = wy - base_y;
        let a = 0.10 + 0.55 * (m as f32 / 8.0);
        fb.blend(x, y, pal::LIQUID_FILL, a);
        if water.surface(wx, wy) {
            let phase = wx as f32 * 0.13 + t * 2.6;
            let shimmer = (phase.sin() * 0.5 + 0.5) * 0.45;
            fb.blend(x, y, pal::LIQUID_EDGE, 0.5 + shimmer);
            // The line bobs by a pixel; the base pixel stays drawn, so the
            // surface never breaks into a dotted line.
            if phase.sin() > 0.55 {
                fb.blend(x, y - 1, pal::LIQUID_EDGE, 0.35);
            }
        }
    }
}

fn draw_exits(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let t = world.elapsed();
    for exit in &world.exits {
        let r = exit.rect;
        let (x0, y0) = cam.world_to_screen(V2::new(r.left(), r.top()));
        let (x1, y1) = cam.world_to_screen(V2::new(r.right(), r.bottom()));
        let color = if exit.open {
            pal::EXIT
        } else {
            pal::EXIT_LOCKED
        };
        let pulse = if exit.open {
            0.55 + 0.45 * (t * 3.0).sin().abs()
        } else {
            0.35
        };
        fb.rect_stroke(x0 as i32, y0 as i32, x1 as i32, y1 as i32, color);
        fb.line_fa((x0, y0), (x1, y1), color, pulse * 0.5);
        fb.line_fa((x1, y0), (x0, y1), color, pulse * 0.5);
        // Corner ticks.
        let c = 8.0;
        fb.line_fa((x0, y0), (x0 + c, y0), color, 1.0);
        fb.line_fa((x0, y0), (x0, y0 + c), color, 1.0);
        fb.line_fa((x1, y0), (x1 - c, y0), color, 1.0);
        fb.line_fa((x1, y0), (x1, y0 + c), color, 1.0);
        fb.line_fa((x0, y1), (x0 + c, y1), color, 1.0);
        fb.line_fa((x0, y1), (x0, y1 - c), color, 1.0);
        fb.line_fa((x1, y1), (x1 - c, y1), color, 1.0);
        fb.line_fa((x1, y1), (x1, y1 - c), color, 1.0);
        if !exit.open {
            // Shut hatch.
            for k in 0..5 {
                let x = x0 + (x1 - x0) * (k as f32 + 0.5) / 5.0;
                fb.line_fa((x, y0), (x, y1), pal::EXIT_LOCKED, 0.5);
            }
        }
    }
}

fn draw_pads(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let t = world.elapsed();
    for pad in &world.pads {
        let r = pad.rect;
        let (x0, y0) = cam.world_to_screen(V2::new(r.left(), r.top()));
        let (x1, y1) = cam.world_to_screen(V2::new(r.right(), r.bottom()));
        let color = pal::PAD;
        let mut x = x0;
        while x < x1 {
            let end = (x + 5.0).min(x1);
            fb.line_fa((x, y0 + 1.0), (end, y0 + 1.0), color, 0.9);
            x += 9.0;
        }
        let blink = (t * 2.0).sin() * 0.5 + 0.5;
        fb.line_fa((x0, y1), (x1, y1), color, 0.35 + blink * 0.5);
        fb.glow(
            ((x1 - x0) * 0.5 + x0) as i32,
            y0 as i32,
            6.0,
            color,
            0.25 + blink * 0.25,
        );
    }
}

fn draw_fuel_pods(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    for fuel in &world.fuel_pods {
        if fuel.taken {
            continue;
        }
        let (x, y) = cam.world_to_screen(fuel.p);
        let r = 6.0 + (fuel.phase * 3.0).sin() * 1.2;
        let pts = [(x, y - r), (x + r * 0.8, y), (x, y + r), (x - r * 0.8, y)];
        fb.poly_fill(&pts, pal::FUEL_POD);
        fb.poly_fill(
            &[
                (x, y - r * 0.45),
                (x + r * 0.35, y),
                (x, y + r * 0.45),
                (x - r * 0.35, y),
            ],
            pal::HUD_PANEL,
        );
        fb.glow(x as i32, y as i32, 10.0, pal::FUEL_POD, 0.35);
    }
}

fn draw_reactors(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    for r in &world.reactors {
        let (x, y) = cam.world_to_screen(r.p);
        let radius = tuning::REACTOR_RADIUS;
        if r.destroyed {
            let flicker = 0.4 + 0.6 * ((r.phase * 9.0).sin() * 0.5 + 0.5);
            fb.circle_stroke(x, y, radius * 1.3, pal::REACTOR_HOT);
            fb.circle_stroke(x, y, radius * 0.6, pal::REACTOR_HOT);
            fb.glow(
                x as i32,
                y as i32,
                radius * 2.0,
                pal::REACTOR_HOT,
                flicker * 0.5,
            );
            continue;
        }
        let pulse = 0.6 + 0.4 * (r.phase * 4.0).sin();
        let hp = r.hp as f32 / tuning::REACTOR_HP as f32;
        let hex: Vec<(f32, f32)> = (0..6)
            .map(|i| {
                let a = i as f32 * std::f32::consts::TAU / 6.0 + r.phase * 0.4;
                (x + a.cos() * radius, y + a.sin() * radius)
            })
            .collect();
        fb.poly_fill(&hex, pal::WALL_FILL);
        for i in 0..hex.len() {
            let j = (i + 1) % hex.len();
            fb.line_fa(hex[i], hex[j], pal::REACTOR, 0.9);
        }
        fb.circle_fill(x, y, radius * 0.45 * hp.max(0.25), pal::REACTOR);
        fb.glow(x as i32, y as i32, radius * 2.2, pal::REACTOR, pulse * 0.45);
        // Hit pips.
        for i in 0..tuning::REACTOR_HP {
            let on = i < r.hp;
            let a = -std::f32::consts::FRAC_PI_2 + (i as f32 - 1.0) * 0.35;
            let px = x + a.cos() * (radius + 8.0);
            let py = y + a.sin() * (radius + 8.0);
            if on {
                fb.circle_fill(px, py, 1.6, pal::REACTOR);
            } else {
                fb.circle_stroke(px, py, 1.6, pal::HUD_DIM);
            }
        }
    }
}

fn draw_turrets(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let t = world.elapsed();
    for turret in &world.turrets {
        let (x, y) = cam.world_to_screen(turret.p);
        // A turret knocked out by EMP or a freeze is still standing, but its
        // barrel goes dark and its glow dies: the state is legible at a glance.
        let op = turret.operational();
        let color = if op { pal::TURRET } else { pal::TURRET_DEAD };
        fb.circle_fill(x, y, 4.5, pal::WALL_FILL);
        fb.circle_stroke(x, y, 5.0, color);
        let len = 13.0;
        let dir = V2::from_angle(turret.aim);
        fb.line_fa(
            (x, y),
            (x + dir.x * len, y + dir.y * len),
            color,
            if op { 1.0 } else { 0.5 },
        );
        if op {
            let charge = 1.0 - (turret.cooldown / tuning::TURRET_FIRE_COOLDOWN).clamp(0.0, 1.0);
            fb.glow(
                x as i32,
                y as i32,
                9.0,
                pal::TURRET,
                (charge * 0.5) + (t * 4.0).sin() * 0.05 + 0.1,
            );
            if turret.cooldown > tuning::TURRET_FIRE_COOLDOWN - 0.08 {
                fb.glow(
                    (x + dir.x * len) as i32,
                    (y + dir.y * len) as i32,
                    12.0,
                    pal::THRUST_HOT,
                    0.9,
                );
            }
        }
    }
}

/// Gates and crushers: solid slabs with hazard bars. Only an active gate is
/// drawn, so a hidden or not-yet-powered one is genuinely invisible.
fn draw_gates(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    for g in &world.gates {
        if !g.active || g.hidden {
            continue;
        }
        let r = g.rect;
        let (a, b) = (
            cam.world_to_screen(V2::new(r.x, r.y)),
            cam.world_to_screen(V2::new(r.right(), r.bottom())),
        );
        if b.0 < 0.0 || b.1 < 0.0 || a.0 >= fb.width() as f32 || a.1 >= fb.height() as f32 {
            continue;
        }
        let (x0, y0, x1, y1) = (a.0 as i32, a.1 as i32, b.0 as i32, b.1 as i32);
        fb.rect_fill(x0, y0, x1, y1, pal::GATE);
        fb.rect_stroke(x0, y0, x1, y1, pal::GATE_EDGE);
        let mut x = x0 + 4;
        while x < x1 - 2 {
            fb.vspan(x, y0 + 2, y1 - 2, pal::GATE_HOT);
            x += 10;
        }
    }
}

fn draw_mines(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    for mine in &world.mines {
        let (x, y) = cam.world_to_screen(mine.body.p);
        // A mine the player laid is friendly and blinks slow; the level's own
        // mines stay hostile red and blink fast, so a field is readable.
        let friendly = mine.from_player;
        let color = if friendly { pal::TROOPER } else { pal::MINE };
        let r = 5.0;
        let pts = [(x, y - r), (x + r, y), (x, y + r), (x - r, y)];
        fb.poly_fill(&pts, pal::WALL_FILL);
        for i in 0..4 {
            let j = (i + 1) % 4;
            fb.line_fa(pts[i], pts[j], color, 0.9);
        }
        let armed = mine.age > 0.6;
        let rate = if friendly { 1.3 } else { 3.0 };
        let blink = ((mine.age * rate).sin() * 0.5 + 0.5) * if armed { 1.0 } else { 0.35 };
        fb.glow(x as i32, y as i32, 7.0, color, blink * 0.6);
        // Only a hostile mine hunts the ship, so only it gets the proximity ring.
        if armed && !friendly && mine.body.p.dist(world.ship.body.p) < tuning::MINE_PROXIMITY * 1.6
        {
            fb.circle_stroke(x, y, 6.0 + blink * 2.0, pal::MINE);
        }
    }
}

/// Charges, gravity wells and trooper pairs: the things `Kind::Place` weapons
/// leave behind. Each reads by its own motion — a charge's blink quickens as its
/// fuse burns, a well's arcs chase inwards, and a trooper pair flashes when it
/// has just fired.
fn draw_gadgets(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let t = world.elapsed();
    for g in &world.gadgets {
        let (x, y) = cam.world_to_screen(g.p);
        match g.kind {
            GadgetKind::Charge => {
                let color = pal::weapon_color(g.weapon);
                // Blink rate climbs as the fuse runs out, so the last seconds
                // are unmissable.
                let full = weapons::spec(g.weapon).ttl.max(1e-4);
                let left = (g.ttl / full).clamp(0.0, 1.0);
                let blink = (t * (3.0 + 14.0 * (1.0 - left))).sin() * 0.5 + 0.5;
                fb.circle_fill(x, y, 3.0, if blink > 0.5 { color } else { pal::WALL_FILL });
                fb.circle_stroke(x, y, 4.0, color);
                fb.glow(x as i32, y as i32, 8.0, color, 0.2 + blink * 0.5);
            }
            GadgetKind::Well => {
                // Three arcs chasing inwards over a bright, pulled-in core: the
                // pull direction is the whole read.
                let r = 9.0;
                for k in 0..3 {
                    let start = t * 2.4 + k as f32 * std::f32::consts::TAU / 3.0;
                    fb.arc(x, y, r, 1.2, start, start + 1.7, pal::WELL_CORE);
                }
                fb.circle_fill(x, y, 3.0 + (t * 5.0).sin() * 0.6, pal::WELL_CORE);
                fb.glow(x as i32, y as i32, 12.0, pal::WELL_CORE, 0.45);
            }
            GadgetKind::Troopers => {
                for k in [-1.0f32, 1.0] {
                    let tx = x + k * 4.0;
                    // A tiny upright figure: head, body, braced legs.
                    fb.circle_fill(tx, y - 5.0, 1.6, pal::TROOPER);
                    fb.line_fa((tx, y - 3.0), (tx, y + 2.0), pal::TROOPER, 1.0);
                    fb.line_fa((tx - 2.0, y + 4.0), (tx, y + 2.0), pal::TROOPER, 0.9);
                    fb.line_fa((tx + 2.0, y + 4.0), (tx, y + 2.0), pal::TROOPER, 0.9);
                }
                // The shooting timer resets on a shot, so a fresh `cd` is the
                // muzzle flash.
                if g.cd > tuning::TROOPER_FIRE_COOLDOWN - 0.06 {
                    fb.glow(x as i32, (y - 3.0) as i32, 9.0, pal::THRUST_HOT, 0.8);
                }
            }
            // Mines are pushed into `world.mines`, never here; nothing to draw.
            GadgetKind::Mine | GadgetKind::Landmine => {}
        }
    }
}

fn draw_drones(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    for drone in &world.drones {
        if !drone.alive {
            continue;
        }
        let (x, y) = cam.world_to_screen(drone.body.p);
        let a = drone.body.angle;
        let nose = V2::from_angle(a) * 7.0;
        let left = V2::from_angle(a + 2.5) * 6.0;
        let right = V2::from_angle(a - 2.5) * 6.0;
        fb.poly_fill(
            &[
                (x + nose.x, y + nose.y),
                (x + left.x, y + left.y),
                (x + right.x, y + right.y),
            ],
            pal::DRONE,
        );
        fb.glow(x as i32, y as i32, 8.0, pal::DRONE, 0.4);
        if drone.thrusting {
            let back = V2::from_angle(a + std::f32::consts::PI);
            fb.line_fa(
                (x + back.x * 6.0, y + back.y * 6.0),
                (x + back.x * 12.0, y + back.y * 12.0),
                pal::THRUST_COOL,
                0.9,
            );
        }
        if !drone.mobile() {
            if drone.netted > 0.0 {
                // Entangled: the net's cross-hatch over the hull.
                for k in 0..3 {
                    let o = (k as f32 - 1.0) * 4.5;
                    fb.line_fa((x - 7.0, y + o), (x + 7.0, y + o), pal::SHOT_COLD, 0.8);
                    fb.line_fa((x + o, y - 7.0), (x + o, y + 7.0), pal::SHOT_COLD, 0.8);
                }
                fb.circle_stroke(x, y, 7.0, pal::SHOT_COLD);
            } else {
                // Frozen solid: an ice rim around a dead hull.
                fb.circle_stroke(x, y, 7.0, pal::SHOT_COLD);
                fb.circle_stroke(x, y, 8.5, pal::SHOT_COLD);
            }
        }
    }
}

fn draw_pods(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    for pod in &world.pods {
        let (x, y) = cam.world_to_screen(pod.body.p);
        let color = if pod.alive { pal::POD } else { pal::POD_DEAD };
        let r = tuning::POD_RADIUS;
        let a = pod.body.angle;
        let corners: Vec<(f32, f32)> = [(-r, -r), (r, -r), (r, r), (-r, r)]
            .iter()
            .map(|(dx, dy)| {
                let p = V2::new(*dx, *dy).rotated(a);
                (x + p.x, y + p.y)
            })
            .collect();
        fb.poly_fill(&corners, pal::WALL_FILL);
        for i in 0..corners.len() {
            let j = (i + 1) % corners.len();
            fb.line_fa(corners[i], corners[j], color, 1.0);
        }
        fb.circle_fill(x, y, 2.2, color);
        if pod.alive {
            fb.glow(x as i32, y as i32, 12.0, color, 0.35);
        }
    }
}

/// Lingering weapon clouds: poison, gas, flame, water, sparks.
///
/// A soft translucent disc that fades as its life runs out, wobbling on the
/// simulation clock so a replay draws the same frames. Sparks are the one
/// exception: they are lit motes, not a grey mass.
fn draw_clouds(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    const SPARK_MOTES: usize = 7;
    let t = world.elapsed();
    for c in &world.clouds {
        let (cx, cy) = cam.world_to_screen(c.p);
        let color = pal::cloud_color(c.kind);
        // Remaining life as a fraction of the cloud's whole life.
        let life = (c.ttl / (c.ttl + c.age).max(1e-4)).clamp(0.0, 1.0);
        let wob = (t * 0.9 + c.p.x * 0.02 + c.p.y * 0.013).sin() * c.radius * 0.06;
        let (x, y) = (cx, cy + wob);
        let r = c.radius;
        if c.kind == CloudKind::Sparks {
            // Bright speckles orbiting the centre, so fireworks read as sparks.
            for k in 0..SPARK_MOTES {
                let ph = c.p.x * 0.21 + c.p.y * 0.13 + k as f32 * 1.7;
                let a = ph + t * (1.8 + 0.11 * k as f32);
                let rr = r * (0.3 + 0.6 * ((ph * 1.3).sin() * 0.5 + 0.5));
                let px = x + a.cos() * rr;
                let py = y + a.sin() * rr;
                fb.blend(px as i32, py as i32, color, 0.15 + 0.7 * life);
                fb.glow(px as i32, py as i32, 3.0, color, 0.35 * life);
            }
            continue;
        }
        // A filled disc, denser at the centre and thinning at the rim.
        let a0 = (0.10 + 0.30 * life) * 0.55;
        let x0 = (x - r).floor() as i32;
        let x1 = (x + r).ceil() as i32;
        let y0 = (y - r).floor() as i32;
        let y1 = (y + r).ceil() as i32;
        for py in y0..=y1 {
            for px in x0..=x1 {
                let dx = px as f32 + 0.5 - x;
                let dy = py as f32 + 0.5 - y;
                let d = (dx * dx + dy * dy).sqrt();
                if d > r {
                    continue;
                }
                fb.blend(px, py, color, a0 * (1.0 - 0.5 * d / r));
            }
        }
        fb.circle_stroke(x, y, r, color);
    }
}

fn draw_beam(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let Some(idx) = world.ship.attached else {
        return;
    };
    let pod = world.pods[idx];
    if !pod.alive {
        return;
    }
    let t = world.elapsed();
    let a = cam.world_to_screen(world.ship.body.p);
    let b = cam.world_to_screen(pod.body.p);
    for i in 0..4 {
        let wobble = ((t * 22.0 + i as f32 * 1.7).sin()) * 3.0 * (1.0 - i as f32 / 4.0);
        let dir = V2::new(b.0 - a.0, b.1 - a.1);
        let n = dir.perp().normalized();
        let off = n * wobble;
        fb.line_fa(
            (a.0 + off.x, a.1 + off.y),
            (b.0 + off.x, b.1 + off.y),
            pal::BEAM,
            0.25,
        );
    }
    fb.line_fa(a, b, pal::BEAM, 0.7);
    fb.glow(a.0 as i32, a.1 as i32, 8.0, pal::BEAM, 0.4);
    fb.glow(b.0 as i32, b.1 as i32, 8.0, pal::BEAM, 0.4);
}

/// One shot, drawn by weapon family.
///
/// The colour comes from the palette's per-weapon mapping, so a shot reads the
/// same in flight as on the HUD; enemy fire keeps the one hostile colour so it
/// can never be mistaken for the player's. Shape carries the rest: the plain gun
/// is a short tracer, heavy bolts are long and bright, shells are solid circles
/// with a dark core, and the odd weapons each get a silhouette of their own.
/// Every shot sits on a glow, so none of them vanish into dirt or rock.
fn draw_bullet(fb: &mut Framebuffer, b: &Bullet, cam: &Camera) {
    const TRACER: f32 = 9.0;
    const TRACER_LONG: f32 = 20.0;
    const TRAIL: f32 = 14.0;
    const SHELL_R: f32 = 4.0;
    const SHELL_CORE: f32 = 1.8;
    let color = if b.from_player {
        pal::weapon_color(b.weapon)
    } else {
        pal::BULLET_ENEMY
    };
    let head = cam.world_to_screen(b.p);
    let (hx, hy) = (head.0 as i32, head.1 as i32);
    let dir = b.v.normalized();
    match b.weapon {
        WeaponId::IonCannon
        | WeaponId::Dumbfire
        | WeaponId::Torpedo
        | WeaponId::Nuke
        | WeaponId::Digger => {
            // Heavy and energy bolts: longer and brighter than the gun.
            let tail = cam.world_to_screen(b.p - dir * TRACER_LONG);
            fb.line_fa(tail, head, color, 1.0);
            fb.glow(hx, hy, 10.0, color, 0.7);
        }
        WeaponId::Bomb | WeaponId::GrenadeLauncher | WeaponId::Splinterbomb => {
            // Shells are physical: a filled body with a dark core.
            fb.circle_fill(head.0, head.1, SHELL_R, color);
            fb.circle_fill(head.0, head.1, SHELL_CORE, pal::HUD_PANEL);
            fb.glow(hx, hy, 7.0, color, 0.45);
        }
        WeaponId::Missile | WeaponId::Bats => {
            // A short trail behind the head, rather than a full tracer.
            let tail = cam.world_to_screen(b.p - dir * TRAIL);
            fb.line_fa(tail, head, color, 0.5);
            fb.circle_fill(head.0, head.1, 2.2, color);
            fb.glow(hx, hy, 8.0, color, 0.55);
        }
        WeaponId::Bouncer => {
            // A bright rim, so a ricochet is easy to follow off a wall.
            fb.circle_fill(head.0, head.1, 2.6, color);
            fb.circle_stroke(head.0, head.1, 4.0, pal::SHIP);
            fb.glow(hx, hy, 7.0, color, 0.6);
        }
        WeaponId::Net => {
            fb.circle_stroke(head.0, head.1, 3.5, color);
            fb.glow(hx, hy, 6.0, color, 0.4);
        }
        WeaponId::Freezer => {
            // A cold blue body with a white core.
            fb.circle_fill(head.0, head.1, 3.0, color);
            fb.circle_stroke(head.0, head.1, 1.5, pal::SHIELD_HIT);
            fb.glow(hx, hy, 9.0, color, 0.6);
        }
        // The gun, autofire and everything else: a short tracer.
        _ => {
            let tail = cam.world_to_screen(b.p - dir * TRACER);
            fb.line_fa(tail, head, color, 0.95);
            fb.glow(hx, hy, 6.0, color, 0.5);
        }
    }
}

fn draw_bullets(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    for b in &world.bullets {
        draw_bullet(fb, b, cam);
    }
}

/// The harpoon's cable, from the hull to the rock it bit into.
///
/// Drawn under the ship so the hull stays on top of its own rope; the anchor
/// grip marks the point in the rock the ship is being reeled towards.
fn draw_tether(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let Some((anchor, _)) = world.ship.tether else {
        return;
    };
    if !world.ship.alive {
        return;
    }
    let a = cam.world_to_screen(world.ship.body.p);
    let b = cam.world_to_screen(anchor);
    fb.line_fa(a, b, pal::TETHER, 0.9);
    // The grip: a small ring with a cross through it.
    fb.circle_stroke(b.0, b.1, 3.0, pal::TETHER);
    fb.line_fa((b.0 - 4.0, b.1), (b.0 + 4.0, b.1), pal::TETHER, 0.8);
    fb.line_fa((b.0, b.1 - 4.0), (b.0, b.1 + 4.0), pal::TETHER, 0.8);
    fb.glow(b.0 as i32, b.1 as i32, 5.0, pal::TETHER, 0.35);
}

fn draw_ship(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let ship = &world.ship;
    if !ship.alive {
        return;
    }
    let t = world.elapsed();
    let hull = ship.hull();
    let pts: Vec<(f32, f32)> = hull.iter().map(|p| cam.world_to_screen(*p)).collect();
    let body_color = if ship.shield.absorbing() {
        pal::SHIP
    } else {
        pal::SHIP_DIM
    };

    if ship.thrusting {
        let back = -ship.thrust_dir;
        let (bx, by) = cam.world_to_screen(ship.body.p + back * 5.0);
        let flicker = 10.0 + ((t * 47.0).sin() * 0.5 + 0.5) * 9.0;
        let n = back.perp() * 4.0;
        fb.poly_fill(
            &[
                (bx + n.x, by + n.y),
                (bx - n.x, by - n.y),
                (bx + back.x * flicker, by + back.y * flicker),
            ],
            pal::THRUST_COOL,
        );
        fb.glow(
            (bx + back.x * 4.0) as i32,
            (by + back.y * 4.0) as i32,
            8.0,
            pal::THRUST_HOT,
            0.6,
        );
    }

    fb.poly_fill(&pts, body_color);
    for i in 0..pts.len() {
        let j = (i + 1) % pts.len();
        fb.line_fa(pts[i], pts[j], pal::SHIP, 1.0);
    }
    fb.glow(pts[0].0 as i32, pts[0].1 as i32, 7.0, pal::SHIP, 0.35);

    // Shield gauge: a ring around the hull that empties as it recharges.
    let (cx, cy) = cam.world_to_screen(ship.body.p);
    let charge = ship.shield.charge();
    let radius = 14.0;
    match ship.shield {
        Shield::Charged => {
            fb.arc(cx, cy, radius, 1.2, 0.0, std::f32::consts::TAU, pal::SHIELD);
        }
        Shield::Absorbing { t: hit_t } => {
            let flash = (1.0 - hit_t / tuning::SHIELD_ABSORB).clamp(0.0, 1.0);
            fb.arc(
                cx,
                cy,
                radius + flash * 3.0,
                2.0 + flash * 2.0,
                0.0,
                std::f32::consts::TAU,
                pal::SHIELD_HIT,
            );
        }
        Shield::Down { .. } => {
            fb.arc(
                cx,
                cy,
                radius,
                1.0,
                0.0,
                std::f32::consts::TAU * charge,
                pal::HUD_DIM,
            );
        }
        Shield::Recharging { .. } => {
            fb.arc(
                cx,
                cy,
                radius,
                1.2,
                -std::f32::consts::FRAC_PI_2,
                -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * charge,
                pal::SHIELD,
            );
        }
    }

    // The Shield weapon's own bubble, outside the recharge gauge: a pulsing
    // second ring in the field colour, so it never reads as the ship's innate
    // shield.
    if ship.shield_field > 0.0 {
        let pulse = (t * 5.0).sin() * 0.5 + 0.5;
        fb.arc(
            cx,
            cy,
            radius + 5.0,
            1.4 + pulse * 0.8,
            0.0,
            std::f32::consts::TAU,
            pal::SHOT_FIELD,
        );
        fb.glow(
            cx as i32,
            cy as i32,
            radius + 9.0,
            pal::SHOT_FIELD,
            0.15 + pulse * 0.1,
        );
    }

    if ship.fuel <= 0.0 {
        fb.circle_stroke(cx, cy, radius + 3.0, pal::HUD_BAD);
    }
}
