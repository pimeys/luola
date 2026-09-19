//! HUD: fuel, shield, objective needle, radar and the game's cards.
//!
//! Deliberately minimal (`docs/game_mechanics.md` §12): fuel, shield state and
//! where the objective is. Everything else is decoration.

use crate::math::V2;
use crate::render::camera::Camera;
use crate::render::fb::Framebuffer;
use crate::render::fx::{Banner, Fx};
use crate::render::palette as pal;
use crate::sim::inputs::Scheme;
use crate::sim::ship::Shield;
use crate::sim::tuning;
use crate::sim::weapons::{self, WeaponId};
use crate::sim::world::{Marker, MarkerKind, RunState, World};

pub fn text_centered(fb: &mut Framebuffer, cx: i32, y: i32, s: &str, scale: i32, color: u32) {
    let w = fb.text_width(s, scale);
    fb.text_scaled(cx - w / 2, y, s, scale, color);
}

fn panel(fb: &mut Framebuffer, x0: i32, y0: i32, x1: i32, y1: i32) {
    fb.rect_fill(x0, y0, x1, y1, pal::HUD_PANEL);
    fb.rect_stroke(x0, y0, x1, y1, pal::HUD_PANEL_EDGE);
}

pub fn format_clock(seconds: f32) -> String {
    let s = seconds.max(0.0);
    format!("{:02}:{:04.1}", (s / 60.0) as i32, s % 60.0)
}

// ---------------------------------------------------------------- status --

pub fn draw_status(fb: &mut Framebuffer, world: &World, hint_alpha: f32, scheme: Scheme) {
    let w = fb.width();
    let fuel = world.ship.fuel;
    let cap = world.ship.fuel_capacity.max(1.0);
    let frac = (fuel / cap).clamp(0.0, 1.0);
    let t = world.elapsed();

    // Fuel gauge.
    fb.text_scaled(8, 6, "FUEL", 1, pal::HUD_DIM);
    let bx = 44;
    let by = 6;
    fb.rect_stroke(bx, by, bx + 122, by + 9, pal::HUD_DIM);
    let low = frac < 0.25;
    let color = if low {
        if (t * 6.0).sin() > -0.2 {
            pal::HUD_BAD
        } else {
            pal::HUD_WARNING
        }
    } else if frac < 0.5 {
        pal::HUD_WARNING
    } else {
        pal::HUD_GOOD
    };
    fb.rect_fill(
        bx + 1,
        by + 1,
        bx + 1 + (120.0 * frac) as i32,
        by + 8,
        color,
    );

    // Shield gauge.
    let (sh_x, sh_y) = (bx + 136, by + 4);
    let charge = world.ship.shield.charge();
    let shield_label = match world.ship.shield {
        Shield::Charged => "SHIELD UP",
        Shield::Absorbing { .. } => "SHIELD HIT",
        Shield::Down { .. } => "SHIELD DOWN",
        Shield::Recharging { .. } => "RECHARGING",
    };
    let shield_color = match world.ship.shield {
        Shield::Charged => pal::SHIELD,
        Shield::Absorbing { .. } => pal::SHIELD_HIT,
        Shield::Down { .. } => pal::HUD_BAD,
        Shield::Recharging { .. } => pal::HUD_WARNING,
    };
    fb.arc(
        sh_x as f32,
        sh_y as f32 + 1.0,
        6.0,
        1.6,
        -std::f32::consts::FRAC_PI_2,
        -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * charge,
        shield_color,
    );
    fb.arc(
        sh_x as f32,
        sh_y as f32 + 1.0,
        6.0,
        0.4,
        0.0,
        std::f32::consts::TAU,
        pal::HUD_DIM,
    );
    fb.text_scaled(sh_x + 12, by, shield_label, 1, shield_color);

    // Carried-load gauge: how much quicker the ship flies than at launch
    // weight, so dumping the magazine reads on screen (mass economy).
    let agi = world.ship.agility();
    let af = ((agi - 1.0) / (tuning::MASS_AGILITY_MAX - 1.0).max(1e-3)).clamp(0.0, 1.0);
    fb.text_scaled(8, 18, "AGI", 1, pal::HUD_DIM);
    let abx = 34;
    fb.rect_stroke(abx, 18, abx + 60, 26, pal::HUD_DIM);
    fb.rect_fill(
        abx + 1,
        19,
        abx + 1 + (58.0 * af) as i32,
        25,
        pal::HUD_ACCENT,
    );
    fb.text_scaled(
        abx + 64,
        18,
        &format!("{:+.0}%", (agi - 1.0) * 100.0),
        1,
        pal::HUD_TEXT,
    );

    // Clock and score.
    let clock = format_clock(world.elapsed());
    let score = format!("SCORE {:06}", world.score.total());
    fb.text_scaled(
        w - 8 - fb.text_width(&score, 1),
        6,
        &score,
        1,
        pal::HUD_TEXT,
    );
    let cw = fb.text_width(&clock, 1);
    fb.text_scaled(w - 8 - cw, 18, &clock, 1, pal::HUD_ACCENT);
    let name = world.level.name.to_uppercase();
    fb.text_scaled(w - 8 - fb.text_width(&name, 1), 30, &name, 1, pal::HUD_DIM);

    // Loadout readout, under the fuel gauge.
    draw_weapon_panel(fb, world);

    // Escape countdown: the loudest thing on screen when it runs.
    if let Some(escape) = world.escape {
        let text = format!("ESCAPE {}", format_clock(escape));
        let blink = if escape < 10.0 {
            (t * 5.0).sin() > -0.3
        } else {
            true
        };
        if blink {
            text_centered(fb, w / 2, 26, &text, 2, pal::HUD_BAD);
            text_centered(fb, w / 2, 44, "THE CAVE IS COLLAPSING", 1, pal::HUD_WARNING);
        }
    }

    draw_objective_text(fb, world);
    draw_hint(fb, hint_alpha, scheme);
}

/// The loadout readout: the mounted special with its ammo and reload, then the
/// gun, which is always there.
///
/// It sits under the fuel gauge and starts at y 51, below the escape countdown's
/// centred banner (which owns the middle of y 26..50), so nothing the player
/// needs at the end of a run is ever drawn over.
fn draw_weapon_panel(fb: &mut Framebuffer, world: &World) {
    let (x0, y0, x1, y1) = (8, 51, 158, 74);
    panel(fb, x0, y0, x1, y1);
    let loadout = &world.ship.loadout;
    let spec = weapons::spec(loadout.special);
    let t = world.elapsed();

    // The pip wears the weapon's own colour, so the panel and the shots agree.
    let pip = pal::weapon_color(loadout.special);
    fb.rect_fill(12, 53, 18, 59, pip);
    fb.text_scaled(22, 53, &spec.name.to_uppercase(), 1, pal::HUD_TEXT);

    // An empty magazine is a failure state, not a number: it blinks on the same
    // cadence as the fuel warning.
    let ammo = format!("{}/{}", loadout.ammo, spec.ammo);
    let ammo_color = if loadout.empty() {
        if (t * 6.0).sin() > -0.2 {
            pal::HUD_BAD
        } else {
            pal::HUD_WARNING
        }
    } else {
        pal::HUD_TEXT
    };
    fb.text_scaled(x1 - 4 - fb.text_width(&ammo, 1), 53, &ammo, 1, ammo_color);

    // Reload pip: it fills as the cooldown runs down, so full reads as "ready".
    fb.rect_fill(12, 62, x1 - 4, 64, pal::HUD_PANEL_EDGE);
    let reload = spec.reload.max(0.01);
    let charged = if loadout.cd <= 0.0 {
        1.0
    } else {
        (1.0 - loadout.cd / reload).clamp(0.0, 1.0)
    };
    let filled = ((x1 - 5 - 12) as f32 * charged) as i32;
    if filled > 0 {
        fb.rect_fill(12, 62, 12 + filled, 64, pip);
    }

    // The gun is the floor of the loadout: no ammo, no reload, always mounted.
    let gun = pal::weapon_color(WeaponId::Gun);
    fb.rect_fill(12, 67, 18, 73, gun);
    fb.text_scaled(22, 67, "GUN", 1, pal::HUD_TEXT);
    // The font has no infinity glyph, so the mark is two drawn lobes.
    let ix = (22 + fb.text_width("GUN", 1) + 5) as f32;
    fb.circle_stroke(ix, 70.0, 2.5, gun);
    fb.circle_stroke(ix + 5.0, 70.0, 2.5, gun);

    // The base is where the weapon changes, so say so while the ship is parked
    // on a pad and there is a roster to step.
    if world.docked_on_pad() && world.level.weapons.len() > 1 {
        let list = &world.level.weapons;
        // A special the level does not list falls back to the top of the roster.
        let idx = list
            .iter()
            .position(|w| *w == loadout.special)
            .unwrap_or(list.len() - 1);
        let next = list[(idx + 1) % list.len()];
        let prompt = format!(
            "TURN KEYS STEP THE ROSTER, NEXT: {}",
            weapons::spec(next).name.to_uppercase()
        );
        // Below the panel, still clear of the radar's top edge.
        fb.text_scaled(x0, 75, &prompt, 1, pal::HUD_ACCENT);
    }
}

fn draw_objective_text(fb: &mut Framebuffer, world: &World) {
    let text = if !world.exit_open() {
        "OBJECTIVE: DESTROY THE REACTOR TO OPEN THE EXIT"
    } else if world.level.require_pod && !world.pods.iter().any(|p| p.delivered) {
        "OBJECTIVE: BEAM THE PAYLOAD TO THE EXIT"
    } else {
        "OBJECTIVE: REACH THE EXIT"
    };
    let y = fb.height() - 11;
    text_centered(fb, fb.width() / 2, y, text, 1, pal::HUD_DIM);
}

fn draw_hint(fb: &mut Framebuffer, alpha: f32, scheme: Scheme) {
    if alpha <= 0.02 {
        return;
    }
    let text = Fx::scheme_hint(scheme);
    let w = fb.text_width(text, 1);
    let x = (fb.width() - w) / 2;
    let y = fb.height() - 26;
    fb.rect_fill(x - 4, y - 3, x + w + 4, y + 9, pal::HUD_PANEL);
    fb.blend(x, y, pal::HUD_ACCENT, alpha);
    fb.text_scaled(x, y, text, 1, pal::HUD_ACCENT);
}

/// Off-screen indicators for the objective and for fuel (§8.5: never punish the
/// player for something they could not see).
pub fn draw_indicators(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let w = fb.width();
    let h = fb.height();
    let view = cam.view();
    let objective = world.objective_target();
    let mut targets: Vec<(Marker, bool)> = Vec::new();
    if let Some(o) = objective {
        targets.push((o, true));
    }
    for m in world.markers() {
        if m.kind == MarkerKind::Fuel {
            targets.push((m, false));
        }
    }
    for (marker, primary) in targets {
        if view.inflate(-24.0).contains(marker.p) {
            continue;
        }
        let (x, y) = cam.world_to_screen(marker.p);
        let c = V2::new(w as f32 * 0.5, h as f32 * 0.5);
        let dir = V2::new(x - c.x, y - c.y).normalized();
        if dir.len_sq() < 1e-6 {
            continue;
        }
        let margin = 18.0;
        // March the direction until it reaches the screen border.
        let tx = if dir.x.abs() > 1e-4 {
            ((if dir.x > 0.0 {
                w as f32 - margin
            } else {
                margin
            }) - c.x)
                / dir.x
        } else {
            f32::INFINITY
        };
        let ty = if dir.y.abs() > 1e-4 {
            ((if dir.y > 0.0 {
                h as f32 - margin
            } else {
                margin
            }) - c.y)
                / dir.y
        } else {
            f32::INFINITY
        };
        let dist = tx.min(ty).max(0.0);
        let px = c.x + dir.x * dist;
        let py = c.y + dir.y * dist;
        let color = marker_color(marker.kind);
        let size = if primary { 6.0 } else { 4.0 };
        let tip = V2::new(px, py);
        let back = -dir * size;
        let side = dir.perp() * size * 0.7;
        fb.poly_fill(
            &[
                (tip.x, tip.y),
                (tip.x + back.x + side.x, tip.y + back.y + side.y),
                (tip.x + back.x - side.x, tip.y + back.y - side.y),
            ],
            color,
        );
        let meters = (marker.p.dist(world.ship.body.p) / 10.0).round() as i32;
        let label = format!("{meters}");
        let lx = (px - dir.x * 16.0) as i32 - fb.text_width(&label, 1) / 2;
        let ly = (py - dir.y * 16.0) as i32 - 3;
        fb.text_scaled(lx, ly, &label, 1, color);
    }
}

fn marker_color(kind: MarkerKind) -> u32 {
    match kind {
        MarkerKind::Pod => pal::POD,
        MarkerKind::Reactor => pal::REACTOR,
        MarkerKind::Exit => pal::EXIT,
        MarkerKind::Fuel => pal::FUEL_POD,
        MarkerKind::Pad => pal::PAD,
        MarkerKind::Turret => pal::TURRET,
        MarkerKind::Drone => pal::DRONE,
        MarkerKind::Mine => pal::MINE,
        MarkerKind::Ship => pal::SHIP,
    }
}

// ----------------------------------------------------------------- radar --

pub fn draw_radar(fb: &mut Framebuffer, world: &World, cam: &Camera) {
    let bounds = world.level.bounds;
    let box_w = 132.0;
    let box_h = 78.0;
    let x0 = 8;
    let y0 = fb.height() - box_h as i32 - 20;
    panel(fb, x0, y0, x0 + box_w as i32, y0 + box_h as i32);
    let scale = (box_w / bounds.w).min(box_h / bounds.h);
    let ox = x0 as f32 + (box_w - bounds.w * scale) * 0.5;
    let oy = y0 as f32 + (box_h - bounds.h * scale) * 0.5;

    let to_map =
        |p: V2| -> (f32, f32) { (ox + (p.x - bounds.x) * scale, oy + (p.y - bounds.y) * scale) };

    // Cave silhouette from the live grid: what is solid *now*, with the cells
    // the player has dug away shown separately so their tunnel is readable.
    let live = world.terrain.spans();
    let pristine = &world.level.terrain;
    let inner_x = x0 + 1;
    let top = y0 as f32 + 1.0;
    let bottom = (y0 + box_h as i32) as f32 - 1.0;
    for rx in 0..(box_w as i32) {
        let wx = bounds.x + (rx as f32 - (ox - x0 as f32)) / scale;
        if wx < bounds.x || wx > bounds.right() {
            continue;
        }
        for &(sy0, sy1) in live.column(wx as i32) {
            let y_a = oy + (sy0 as f32 - bounds.y) * scale;
            let y_b = oy + (sy1 as f32 - bounds.y) * scale;
            let ya = y_a.max(top);
            let yb = y_b.min(bottom);
            if yb > ya {
                fb.vspan(inner_x + rx, ya as i32, yb as i32, pal::WALL_EDGE);
            }
        }
        // One sample per map pixel: authored solid, dug away since.
        for ry in 0..(box_h as i32) {
            let wy = bounds.y + (ry as f32 - (oy - y0 as f32)) / scale;
            let wyi = wy as i32;
            if !pristine.solid(wx as i32, wyi) || world.terrain.solid(wx as i32, wyi) {
                continue;
            }
            let ya = oy + (wyi as f32 - bounds.y) * scale;
            if ya > top && ya < bottom {
                fb.set(inner_x + rx, ya as i32, pal::RADAR_DUG);
            }
        }
    }

    for marker in world.markers() {
        let (mx, my) = to_map(marker.p);
        let color = marker_color(marker.kind);
        fb.set(mx as i32, my as i32, color);
        fb.set(mx as i32 + 1, my as i32, color);
        fb.set(mx as i32, my as i32 + 1, color);
    }

    // View rectangle and ship.
    let view = cam.view();
    let (vx0, vy0) = to_map(V2::new(view.left(), view.top()));
    let (vx1, vy1) = to_map(V2::new(view.right(), view.bottom()));
    fb.rect_stroke(vx0 as i32, vy0 as i32, vx1 as i32, vy1 as i32, pal::HUD_DIM);
    let (sx, sy) = to_map(world.ship.body.p);
    fb.circle_fill(sx, sy, 1.6, pal::SHIP);
    fb.text_scaled(
        x0 + 3,
        y0 + box_h as i32 - 10,
        &world.level.name.to_uppercase(),
        1,
        pal::HUD_DIM,
    );
}

// ---------------------------------------------------------------- banner --

pub fn draw_banner(fb: &mut Framebuffer, banner: &Banner) {
    let cx = fb.width() / 2;
    let y = 74;
    // Typewriter reveal, in place of alpha fading: it reads as an announcement.
    let revealed = ((banner.max_ttl - banner.ttl) * 34.0) as usize;
    let shown: String = banner.text.chars().take(revealed.max(1)).collect();
    let w = fb
        .text_width(&banner.text, 2)
        .max(fb.text_width(&banner.sub, 1));
    fb.dim(cx - w / 2 - 8, y - 5, cx + w / 2 + 8, y + 22, 0.35);
    text_centered(fb, cx, y, &shown, 2, banner.color);
    if !banner.sub.is_empty() {
        text_centered(fb, cx, y + 15, &banner.sub, 1, pal::HUD_TEXT);
    }
}

// --------------------------------------------------------------- screens --

pub fn draw_vignette(fb: &mut Framebuffer) {
    let w = fb.width();
    let h = fb.height();
    for i in 0..10 {
        let f = 1.0 - i as f32 * 0.02;
        fb.dim(0, i, w - 1, i, f);
        fb.dim(0, h - 1 - i, w - 1, h - 1 - i, f);
        fb.dim(i, 0, i, h - 1, f);
        fb.dim(w - 1 - i, 0, w - 1 - i, h - 1, f);
    }
}

pub fn draw_title(fb: &mut Framebuffer, t: f32, levels: usize) {
    let w = fb.width();
    let h = fb.height();
    fb.rect_fill(0, 0, w - 1, h - 1, pal::SPACE);
    for i in 0..levels.min(12) {
        let y = 40 + i as i32 * 8;
        fb.rect_fill(
            20,
            y,
            20 + (w as f32 * (0.2 + 0.06 * i as f32)) as i32,
            y + 3,
            pal::WALL_FILL,
        );
    }
    text_centered(fb, w / 2, h / 2 - 60, "LUOLALENTELY", 3, pal::HUD_TEXT);
    text_centered(
        fb,
        w / 2,
        h / 2 - 30,
        "A GRAVITY SHOOTER",
        1,
        pal::HUD_ACCENT,
    );
    let blink = (t * 2.0).sin() > -0.4;
    if blink {
        text_centered(fb, w / 2, h / 2 + 6, "PRESS ENTER TO FLY", 2, pal::HUD_TEXT);
    }
    text_centered(
        fb,
        w / 2,
        h / 2 + 34,
        "C: CONTROL SCHEME   H: HELP   ESC: QUIT",
        1,
        pal::HUD_DIM,
    );
    text_centered(
        fb,
        w / 2,
        h / 2 + 50,
        "GRAB THE PAYLOAD. MIND THE FUEL. THE CAVE IS THE ENEMY.",
        1,
        pal::HUD_DIM,
    );
}

pub fn draw_briefing(fb: &mut Framebuffer, world: &World, level_index: usize, total: usize) {
    let w = fb.width();
    let h = fb.height();
    fb.rect_fill(0, 0, w - 1, h - 1, pal::SPACE);
    text_centered(
        fb,
        w / 2,
        60,
        &format!("LEVEL {} OF {}", level_index + 1, total),
        1,
        pal::HUD_ACCENT,
    );
    text_centered(
        fb,
        w / 2,
        78,
        &world.level.name.to_uppercase(),
        3,
        pal::HUD_TEXT,
    );
    let briefing = if world.level.briefing.is_empty() {
        "NO BRIEFING".to_string()
    } else {
        world.level.briefing.to_uppercase()
    };
    text_centered(fb, w / 2, 112, &briefing, 1, pal::HUD_TEXT);

    let lines = [
        format!("GRAVITY {:>5.1}", world.level.gravity),
        format!("FUEL    {:>5.1}", world.level.start_fuel),
        format!(
            "HAZARDS {} TURRETS {} DRONES {} MINES",
            world.level.turrets.len(),
            world.level.drones.len(),
            world.level.mines.len()
        ),
        format!(
            "FURNITURE {} FUEL PODS {} PADS",
            world.level.fuel_pods.len(),
            world.level.pads.len()
        ),
        format!(
            "WEAPONS {} START {}",
            world.level.weapons.len(),
            weapons::spec(world.level.start_weapon).name.to_uppercase()
        ),
    ];
    for (i, line) in lines.iter().enumerate() {
        text_centered(fb, w / 2, 150 + i as i32 * 12, line, 1, pal::HUD_DIM);
    }

    let mut objective = if world.level.exit_locked {
        "DESTROY THE REACTOR, THEN FLY OUT BEFORE THE CAVE COLLAPSES".to_string()
    } else if world.level.require_pod {
        "BEAM THE PAYLOAD AND CARRY IT TO THE EXIT".to_string()
    } else {
        "REACH THE EXIT".to_string()
    };
    // A one-weapon level prescribes the loadout instead of offering a choice, so
    // the weapon is part of the plan; with a roster to step, it is the player's.
    if world.level.weapons.len() == 1 {
        objective.push_str(&format!(
            " - YOUR ONLY SPECIAL IS {}",
            weapons::spec(world.level.weapons[0]).name.to_uppercase()
        ));
    }
    text_centered(fb, w / 2, 210, &objective, 1, pal::HUD_WARNING);
    text_centered(
        fb,
        w / 2,
        h - 40,
        "PRESS ENTER TO LAUNCH   ESC FOR THE MENU",
        1,
        pal::HUD_ACCENT,
    );
}

pub fn draw_pause(fb: &mut Framebuffer, world: &World, scheme: Scheme, t: f32) {
    let w = fb.width();
    let h = fb.height();
    fb.dim(0, 0, w - 1, h - 1, 0.45);
    text_centered(fb, w / 2, h / 2 - 60, "PAUSED", 3, pal::HUD_TEXT);
    text_centered(
        fb,
        w / 2,
        h / 2 - 24,
        Fx::scheme_hint(scheme),
        1,
        pal::HUD_ACCENT,
    );
    text_centered(
        fb,
        w / 2,
        h / 2 - 6,
        "ESC RESUME   R RESTART   M MENU   C SWITCH SCHEME",
        1,
        pal::HUD_DIM,
    );
    text_centered(
        fb,
        w / 2,
        h / 2 + 14,
        &format!(
            "TIME {}   SCORE {}   FUEL {:.0}   SHIELD {}",
            format_clock(world.elapsed()),
            world.score.total(),
            world.ship.fuel,
            if world.ship.shield.absorbing() {
                "UP"
            } else {
                "DOWN"
            }
        ),
        1,
        pal::HUD_TEXT,
    );
    // The loadout reads on its own line under the state line: appended to it the
    // combined text would clip TIME and SCORE at the 320x180 minimum.
    let loadout = &world.ship.loadout;
    let spec = weapons::spec(loadout.special);
    text_centered(
        fb,
        w / 2,
        h / 2 + 26,
        &format!(
            "WPN {} {}/{}",
            spec.name.to_uppercase(),
            loadout.ammo,
            spec.ammo
        ),
        1,
        pal::HUD_TEXT,
    );
    text_centered(fb, w / 2, h / 2 + 40, "PAYLOAD TELEMETRY", 1, pal::HUD_DIM);
    let pods = if world.pods.is_empty() {
        "NONE".to_string()
    } else {
        world
            .pods
            .iter()
            .map(|p| {
                if p.delivered {
                    "DELIVERED"
                } else if p.attached {
                    "ON BEAM"
                } else if p.alive {
                    "IN CAVE"
                } else {
                    "LOST"
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    text_centered(fb, w / 2, h / 2 + 52, &pods, 1, pal::HUD_TEXT);
    let _ = t;
}

pub fn draw_help(fb: &mut Framebuffer) {
    let w = fb.width();
    let h = fb.height();
    fb.rect_fill(0, 0, w - 1, h - 1, pal::SPACE);
    text_centered(fb, w / 2, 30, "HOW TO FLY", 2, pal::HUD_TEXT);
    // Twelve lines at 9 px of pitch: the block has to clear the return prompt
    // even at the 320x180 minimum, which the old 13 lines at 12 px did not.
    let lines = [
        "GRAVITY IS ALWAYS ON AND ONLY THRUST FIGHTS IT. NO DRAG, NO SPEED LIMIT.",
        "TOUCHING THE CAVE IS DEATH. THE SHIELD EATS ONE HIT, THEN NEEDS A MOMENT.",
        "THRUST BURNS FUEL. A DRY TANK KILLS THE ENGINES AND THE SHIELD.",
        "LAND SLOWLY ON A PAD TO REFUEL, RESET THE SHIELD AND REARM.",
        "HOLD BEAM NEAR THE PAYLOAD; THE ROD SWINGS WHEN YOU TURN.",
        "SHOOT TURRETS BEFORE THEY SIGHT YOU. THE REACTOR OPENS THE EXIT.",
        "CARRY THE PAYLOAD INTO THE EXIT; A BLOWN REACTOR COLLAPSES THE CAVE.",
        "THE GUN NEVER RUNS OUT. F FIRES THE SPECIAL YOU ARE CARRYING.",
        "SPECIALS SWAP ONLY PARKED ON A PAD: THE TURN KEYS STEP THE ROSTER, LANDING REARMS.",
        "CLASSIC: A/D ROTATE, W THRUST, SPACE GUN, F SPECIAL, E BEAM",
        "MODERN: WASD FLY, MOUSE AIM, LMB GUN, F SPECIAL, RMB BEAM",
        "GRANULAR WALLS HOLD YOU: SHOOT FREE. X DUMPS AMMO TO FLY LIGHTER.",
        "P PAUSE   R RESTART   C SCHEME   F1 RECORDING   ESC MENU",
    ];
    for (i, line) in lines.iter().enumerate() {
        text_centered(fb, w / 2, 44 + i as i32 * 9, line, 1, pal::HUD_TEXT);
    }
    text_centered(
        fb,
        w / 2,
        h - 24,
        "PRESS ENTER TO RETURN",
        1,
        pal::HUD_ACCENT,
    );
}

pub fn draw_results(fb: &mut Framebuffer, world: &World, failure: Option<&str>, next: bool) {
    let w = fb.width();
    let h = fb.height();
    fb.dim(0, 0, w - 1, h - 1, 0.55);
    let (title, color) = match world.state {
        RunState::Complete { outcome, .. } => (outcome.label().to_string(), pal::HUD_GOOD),
        RunState::Failed { reason, .. } => (reason.label().to_string(), pal::HUD_BAD),
        RunState::ShipLost { .. } => ("SHIP LOST".to_string(), pal::HUD_BAD),
        RunState::Flying => ("RUNNING".to_string(), pal::HUD_TEXT),
    };
    text_centered(fb, w / 2, 60, &title, 3, color);
    if let Some(f) = failure {
        text_centered(fb, w / 2, 88, f, 1, pal::HUD_DIM);
    }
    text_centered(
        fb,
        w / 2,
        108,
        &format!(
            "{}   {}",
            world.level.name.to_uppercase(),
            format_clock(world.elapsed())
        ),
        1,
        pal::HUD_ACCENT,
    );

    let lines = world.score.lines();
    for (i, (label, value)) in lines.iter().enumerate() {
        let y = 140 + i as i32 * 12;
        let text = format!("{label:<18}{value:>8}");
        let wtext = fb.text_width(&text, 1);
        fb.text_scaled(w / 2 - wtext / 2, y, &text, 1, pal::HUD_TEXT);
    }
    let total = format!("{:<18}{:>8}", "TOTAL", world.score.total());
    let tw = fb.text_width(&total, 1);
    fb.text_scaled(
        w / 2 - tw / 2,
        140 + lines.len() as i32 * 12 + 6,
        &total,
        1,
        pal::HUD_ACCENT,
    );

    let footer = if next {
        "ENTER: NEXT LEVEL   R: FLY AGAIN   ESC: MENU"
    } else {
        "R: FLY AGAIN   ESC: MENU"
    };
    text_centered(fb, w / 2, h - 40, footer, 1, pal::HUD_WARNING);
}

pub fn draw_campaign_complete(fb: &mut Framebuffer, total_score: i32, total_time: f32) {
    let w = fb.width();
    let h = fb.height();
    fb.rect_fill(0, 0, w - 1, h - 1, pal::SPACE);
    text_centered(fb, w / 2, h / 2 - 50, "CAMPAIGN COMPLETE", 3, pal::HUD_GOOD);
    text_centered(
        fb,
        w / 2,
        h / 2 - 10,
        &format!("FINAL SCORE {total_score}"),
        2,
        pal::HUD_TEXT,
    );
    text_centered(
        fb,
        w / 2,
        h / 2 + 16,
        &format!("TOTAL FLIGHT TIME {}", format_clock(total_time)),
        1,
        pal::HUD_ACCENT,
    );
    text_centered(
        fb,
        w / 2,
        h / 2 + 44,
        "THE FANSITE SAYS YOU NOW HAVE 9 700 HOURS TO GO",
        1,
        pal::HUD_DIM,
    );
    text_centered(
        fb,
        w / 2,
        h - 40,
        "PRESS ENTER FOR THE MENU",
        1,
        pal::HUD_WARNING,
    );
}

/// Small legend used on menu screens so the vocabulary is learned up front.
pub fn draw_legend(fb: &mut Framebuffer, x: i32, y: i32) {
    let items = [
        (MarkerKind::Pod, "PAYLOAD"),
        (MarkerKind::Reactor, "REACTOR"),
        (MarkerKind::Exit, "EXIT"),
        (MarkerKind::Fuel, "FUEL"),
        (MarkerKind::Pad, "PAD"),
        (MarkerKind::Turret, "TURRET"),
        (MarkerKind::Drone, "DRONE"),
        (MarkerKind::Mine, "MINE"),
    ];
    for (i, (kind, label)) in items.iter().enumerate() {
        let iy = y + i as i32 * 10;
        let color = marker_color(*kind);
        fb.rect_fill(x, iy, x + 4, iy + 4, color);
        fb.text_scaled(x + 8, iy - 2, label, 1, pal::HUD_DIM);
    }
}

/// Distance readout for the objective, drawn under the fuel gauge.
pub fn draw_objective_distance(fb: &mut Framebuffer, world: &World) {
    if let Some(target) = world.objective_target() {
        let d = target.p.dist(world.ship.body.p) / 10.0;
        let label = match target.kind {
            MarkerKind::Pod => format!("PAYLOAD {:.0}M", d),
            MarkerKind::Reactor => format!("REACTOR {:.0}M", d),
            MarkerKind::Exit => format!("EXIT {:.0}M", d),
            _ => format!("TARGET {:.0}M", d),
        };
        fb.text_scaled(8, 20, &label, 1, marker_color(target.kind));
    }
}
