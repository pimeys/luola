//! Tests for the presentation path that can be checked without a window: the
//! integer-scaled, letterboxed blit from the internal framebuffer to the window
//! buffer.

use std::sync::Arc;

use luola::app::blit;
use luola::math::V2;
use luola::render::camera::Camera;
use luola::render::fb::{Framebuffer, rgb};
use luola::render::scene;
use luola::sim::entities::{Cloud, Gadget};
use luola::sim::inputs::Scheme;
use luola::sim::level::Level;
use luola::sim::weapons::{CloudKind, GadgetKind, WeaponId};
use luola::sim::world::World;

const BG: u32 = rgb(255, 0, 255);

#[test]
fn blit_nearest_neighbour_scales_each_pixel_to_a_block() {
    let mut fb = Framebuffer::new(2, 2);
    let a = rgb(10, 20, 30);
    let b = rgb(40, 50, 60);
    let c = rgb(70, 80, 90);
    let d = rgb(100, 110, 120);
    fb.set(0, 0, a);
    fb.set(1, 0, b);
    fb.set(0, 1, c);
    fb.set(1, 1, d);

    let (w, h, scale) = (4i32, 4i32, 2i32);
    let mut out = vec![0u32; (w * h) as usize];
    blit(&fb, &mut out, w, h, scale, (0, 0), BG);

    let at = |x: i32, y: i32| out[(y * w + x) as usize];
    assert_eq!(at(0, 0), a);
    assert_eq!(at(1, 0), a);
    assert_eq!(at(0, 1), a);
    assert_eq!(at(1, 1), a);
    assert_eq!(at(2, 0), b);
    assert_eq!(at(3, 1), b);
    assert_eq!(at(0, 2), c);
    assert_eq!(at(3, 3), d);
}

#[test]
fn blit_centres_the_frame_and_fills_the_letterbox() {
    let mut fb = Framebuffer::new(2, 2);
    fb.clear(rgb(1, 2, 3));
    // 2x2 framebuffer at 2x inside a 6x6 window: 4x4 of content, 1px bars.
    let (w, h) = (6i32, 6i32);
    let mut out = vec![0u32; (w * h) as usize];
    blit(&fb, &mut out, w, h, 2, (1, 1), BG);
    let at = |x: i32, y: i32| out[(y * w + x) as usize];
    assert_eq!(at(0, 0), BG, "left bar must stay background");
    assert_eq!(at(5, 0), BG, "right bar must stay background");
    assert_eq!(at(1, 1), rgb(1, 2, 3));
    assert_eq!(at(4, 4), rgb(1, 2, 3));
    assert_eq!(at(0, 5), BG);
}

#[test]
fn blit_clips_instead_of_wrapping() {
    let mut fb = Framebuffer::new(4, 4);
    fb.clear(rgb(9, 9, 9));
    let (w, h) = (4i32, 4i32);
    let mut out = vec![BG; (w * h) as usize];
    // Offset so most of the frame is off-screen; nothing may wrap to the far side.
    blit(&fb, &mut out, w, h, 2, (-20, -20), BG);
    assert!(
        out.iter().all(|p| *p == BG),
        "off-screen frame leaked pixels into the window"
    );

    // Partially visible, in a window big enough to have bars left over: the
    // frame's right/bottom blocks land on the left/top of the window and the
    // remainder is background.
    let (w, h) = (10i32, 10i32);
    let mut out = vec![BG; (w * h) as usize];
    blit(&fb, &mut out, w, h, 2, (-2, -2), BG);
    let at = |x: i32, y: i32| out[(y * w + x) as usize];
    assert_eq!(at(0, 0), rgb(9, 9, 9), "clipped frame must still be drawn");
    assert_eq!(at(5, 5), rgb(9, 9, 9));
    assert_eq!(at(6, 0), BG, "beyond the frame must stay background");
    assert_eq!(at(9, 9), BG);
}

#[test]
fn blit_rejects_degenerate_scale() {
    let mut fb = Framebuffer::new(2, 2);
    fb.clear(rgb(5, 5, 5));
    let mut out = vec![BG; 4];
    blit(&fb, &mut out, 2, 2, 0, (0, 0), BG);
    assert!(out.iter().all(|p| *p == BG));
}

// ------------------------------------------------------- weapon overlays ---
//
// The simulation hands the renderer clouds, gadgets and a tether; a pass that
// forgets to draw one of them is invisible to every other kind of test, because
// nothing else reads a framebuffer.

const SCENE_LEVEL: &str = r#"
name = "Scene"
gravity = 20.0
fuel = 100.0

[player]
pos = [160.0, 90.0]

[[wall]]
points = [[0, 0], [320, 0], [320, 20], [0, 20]]

[[wall]]
points = [[0, 160], [320, 160], [320, 180], [0, 160]]

[[wall]]
points = [[0, 0], [20, 0], [20, 180], [0, 180]]

[[wall]]
points = [[300, 0], [320, 0], [320, 180], [300, 180]]

[[exit]]
pos = [280.0, 40.0]
size = [30.0, 30.0]
"#;

/// Renders the world into a fresh framebuffer with the camera parked on `focus`.
fn render(world: &World, focus: V2) -> Vec<u32> {
    let mut fb = Framebuffer::new(320, 180);
    let mut cam = Camera::new(320.0, 180.0);
    cam.snap(focus, world.level.bounds);
    scene::draw_world(&mut fb, world, &cam);
    fb.pixels().to_vec()
}

/// How many pixels differ between two frames inside a disc around `p`.
fn changed_near(cam: &Camera, before: &[u32], after: &[u32], p: V2, radius: f32) -> usize {
    let (cx, cy) = cam.world_to_screen(p);
    let mut changed = 0;
    for y in (cy - radius) as i32..=(cy + radius) as i32 {
        for x in (cx - radius) as i32..=(cx + radius) as i32 {
            if x < 0 || y < 0 || x >= 320 || y >= 180 {
                continue;
            }
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let i = (y * 320 + x) as usize;
            if before[i] != after[i] {
                changed += 1;
            }
        }
    }
    changed
}

#[test]
fn weapon_clouds_gadgets_and_the_tether_are_drawn() {
    let level = Arc::new(Level::parse(SCENE_LEVEL).expect("scene level parses"));
    let mut world = World::new(level, Scheme::Classic, 3);
    let focus = world.ship.body.p;
    let mut cam = Camera::new(320.0, 180.0);
    cam.snap(focus, world.level.bounds);
    let before = render(&world, focus);

    // A poison cloud hanging in the air, a gravity well and a trooper post on
    // the floor, and the harpoon's line running up to the ceiling.
    let cloud_p = V2::new(160.0, 60.0);
    world.clouds.push(Cloud {
        p: cloud_p,
        v: V2::ZERO,
        kind: CloudKind::Poison,
        radius: 34.0,
        ttl: 4.0,
        age: 1.0,
        dps: 0.5,
        acc: 0.0,
        push: 0.0,
        from_player: true,
        weapon: WeaponId::Poison,
    });
    let well_p = V2::new(230.0, 110.0);
    world.gadgets.push(Gadget {
        p: well_p,
        prev: well_p,
        v: V2::ZERO,
        kind: GadgetKind::Well,
        weapon: WeaponId::Gravitor,
        from_player: true,
        age: 1.0,
        ttl: 4.0,
        armed: true,
        resting: false,
        blast: 0.0,
        radius: 120.0,
        cd: 0.0,
    });
    let anchor = V2::new(160.0, 24.0);
    world.ship.tether = Some((anchor, 4.0));

    let after = render(&world, focus);
    assert_ne!(before, after, "the new passes drew nothing at all");

    let cloud_px = changed_near(&cam, &before, &after, cloud_p, 30.0);
    assert!(cloud_px > 100, "the cloud painted only {cloud_px} px");
    let well_px = changed_near(&cam, &before, &after, well_p, 24.0);
    assert!(well_px > 20, "the well painted only {well_px} px");
    // The tether runs from the hull up to the ceiling anchor: sample it midway.
    let mid = (world.ship.body.p + anchor) * 0.5;
    let tether_px = changed_near(&cam, &before, &after, mid, 16.0);
    assert!(tether_px > 10, "the tether painted only {tether_px} px");

    // And an empty loadout is visible on the HUD panel without panicking.
    world.ship.loadout.ammo = 0;
    let mut fb = Framebuffer::new(320, 180);
    luola::render::hud::draw_status(&mut fb, &world, 0.0, Scheme::Classic);
    assert!(
        fb.pixels().iter().any(|p| *p != 0),
        "the HUD drew nothing for an empty special"
    );
}

/// The base prompt is state-dependent: it only appears while the ship is parked
/// on a pad in a cave that offers a choice of weapons, which is the moment the
/// player needs to know the turn keys do something else.
#[test]
fn the_docked_prompt_appears_only_at_a_base() {
    const PAD_LEVEL: &str = r#"
name = "Base"
gravity = 2.0
fuel = 100.0

[weapons]
available = ["shotgun", "mine"]
start = "shotgun"

[player]
pos = [160.0, 130.0]

[[wall]]
points = [[0, 0], [320, 0], [320, 20], [0, 20]]

[[wall]]
points = [[0, 150], [320, 150], [320, 180], [0, 150]]

[[wall]]
points = [[0, 0], [20, 0], [20, 180], [0, 180]]

[[wall]]
points = [[300, 0], [320, 0], [320, 180], [300, 180]]

[[exit]]
pos = [280.0, 40.0]
size = [30.0, 30.0]

[[pad]]
pos = [160.0, 145.0]
size = [64.0, 10.0]
"#;
    let level = Arc::new(Level::parse(PAD_LEVEL).expect("pad level parses"));
    let mut world = World::new(level, Scheme::Classic, 1);
    assert!(world.docked_on_pad(), "the ship should start parked");

    let strip = |fb: &Framebuffer| -> Vec<u32> {
        // The prompt's band, under the weapon panel.
        let mut out = Vec::new();
        for y in 74..84 {
            for x in 0..320 {
                out.push(fb.pixels()[(y * 320 + x) as usize]);
            }
        }
        out
    };
    let mut docked = Framebuffer::new(320, 180);
    luola::render::hud::draw_status(&mut docked, &world, 0.0, Scheme::Classic);

    world.ship.body.p.y -= 60.0;
    world.ship.prev = world.ship.body;
    assert!(!world.docked_on_pad());
    let mut flying = Framebuffer::new(320, 180);
    luola::render::hud::draw_status(&mut flying, &world, 0.0, Scheme::Classic);

    assert_ne!(
        strip(&docked),
        strip(&flying),
        "the docked prompt did not appear at the base"
    );
}

// ---------------------------------------------------------------- gates ----

/// A cave with one `always` gate in the middle of the playfield.
const GATE_LEVEL: &str = r#"
name = "GateScene"
gravity = 20.0
fuel = 100.0

[player]
pos = [60.0, 90.0]

[[wall]]
points = [[0, 0], [320, 0], [320, 20], [0, 20]]

[[wall]]
points = [[0, 160], [320, 160], [320, 180], [0, 160]]

[[wall]]
points = [[0, 0], [20, 0], [20, 180], [0, 180]]

[[wall]]
points = [[300, 0], [320, 0], [320, 180], [300, 180]]

[[exit]]
pos = [280.0, 40.0]
size = [30.0, 30.0]

[[gate]]
pos = [150.0, 70.0]
size = [40.0, 30.0]
to = [220.0, 70.0]
speed = 40.0
"#;

/// A gate that is switched off leaves no pixels; switching it on paints a slab
/// where it sits. This is the *only* thing that reads the gate renderer, so a
/// pass that forgets to draw one would otherwise be invisible.
#[test]
fn an_inactive_gate_is_invisible_and_an_active_one_paints() {
    let level = Arc::new(Level::parse(GATE_LEVEL).expect("gate scene parses"));
    let mut world = World::new(level, Scheme::Classic, 1);
    let gate = world.gates[0].rect.center();
    assert!(world.gates[0].active, "an always gate starts live");

    let lit = render(&world, gate);
    world.gates[0].active = false;
    let dark = render(&world, gate);

    let mut cam = Camera::new(320.0, 180.0);
    cam.snap(gate, world.level.bounds);
    let changed = changed_near(&cam, &lit, &dark, gate, 60.0);
    assert!(
        changed > 200,
        "an active gate must paint its slab, changed {changed} px"
    );
}
