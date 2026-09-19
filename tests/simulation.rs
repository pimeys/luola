//! Behaviour tests for the simulation, the terrain and the replay format.
//!
//! These defend the contracts the game actually depends on: exact determinism,
//! swept collision at speed, the shield's frame-level timing, the rig of the
//! payload rod, and the shipped levels being flyable at all.

use std::path::PathBuf;
use std::sync::Arc;

use luola::headless::SimRunner;
use luola::math::V2;
use luola::sim::body::Body;
use luola::sim::inputs::{BTN_DUMP, BTN_FIRE, InputFrame, Scheme};
use luola::sim::level::Level;
use luola::sim::replay::Replay;
use luola::sim::script::Script;
use luola::sim::ship::Shield;
use luola::sim::tuning;
use luola::sim::validate;
use luola::sim::world::{RunState, World};

/// A sealed box cave, 480x260 playfield, with a floor, ceiling, side walls and a
/// hanging slab. Small enough to reason about, complete enough to fly in.
const FIXTURE: &str = r#"
name = "Fixture"
gravity = 90.0
fuel = 100.0

[player]
pos = [240.0, 90.0]

[[wall]]
points = [[0, 0], [480, 0], [480, 30], [0, 30]]

[[wall]]
points = [[0, 230], [480, 230], [480, 260], [0, 260]]

[[wall]]
points = [[0, 0], [30, 0], [30, 260], [0, 260]]

[[wall]]
points = [[450, 0], [480, 0], [480, 260], [450, 260]]

[[wall]]
points = [[300, 30], [340, 30], [340, 150], [300, 150]]

[[exit]]
pos = [60.0, 200.0]
size = [40.0, 40.0]

[[fuel_pod]]
pos = [200.0, 200.0]
amount = 40.0

[[pad]]
pos = [400.0, 225.0]
size = [60.0, 10.0]
"#;

/// The same cave flown as a payload mission, with the pod parked in the open.
const PAYLOAD_FIXTURE: &str = r#"
name = "Payload Fixture"
gravity = 90.0
fuel = 100.0
require_pod = true

[player]
pos = [240.0, 150.0]
angle = 3.1415927

[[wall]]
points = [[0, 0], [480, 0], [480, 30], [0, 30]]

[[wall]]
points = [[0, 300], [480, 300], [480, 330], [0, 330]]

[[wall]]
points = [[0, 0], [30, 0], [30, 330], [0, 330]]

[[wall]]
points = [[450, 0], [480, 0], [480, 330], [450, 330]]

[[exit]]
pos = [70.0, 150.0]
size = [48.0, 48.0]

[[pod]]
pos = [262.0, 168.0]
"#;

fn fixture() -> Arc<Level> {
    Arc::new(Level::parse(FIXTURE).expect("fixture parses"))
}

fn campaign_paths() -> Vec<PathBuf> {
    let dir = std::env::var("LUOLA_LEVELS").unwrap_or_else(|_| "levels".into());
    ["01_first_descent", "02_payload", "03_reactor_run"]
        .iter()
        .map(|n| PathBuf::from(&dir).join(format!("{n}.toml")))
        .collect()
}

/// Same inputs, same ticks, same state — the whole point of the fixed timestep.
#[test]
fn simulation_is_bit_exact() {
    let level = fixture();
    let mut a = SimRunner::new(level.clone(), Scheme::Classic, 7);
    let mut b = SimRunner::new(level, Scheme::Classic, 7);
    let mut checksums = Vec::new();
    for _ in 0..600 {
        let frame = Script::Dervish.frame(a.world.tick);
        a.push(frame);
        b.push(frame);
        checksums.push(a.world.checksum());
    }
    assert_eq!(
        a.world.checksum(),
        b.world.checksum(),
        "two runs of the same input log diverged"
    );
    assert_eq!(a.world.tick, b.world.tick, "tick counters disagree");
    // The state must actually change, otherwise equality proves nothing.
    assert!(
        checksums.windows(2).any(|w| w[0] != w[1]),
        "state never changed during the run"
    );
}

/// Recording, serialising, parsing and replaying must reproduce the run exactly.
#[test]
fn replay_round_trip_reproduces_the_run() {
    let level = fixture();
    let mut runner = SimRunner::new(level.clone(), Scheme::Modern, 4242);
    runner.run_script(Script::Wobble, 480);
    runner.finish_replay();

    let text = runner.replay.to_text();
    let parsed = Replay::parse(&text).expect("replay parses");
    assert_eq!(parsed.frames.len(), 480);
    assert_eq!(parsed.scheme, Scheme::Modern);
    assert_eq!(parsed.seed, 4242);

    let report = luola::headless::verify_replay(level, &parsed, None);
    assert_eq!(
        Some(report.checksum),
        parsed.checksum,
        "replayed run does not match the recorded checksum"
    );
}

/// `f32` payloads must survive the text format exactly, including subnormals and
/// signed zero, because a lossy frame would break replay verification.
#[test]
fn replay_frames_round_trip_exactly() {
    let mut replay = Replay::new(None, "x", Scheme::Modern, 1);
    for f in [
        InputFrame {
            buttons: 0b10101,
            move_dir: V2::new(-0.0, 1.0e-30),
            aim: -std::f32::consts::PI,
        },
        InputFrame {
            buttons: 0,
            move_dir: V2::new(0.5, f32::MIN_POSITIVE),
            aim: f32::MAX,
        },
    ] {
        replay.push(f);
    }
    let parsed = Replay::parse(&replay.to_text()).expect("parses");
    assert_eq!(parsed.frames, replay.frames);
}

/// A wall must be hit even when the ship crosses it inside a single tick.
#[test]
fn swept_collision_catches_fast_crossings() {
    let level = fixture();
    let terrain = &level.terrain;
    // Straight through the hanging slab (x 300..340, y 30..150).
    let hit = terrain
        .segment_hit(V2::new(280.0, 100.0), V2::new(360.0, 100.0))
        .expect("slab must be hit");
    assert!((hit.p.x - 300.0).abs() < 1.0, "hit at {:?}", hit.p);
    assert!(
        hit.normal.x < 0.0,
        "normal must oppose travel: {:?}",
        hit.normal
    );

    // Through the floor.
    let hit = terrain
        .segment_hit(V2::new(240.0, 200.0), V2::new(240.0, 400.0))
        .expect("floor must be hit");
    assert!(hit.p.y <= 231.0 && hit.p.y >= 229.0, "hit at {:?}", hit.p);

    // Free space stays free.
    assert!(
        terrain
            .segment_hit(V2::new(60.0, 60.0), V2::new(240.0, 200.0))
            .is_none(),
        "empty chamber reported a collision"
    );
}

/// The renderer blits the baked span cache while the physics samples the grid;
/// if the two disagree the player dies to an invisible wall (or flies through
/// one).
#[test]
fn baked_spans_agree_with_collision() {
    let level = fixture();
    let spans = level.terrain.spans();
    let bounds = level.terrain.bounds;
    let mut checked = 0;
    let mut x = bounds.x as i32 + 1;
    while (x as f32) < bounds.right() - 1.0 {
        let solid_from_spans: Vec<(i32, i32)> = spans.column(x).to_vec();
        let mut y = bounds.y as i32 + 1;
        while (y as f32) < bounds.bottom() - 1.0 {
            let p = V2::new(x as f32 + 0.5, y as f32 + 0.5);
            let in_spans = solid_from_spans.iter().any(|&(y0, y1)| y >= y0 && y <= y1);
            let solid = level.terrain.point_solid(p);
            assert_eq!(
                in_spans, solid,
                "span cache disagrees with point_solid at ({x}, {y})"
            );
            checked += 1;
            y += 7;
        }
        x += 3;
    }
    assert!(checked > 4000, "only checked {checked} samples");
}

/// Gravity Ace's shield timings, which the whole forgiveness design rests on.
#[test]
fn shield_absorbs_one_hit_then_recharges() {
    let mut shield = Shield::Charged;
    assert!(shield.absorb(true), "a charged shield must absorb");
    assert!(matches!(shield, Shield::Absorbing { .. }));

    // Rapid multi-hits inside the 500 ms window are all absorbed.
    assert!(shield.absorb(true));
    for _ in 0..40 {
        shield.advance(tuning::DT, true); // 0.33 s
    }
    assert!(shield.absorb(true), "absorb window closed too early");

    // Past the window the shield is down and lethal hits land.
    for _ in 0..40 {
        shield.advance(tuning::DT, true);
    }
    assert!(matches!(
        shield,
        Shield::Down { .. } | Shield::Recharging { .. }
    ));
    assert!(!shield.absorb(true), "shield should be down by now");

    // It comes back on its own.
    let total = tuning::SHIELD_DOWN + tuning::SHIELD_RECHARGE;
    for _ in 0..((total / tuning::DT) as i32 + 2) {
        shield.advance(tuning::DT, true);
    }
    assert_eq!(shield, Shield::Charged, "shield never recharged");
    assert!(shield.absorb(true));
}

/// A dry tank means no shield at all: the second hit is lethal.
#[test]
fn shield_is_dead_without_fuel() {
    let mut shield = Shield::Charged;
    assert!(!shield.absorb(false), "dry tank must not absorb");
    shield.repair();
    shield.advance(tuning::DT, false);
    assert!(!shield.absorb(false));
    // Refuelling brings it back.
    for _ in 0..((tuning::SHIELD_DOWN + tuning::SHIELD_RECHARGE) / tuning::DT) as i32 + 2 {
        shield.advance(tuning::DT, true);
    }
    assert!(shield.absorb(true), "shield must recover once refuelled");
}

/// Flying into the floor destroys the ship and ends the run.
#[test]
fn terrain_contact_ends_the_run() {
    let level = fixture();
    let mut world = World::new(level, Scheme::Classic, 1);
    // Aim the hull straight down and let gravity do the rest, with the shield
    // already spent so the first contact is lethal.
    world.ship.body.angle = std::f32::consts::FRAC_PI_2;
    world.ship.shield = Shield::Down { t: 0.0 };
    for _ in 0..600 {
        world.step(InputFrame::default());
        if world.state.is_over() {
            break;
        }
    }
    assert!(
        !world.ship.alive,
        "ship survived a fall onto the floor: {:?}",
        world.ship.body.p
    );
    assert!(matches!(world.state, RunState::ShipLost { .. }));
}

/// The rod is rigid: the payload stays at the capture distance while flying.
#[test]
fn payload_rod_holds_its_length() {
    let level = fixture();
    let mut world = World::new(level, Scheme::Classic, 3);
    world.pods.push(luola::sim::entities::Pod::new(
        world.ship.body.p + V2::new(0.0, 40.0),
        0.0,
    ));
    let pod_index = world.pods.len() - 1;
    let mut frame = InputFrame::default();
    frame.set(luola::sim::inputs::BTN_BEAM, true);
    world.step(frame);
    assert_eq!(
        world.ship.attached,
        Some(pod_index),
        "beam did not capture a pod inside range"
    );

    let rod = world.pods[pod_index].rod_len;
    let mut max_error: f32 = 0.0;
    for i in 0..900u64 {
        let mut f = InputFrame::default();
        f.set(luola::sim::inputs::BTN_BEAM, true);
        f.set(luola::sim::inputs::BTN_THRUST, i % 120 < 60);
        f.set(luola::sim::inputs::BTN_ROTATE_CCW, i % 240 < 120);
        f.set(luola::sim::inputs::BTN_ROTATE_CW, i % 240 >= 120);
        world.step(f);
        if !world.ship.alive || !world.pods[pod_index].alive {
            break;
        }
        if world.ship.attached == Some(pod_index) {
            let d = world.pods[pod_index].body.p.dist(world.ship.body.p);
            max_error = max_error.max((d - rod).abs());
        }
    }
    assert!(
        max_error < 12.0,
        "rod stretched {max_error:.1} px from {rod:.1} px"
    );
}

/// The payload is a body under gravity. Left alone it must settle on the rock
/// (a level may start it on a ledge, and a player must be able to park it while
/// scouting); on the beam, rock contact is fatal.
#[test]
fn payload_settles_on_rock_but_dies_on_the_beam() {
    let level = Arc::new(Level::parse(PAYLOAD_FIXTURE).expect("parses"));
    let mut world = World::new(level, Scheme::Classic, 12);
    // Drop it from high up: it lands, it does not shatter.
    world.pods[0].body.p = V2::new(240.0, 60.0);
    world.pods[0].body.v = V2::ZERO;
    for _ in 0..900 {
        let mut f = InputFrame::default();
        f.set(luola::sim::inputs::BTN_ROTATE_CCW, true);
        world.step(f);
        if !matches!(world.state, RunState::Flying) {
            break;
        }
    }
    assert!(
        world.pods[0].alive,
        "a free payload was destroyed by a landing at {:?}",
        world.pods[0].body.p
    );
    assert!(
        !world.level.terrain.point_solid(world.pods[0].body.p),
        "the settled payload is buried in rock"
    );

    // Towed into a wall at speed, it is gone and the mission is failed.
    let level = Arc::new(Level::parse(PAYLOAD_FIXTURE).expect("parses"));
    let mut world = World::new(level, Scheme::Classic, 12);
    let mut beam = InputFrame::default();
    beam.set(luola::sim::inputs::BTN_BEAM, true);
    // Put the payload *ahead* of the ship, on the far side from the exit, so it
    // is the pod and not the ship that meets the rock.
    world.pods[0].body.p = V2::new(290.0, 150.0);
    world.pods[0].prev = world.pods[0].body;
    world.step(beam);
    assert_eq!(world.ship.attached, Some(0));
    world.ship.body.v = V2::new(500.0, 0.0);
    for _ in 0..120 {
        world.step(beam);
        if !matches!(world.state, RunState::Flying) {
            break;
        }
    }
    assert!(
        !world.pods[0].alive,
        "the towed payload survived a wall at 500 px/s"
    );
    assert!(matches!(
        world.state,
        RunState::Failed {
            reason: luola::sim::events::Failure::PayloadLost,
            ..
        }
    ));
}

/// Towing must be possible and must hurt. Hang the payload under the ship, then
/// climb: the payload has to come up with it, and the pair must climb clearly
/// slower than a solo ship — that penalty is the whole point of the mission.
#[test]
fn towing_the_payload_costs_climb_rate() {
    let run = |with_pod: bool| -> (f32, f32) {
        let level = Arc::new(Level::parse(PAYLOAD_FIXTURE).expect("parses"));
        let mut world = World::new(level, Scheme::Classic, 4);
        let mut held = InputFrame::default();
        held.set(luola::sim::inputs::BTN_THRUST, true);
        // The beam key must stay *off* for the solo run, or the pilot picks the
        // payload up and the baseline stops being a baseline.
        held.set(luola::sim::inputs::BTN_BEAM, with_pod);
        if with_pod {
            world.step(held);
            assert_eq!(world.ship.attached, Some(0), "beam did not capture");
            // Let the rod settle straight below with the engine off.
            let mut coast = InputFrame::default();
            coast.set(luola::sim::inputs::BTN_BEAM, true);
            world.ship.body.angle = -std::f32::consts::FRAC_PI_2;
            for _ in 0..60 {
                world.step(coast);
            }
        }
        world.ship.body.angle = -std::f32::consts::FRAC_PI_2; // nose up
        world.ship.body.v = V2::ZERO;
        if let Some(pod) = world.pods.first_mut() {
            pod.body.v = V2::ZERO;
        }
        // Short window: the cave ceiling is close, and a bounce would make the
        // measurement meaningless.
        for _ in 0..90 {
            world.step(held);
        }
        let pod_climb = world.pods.first().map(|p| -p.body.v.y).unwrap_or(f32::NAN);
        (-world.ship.body.v.y, pod_climb)
    };

    let (solo, _) = run(false);
    let (towing, pod_climb) = run(true);
    assert!(solo > 50.0, "a solo ship barely climbs: {solo:.1} px/s");
    assert!(
        pod_climb > 20.0,
        "the towed payload is not being lifted ({pod_climb:.1} px/s): payload levels would be unwinnable"
    );
    assert!(
        towing > solo * 0.25,
        "the ship can barely climb while towing ({towing:.1} of {solo:.1} px/s): payload levels would be unwinnable"
    );
    assert!(
        towing < solo * 0.5,
        "towing is too cheap ({towing:.1} of {solo:.1} px/s): the return leg stops being the puzzle"
    );
}

/// Fuel is the budget: thrusting spends it and a dry tank produces no thrust.
#[test]
fn thrusting_burns_fuel_and_a_dry_tank_gives_no_thrust() {
    let level = fixture();
    let gravity = level.gravity;
    let mut world = World::new(level, Scheme::Classic, 5);
    let start = world.ship.fuel;
    let mut frame = InputFrame::default();
    frame.set(luola::sim::inputs::BTN_THRUST, true);
    for _ in 0..120 {
        world.step(frame);
    }
    let burned = start - world.ship.fuel;
    assert!(
        (burned - tuning::FUEL_BURN).abs() < 0.5,
        "one second of thrust burned {burned:.2} fuel, expected ~{:.2}",
        tuning::FUEL_BURN
    );

    // Same tick, dry tank: the only acceleration left is gravity.
    world.ship.fuel = 0.0;
    let before = world.ship.body.v;
    world.step(frame);
    let expected = before + V2::new(0.0, gravity) * tuning::DT;
    assert!(
        (world.ship.body.v - expected).len() < 0.5,
        "engines produced thrust on an empty tank: {:?} vs {:?}",
        world.ship.body.v,
        expected
    );

    // And the ship-level contract: burn drains to exactly zero and stops.
    let mut ship = luola::sim::ship::Ship::new(V2::ZERO, 0.0, 12.0);
    assert!((ship.thrust_available(gravity) - gravity * tuning::THRUST_RATIO).abs() < 1e-3);
    for _ in 0..600 {
        ship.burn(tuning::DT);
    }
    assert_eq!(ship.fuel, 0.0, "a dry tank must not go negative");
    assert_eq!(ship.thrust_available(gravity), 0.0);
}

/// Levels are validated rather than trusted: an objective walled off from the
/// spawn must be reported, and a cave cannot leak however little it authored,
/// because the grid carries its own rock casing.
#[test]
fn level_validation_rejects_broken_caves() {
    // The fixture with its ceiling brush removed is still sealed: the casing
    // closes the grid, so there is no way to author a hole out of the level.
    let open = FIXTURE.replace(
        "[[wall]]\npoints = [[0, 0], [480, 0], [480, 30], [0, 30]]",
        "",
    );
    let level = Level::parse(&open).expect("parses without a ceiling");
    let reach = validate::flood(&level);
    assert!(
        reach.sealed,
        "a cave with no ceiling brush must still be sealed by the casing"
    );

    // A bar across the cave below the spawn: the exit, the pad and the pod are all
    // walled off from it, and the bar must not bury any of them on its way past.
    let sealed_off = FIXTURE.replace(
        "points = [[0, 230], [480, 230], [480, 260], [0, 260]]",
        "points = [[0, 230], [480, 230], [480, 260], [0, 260]]\n\n[[wall]]\npoints = [[30, 100], [450, 100], [450, 108], [30, 108]]",
    );
    let level = Level::parse(&sealed_off).expect("parses");
    let reach = validate::flood(&level);
    assert!(
        !reach.unreachable.is_empty(),
        "objective behind a wall was reported reachable"
    );
}

/// A cave can hold air the ship cannot get to, and the validator has to say
/// where it is: a pocket trapped under the flank of a mass is invisible on the
/// map until you know its position. This also drives the organic brush kinds
/// through the real loader.
#[test]
fn sealed_pockets_are_reported_with_their_position() {
    let level = Level::parse(
        r#"
name = "Pockets"
[player]
pos = [290, 100]

# the cave: a closed lumpy loop, with the same loop swept at its full radius so
# the wall meets the casing and does not trap air of its own
[[wall]]
kind = "blob"
points = [[40, 40], [360, 40], [360, 300], [40, 300], [40, 40]]
radius = 40
lumps = 24

[[wall]]
kind = "chain"
points = [[40, 40], [360, 40], [360, 300], [40, 300], [40, 40]]
radius = 40

# a small ring inside it: the air it wraps is sealed off
[[wall]]
kind = "chain"
points = [[180, 120], [222, 162], [222, 222], [180, 264], [138, 222], [138, 162], [180, 120]]
radius = 20

[[exit]]
pos = [280, 250]
size = [40, 40]
"#,
    )
    .expect("parses");

    let reach = validate::flood(&level);
    assert!(reach.sealed);
    assert!(reach.unreachable.is_empty(), "{:?}", reach.unreachable);
    let pocket = reach
        .pockets
        .iter()
        .find(|p| {
            p.rect.x >= 118.0
                && p.rect.right() <= 242.0
                && p.rect.y >= 100.0
                && p.rect.bottom() <= 284.0
        })
        .unwrap_or_else(|| panic!("no pocket reported inside the ring: {:?}", reach.pockets));
    assert!(pocket.cells > 0);
}

#[test]
fn malformed_levels_are_rejected_with_reasons() {
    let cases = [
        ("", "parse error"),
        ("name = \"x\"\n[player]\npos = [10,10]\n", "no `[[wall]]`"),
        (
            "name = \"x\"\n[player]\npos = [10,10]\n[[wall]]\npoints = [[0,0],[10,0]]\n",
            "at least 3",
        ),
        (
            "name = \"x\"\n[player]\npos = [10,10]\n[[wall]]\npoints = [[0,0],[10,0],[0,10]]\n",
            "no `[[exit]]`",
        ),
        (
            "name = \"x\"\nrequire_pod = true\n[player]\npos = [10,10]\n[[wall]]\npoints = [[0,0],[100,0],[100,100],[0,100]]\n[[exit]]\npos = [50,50]\n",
            "no `[[pod]]`",
        ),
        (
            "name = \"x\"\nexit_locked = true\n[player]\npos = [10,10]\n[[wall]]\npoints = [[0,0],[100,0],[100,100],[0,100]]\n[[exit]]\npos = [50,50]\n",
            "no `[[reactor]]`",
        ),
        (
            "name = \"x\"\n[player]\npos = [10,10]\n[[wall]]\npoints = [[0,0],[100,0],[100,100],[0,100]]\n[[exit]]\npos = [50,50]\n",
            "inside solid terrain",
        ),
        // A kind that ignores a field is a brush that does nothing.
        (
            "name = \"x\"\n[player]\npos = [10,10]\n[[wall]]\nkind = \"disc\"\npoints = [[0,0],[10,0],[0,10]]\nradius = 20\n[[exit]]\npos = [50,50]\n",
            "takes no `points`",
        ),
        (
            "name = \"x\"\n[player]\npos = [10,10]\n[[wall]]\nkind = \"poly\"\npoints = [[0,0],[100,0],[100,100],[0,100]]\nradius = 20\n[[exit]]\npos = [50,50]\n",
            "takes no `radius`",
        ),
        (
            "name = \"x\"\n[player]\npos = [10,10]\n[[wall]]\nkind = \"chain\"\npoints = [[0,0],[100,0]]\nlumps = 8\n[[exit]]\npos = [50,50]\n",
            "takes no `lumps`",
        ),
        (
            "name = \"x\"\n[player]\npos = [10,10]\n[[wall]]\nkind = \"spline\"\npoints = [[0,0],[100,0]]\nradius = 20\n[[exit]]\npos = [50,50]\n",
            // The message must name the offending kind; which alternatives it
            // lists is presentation, and pinning that here would make every new
            // brush shape a test failure.
            "kind `spline`",
        ),
        // Furniture is drawn over by organic masses more easily than by boxes,
        // so a buried turret has to be a load error rather than a surprise.
        (
            "name = \"x\"\n[player]\npos = [100,50]\n[[wall]]\npoints = [[0,100],[200,100],[200,120],[0,120]]\n[[exit]]\npos = [100,60]\n[[turret]]\npos = [100,110]\n",
            "turret at (100, 110) is inside solid terrain",
        ),
    ];
    for (text, needle) in cases {
        let err = Level::parse(text).expect_err("must be rejected");
        assert!(
            err.to_string().contains(needle),
            "error {err:?} does not mention {needle:?}"
        );
    }
}

/// The shipped campaign has to be flyable: sealed caves, reachable objectives.
#[test]
fn campaign_levels_are_sealed_and_reachable() {
    for path in campaign_paths() {
        let level = Level::load(&path)
            .unwrap_or_else(|e| panic!("campaign level {} does not load: {e}", path.display()));
        let reach = validate::flood(&level);
        assert!(
            !reach.spawn_buried,
            "{}: player spawns inside rock",
            level.name
        );
        assert!(reach.sealed, "{}: cave is not sealed", level.name);
        assert!(
            reach.unreachable.is_empty(),
            "{}: unreachable {}",
            level.name,
            reach.unreachable.join(", ")
        );
        assert!(
            reach.reachable_fraction() > 0.9,
            "{}: only {:.0}% of open space is reachable from the spawn",
            level.name,
            reach.reachable_fraction() * 100.0
        );
        assert!(!level.exits.is_empty(), "{}: no exit", level.name);
    }
}

/// The genre's classic trap is winning the objective and then being unable to
/// leave (`docs/game_mechanics.md` §8.3). Every shipped level must be flyable
/// within its own fuel budget, and the exit must be reachable on the tank alone.
#[test]
fn campaign_levels_fit_their_fuel_budget() {
    for path in campaign_paths() {
        let level = Level::load(&path).expect("loads");
        let reach = validate::flood(&level);
        let route = reach.route_px.expect("an exit route exists");
        let exit_fuel = validate::fuel_for_distance(route);
        assert!(
            exit_fuel <= level.start_fuel,
            "{}: reaching the exit costs {exit_fuel:.0} fuel but the tank holds {:.0}",
            level.name,
            level.start_fuel
        );
        let plan = reach.mission_distance().expect("a mission route exists");
        let plan_fuel = validate::fuel_for_distance(plan);
        let budget = validate::fuel_budget(&level);
        assert!(
            plan_fuel <= budget,
            "{}: the mission plan needs {plan_fuel:.0} fuel, the level offers {budget:.0}",
            level.name
        );
    }
}

/// The rotate keys must turn the ship the way the player sees it: screen space
/// has +y down, so "counter-clockwise" means the nose goes up when flying right.
#[test]
fn rotation_keys_turn_the_way_they_read() {
    let level = fixture();
    let mut world = World::new(level, Scheme::Classic, 11);

    let mut ccw = InputFrame::default();
    ccw.set(luola::sim::inputs::BTN_ROTATE_CCW, true);
    for _ in 0..(tuning::TICK_HZ / 2) {
        world.step(ccw);
    }
    let forward = world.ship.body.forward();
    assert!(
        forward.y < -0.3,
        "holding rotate-CCW from level flight did not lift the nose: {forward:?}"
    );

    // Fresh run: the second half must start from level flight again.
    let mut world = World::new(fixture(), Scheme::Classic, 11);
    let mut cw = InputFrame::default();
    cw.set(luola::sim::inputs::BTN_ROTATE_CW, true);
    for _ in 0..(tuning::TICK_HZ / 2) {
        world.step(cw);
    }
    let forward = world.ship.body.forward();
    assert!(
        forward.y > 0.3,
        "holding rotate-CW did not push the nose down: {forward:?}"
    );
}

/// A scripted pilot, good enough to fly the fixtures.
///
/// Classic-mode attitude planning in one rule: the hull is the only thrust
/// vector, so the engine must never be aimed so far sideways that the vertical
/// component of its `THRUST_RATIO * g` can no longer carry the ship. The
/// horizontal demand is therefore clamped to what is left over, and the engine
/// is pulsed — full burn everywhere would overshoot every approach.
fn fly_towards(world: &mut World, target: V2, ticks: u64, beam: bool) -> bool {
    let gravity = world.level.gravity;
    let thrust = gravity * tuning::THRUST_RATIO;
    // Largest horizontal acceleration that still leaves enough lift.
    let max_sideways = (thrust * thrust - gravity * gravity).sqrt() * 0.95;
    for _ in 0..ticks {
        let p = world.ship.body.p;
        let v = world.ship.body.v;
        let to = target - p;
        let dist = to.len();
        let speed_cap = (dist * 0.9).clamp(35.0, 110.0);
        let desired_v = to.normalized() * speed_cap;
        let closing = v.dot(to.normalized());

        let want = if dist < 150.0 && closing > 60.0 {
            // Flip and burn: a cave flyer brakes by pointing the other way.
            -v.normalized() * thrust + V2::new(0.0, -gravity)
        } else {
            let mut demand = (desired_v - v) * 1.8;
            demand.x = demand.x.clamp(-max_sideways, max_sideways);
            demand.y = demand.y.clamp(-max_sideways, max_sideways);
            demand + V2::new(0.0, -gravity)
        };
        let err = luola::math::angle_diff(world.ship.body.angle, want.angle());

        let mut f = InputFrame::default();
        // Screen space is +y down: pressing "clockwise" increases the angle. The
        // deadband keeps the full-rate rotation from oscillating forever.
        f.set(luola::sim::inputs::BTN_ROTATE_CW, err > 0.12);
        f.set(luola::sim::inputs::BTN_ROTATE_CCW, err < -0.12);
        f.set(
            luola::sim::inputs::BTN_THRUST,
            err.abs() < 0.8 && want.len() > gravity * 0.9,
        );
        f.set(luola::sim::inputs::BTN_BEAM, beam);
        world.step(f);
        if !matches!(world.state, RunState::Flying) {
            return !matches!(
                world.state,
                RunState::ShipLost { .. } | RunState::Failed { .. }
            );
        }
    }
    false
}

/// The whole mission loop: fly to the exit through the cave, and be scored for
/// it. This is the one test that exercises physics, collision, mission logic,
/// events and scoring together.
#[test]
fn reaching_the_exit_completes_the_run() {
    let level = fixture();
    let target = level.exits[0].rect.center();
    let mut world = World::new(level, Scheme::Classic, 99);
    assert!(
        fly_towards(&mut world, target, 2400, false),
        "pilot crashed"
    );
    assert!(
        matches!(world.state, RunState::Complete { .. }),
        "run did not complete at the exit: {:?} at {:?}",
        world.state,
        world.ship.body.p
    );
    assert_eq!(world.score.escape, luola::sim::tuning::SCORE_ESCAPE);
    assert!(world.score.total() >= luola::sim::tuning::SCORE_ESCAPE);
    assert!(
        world.ship.fuel > 0.0,
        "the run should not have stranded the tank"
    );
}

/// The signature mission: capture the payload on the beam, carry it to the exit
/// and get paid for it.
#[test]
fn payload_mission_can_be_completed() {
    let level = Arc::new(Level::parse(PAYLOAD_FIXTURE).expect("fixture parses"));
    let exit = level.exits[0].rect.center();
    let mut world = World::new(level, Scheme::Classic, 5);

    // The payload starts on the beam's doorstep: open the beam and it is ours.
    let mut beam = InputFrame::default();
    beam.set(luola::sim::inputs::BTN_BEAM, true);
    let gap = world.pods[0].body.p.dist(world.ship.body.p);
    world.step(beam);
    assert_eq!(
        world.ship.attached,
        Some(0),
        "the beam did not capture a payload {gap:.0} px away"
    );

    // Stage clear of the exit and come in from the right: the rod trails ~37 px
    // behind the ship, and approaching from the left drags the payload along the
    // cave wall. That is a flying mistake, not a physics one.
    let stage = exit + V2::new(150.0, 70.0);
    fly_towards(&mut world, stage, 1200, true);
    fly_towards(&mut world, exit, 1200, true);
    assert!(
        world.level.bounds.contains(world.ship.body.p),
        "the ship left the cave at {:?}",
        world.ship.body.p
    );
    assert!(
        matches!(world.state, RunState::Complete { .. }),
        "payload run did not complete: {:?} at {:?}",
        world.state,
        world.ship.body.p
    );
    assert!(
        world.payload_secured(),
        "the escape did not count the payload as secured"
    );
    assert_eq!(world.score.payload, luola::sim::tuning::SCORE_POD);
}

/// Landing pads are the level's repair stations: slow contact refuels the tank
/// and brings the shield back.
#[test]
fn landing_pad_repairs_and_refuels() {
    let level = fixture();
    let pad = level.pads[0].rect.center();
    let mut world = World::new(level, Scheme::Classic, 2);
    world.ship.body.p = pad;
    world.ship.body.v = V2::ZERO;
    world.ship.body.angle = std::f32::consts::FRAC_PI_2;
    world.ship.fuel = 5.0;
    world.ship.shield = Shield::Down { t: 0.0 };
    world.step(InputFrame::default());
    assert_eq!(
        world.ship.fuel, world.ship.fuel_capacity,
        "pad did not refuel"
    );
    assert_eq!(world.ship.shield, Shield::Charged, "pad did not repair");
}

/// A body that has penetrated the wall mass must be pushed back out. Swept
/// tests alone cannot see it, because there is no edge to cross from inside, and
/// the failure mode is a ship flying out of the level through solid rock.
#[test]
fn penetrating_the_wall_does_not_tunnel_out() {
    let level = fixture();
    let mut world = World::new(level, Scheme::Classic, 8);
    // Bury the hull just inside the left wall (x 0..30), still pushing left, and
    // keep the engine pointed at the rock for two seconds: the ship must bounce
    // out of the near face every time, never appear on the far side.
    world.ship.body.p = V2::new(28.0, 120.0);
    world.ship.body.angle = std::f32::consts::PI;
    world.ship.body.v = V2::new(-400.0, 0.0);
    let mut frame = InputFrame::default();
    frame.set(luola::sim::inputs::BTN_THRUST, true);

    let mut buried_ticks = 0;
    for _ in 0..240 {
        world.step(frame);
        if world.state.is_over() {
            break;
        }
        assert!(
            world.ship.body.p.x > 0.0,
            "ship tunnelled out through the far face to {:?}",
            world.ship.body.p
        );
        if world.level.terrain.point_solid(world.ship.body.p) {
            buried_ticks += 1;
        }
    }
    assert!(
        buried_ticks < 30,
        "ship stayed buried in the wall for {buried_ticks} ticks"
    );
}

/// A body under gravity and no thrust behaves exactly like the genre says:
/// velocity accumulates, nothing clamps it.
#[test]
fn no_drag_and_no_speed_clamp() {
    let mut body = Body::new(V2::ZERO, 0.0);
    let g = 90.0;
    for _ in 0..120 {
        body.integrate(V2::new(0.0, g), tuning::DT);
    }
    assert!(
        (body.v.y - g).abs() < 0.01,
        "one second of free fall gave v={}",
        body.v.y
    );
    for _ in 0..600 {
        body.integrate(V2::ZERO, tuning::DT);
    }
    assert!(
        (body.v.y - g).abs() < 0.01,
        "velocity drifted without any force applied"
    );
}

#[test]
fn debug_pilot_trace() {
    let level = Arc::new(Level::parse(PAYLOAD_FIXTURE).expect("parses"));
    let exit = level.exits[0].rect.center();
    let mut world = World::new(level, Scheme::Classic, 5);
    let mut beam = InputFrame::default();
    beam.set(luola::sim::inputs::BTN_BEAM, true);
    world.step(beam);
    for i in 0..600u64 {
        fly_towards(&mut world, exit, 1, true);
        if i % 5 == 0 || !matches!(world.state, RunState::Flying) {
            println!(
                "t={i} ship ({:.0},{:.0}) v ({:.0},{:.0}) ang {:.2} pod ({:.0},{:.0}) alive={} {:?}",
                world.ship.body.p.x,
                world.ship.body.p.y,
                world.ship.body.v.x,
                world.ship.body.v.y,
                world.ship.body.angle,
                world.pods[0].body.p.x,
                world.pods[0].body.p.y,
                world.pods[0].alive,
                world.state
            );
        }
        if !matches!(world.state, RunState::Flying) {
            break;
        }
    }
}

/// The same round trip on a real campaign level, with the classic scheme: the
/// log must survive the text format frame for frame and reproduce the run.
#[test]
fn campaign_replay_round_trips_frame_for_frame() {
    let level = Arc::new(Level::load(&campaign_paths()[2]).expect("campaign level 3 loads"));
    let mut runner = SimRunner::new(level.clone(), Scheme::Classic, 0x10aa_1e17);
    runner.run_script(Script::Dervish, 2000);
    runner.finish_replay();

    let parsed = Replay::parse(&runner.replay.to_text()).expect("replay parses");
    assert_eq!(parsed.frames.len(), 2000);
    assert_eq!(
        parsed.frames, runner.replay.frames,
        "frames changed in the file"
    );

    let report = luola::headless::verify_replay(level, &parsed, None);
    assert_eq!(
        Some(report.checksum),
        parsed.checksum,
        "the recorded run did not reproduce on a campaign level"
    );
    assert_eq!(
        report.ticks, 2000,
        "the whole log must be replayed by default"
    );
}

// ---------------------------------------------------------------------------
// Digging and water.
// ---------------------------------------------------------------------------

/// A sealed chamber with a thick dirt floor 60 px under the spawn, and the ship
/// already pointing down it.
const DIG_FIXTURE: &str = r#"
name = "Dig"
gravity = 90.0
fuel = 100.0

[player]
pos = [240.0, 110.0]
angle = 1.5707963

[[wall]]
points = [[0, 0], [480, 0], [480, 30], [0, 30]]

[[wall]]
points = [[0, 170], [480, 170], [480, 240], [0, 240]]

[[wall]]
points = [[0, 0], [30, 0], [30, 240], [0, 240]]

[[wall]]
points = [[450, 0], [480, 0], [480, 240], [450, 240]]

[[exit]]
pos = [60.0, 60.0]
size = [40.0, 40.0]
"#;

/// The same chamber, with the floor authored as stone.
const ROCK_FIXTURE: &str = r#"
name = "Rock"
gravity = 90.0
fuel = 100.0

[player]
pos = [240.0, 110.0]
angle = 1.5707963

[[wall]]
points = [[0, 0], [480, 0], [480, 30], [0, 30]]

[[wall]]
material = "rock"
points = [[0, 170], [480, 170], [480, 240], [0, 240]]

[[wall]]
points = [[0, 0], [30, 0], [30, 240], [0, 240]]

[[wall]]
points = [[450, 0], [480, 0], [480, 240], [450, 240]]

[[exit]]
pos = [60.0, 60.0]
size = [40.0, 40.0]
"#;

/// A pool resting on a thin dirt floor, with an empty hall below it.
const POOL_FIXTURE: &str = r#"
name = "Pool"
gravity = 90.0
fuel = 100.0

[player]
pos = [240.0, 60.0]
angle = 1.5707963

[[wall]]
points = [[0, 0], [480, 0], [480, 30], [0, 30]]

[[wall]]
points = [[0, 140], [480, 140], [480, 146], [0, 146]]

[[wall]]
points = [[0, 220], [480, 220], [480, 240], [0, 240]]

[[wall]]
points = [[0, 0], [30, 0], [30, 240], [0, 240]]

[[wall]]
points = [[450, 0], [480, 0], [480, 240], [450, 240]]

[[liquid]]
points = [[30.0, 100.0], [450.0, 100.0], [450.0, 140.0], [30.0, 140.0]]

[[exit]]
pos = [60.0, 200.0]
size = [40.0, 40.0]
"#;

fn water_in_rows(world: &World, y0: i32, y1: i32) -> u64 {
    let mut total = 0u64;
    for y in y0..=y1 {
        for x in 0..480 {
            total += world.water.mass(x, y) as u64;
        }
    }
    total
}

/// Shooting the ground destroys it: this is the whole point of the grid.
#[test]
fn bullets_dig_craters_in_dirt() {
    let level = Arc::new(Level::parse(DIG_FIXTURE).expect("dig fixture parses"));
    let mut runner = SimRunner::new(level, Scheme::Classic, 11);
    let before = runner.world.terrain.solid_cells();

    for _ in 0..80 {
        runner.push(InputFrame::default().with(BTN_FIRE, true));
    }

    let after = runner.world.terrain.solid_cells();
    assert!(after < before, "no dirt was dug: {before} -> {after}");
    assert!(
        !runner.world.terrain.solid(240, 171),
        "the floor should have a hole where the shots landed"
    );
    assert_eq!(
        runner.world.terrain.cell(240, 235),
        luola::sim::terrain::MAT_DIRT,
        "the crater must stay local"
    );
}

/// Rock does not dig, however much of it you shoot.
#[test]
fn rock_survives_every_bullet() {
    let level = Arc::new(Level::parse(ROCK_FIXTURE).expect("rock fixture parses"));
    let mut runner = SimRunner::new(level, Scheme::Classic, 11);
    let before = runner.world.terrain.solid_cells();

    for _ in 0..80 {
        runner.push(InputFrame::default().with(BTN_FIRE, true));
    }

    assert_eq!(
        runner.world.terrain.solid_cells(),
        before,
        "stone must be permanent, or a level's layout means nothing"
    );
}

/// Blow the floor out from under a pool and the water pours into the hall below.
#[test]
fn water_pours_through_a_dug_floor() {
    let level = Arc::new(Level::parse(POOL_FIXTURE).expect("pool fixture parses"));
    let mut runner = SimRunner::new(level, Scheme::Classic, 3);

    let pool_before = water_in_rows(&runner.world, 100, 139);
    let below_before = water_in_rows(&runner.world, 146, 219);
    let total_before = runner.world.water.total();
    assert!(pool_before > 0, "the fixture must start with water");
    assert_eq!(below_before, 0, "the hall starts dry");
    for _ in 0..5 {
        runner.push(InputFrame::default());
    }
    assert!(
        runner.world.water.is_settled(),
        "a freshly seeded pool is already level"
    );

    let carve = runner.world.terrain.carve(V2::new(240.0, 143.0), 7.0);
    assert!(carve.hit_terrain(), "the pool floor is dirt");
    runner
        .world
        .water
        .wake(carve.x0, carve.y0, carve.x1, carve.y1);
    assert!(
        !runner.world.water.is_settled(),
        "opening the floor must wake the water"
    );

    // Fly straight; the water needs time to pour through the hole.
    for _ in 0..400 {
        runner.push(InputFrame::default());
    }

    let pool_after = water_in_rows(&runner.world, 100, 139);
    let below_after = water_in_rows(&runner.world, 146, 219);
    assert!(below_after > 0, "the water never reached the hall below");
    assert!(
        pool_after < pool_before,
        "the pool level must drop: {pool_before} -> {pool_after}"
    );
    // Whole-body conservation, not the sum of two windows: some of the water is
    // in the crater itself at this point.
    assert_eq!(
        runner.world.water.total(),
        total_before,
        "water is conserved when it pours"
    );
}

/// Carving and flowing water are part of the checksummed run, so two worlds fed
/// the same inputs must not diverge while the cave comes apart.
#[test]
fn digging_and_flowing_water_stay_bit_exact() {
    let level = Arc::new(Level::parse(POOL_FIXTURE).expect("pool fixture parses"));
    let pristine = level.terrain.solid_cells();
    let mut a = SimRunner::new(level.clone(), Scheme::Classic, 99);
    let mut b = SimRunner::new(level, Scheme::Classic, 99);

    for t in 0..600 {
        let frame = InputFrame::default().with(BTN_FIRE, t < 120);
        a.push(frame);
        b.push(frame);
        assert_eq!(
            a.world.checksum(),
            b.world.checksum(),
            "worlds diverged at tick {t}"
        );
    }

    assert_eq!(a.world.terrain.solid_cells(), b.world.terrain.solid_cells());
    assert_eq!(a.world.water.total(), b.world.water.total());
    assert_eq!(a.world.water.moved(), b.world.water.moved());
    assert!(
        a.world.terrain.solid_cells() < pristine,
        "the run should have dug something out"
    );
}

/// The cave's outer wall is rock however the level is authored: it is the grid's
/// own casing, so there is nothing for an author to remember and nothing for a
/// player to tunnel through. The test walks the grid's real outer ring — in world
/// coordinates, from `bounds` — because a level's brushes no longer start at 0,0.
#[test]
fn campaign_cave_boundaries_are_rock() {
    use luola::sim::terrain::MAT_DIRT;
    for path in campaign_paths() {
        let level = Level::load(&path).expect("campaign level loads");
        let terrain = &level.terrain;
        let b = terrain.bounds;
        let (x0, y0) = (b.x as i32, b.y as i32);
        let (x1, y1) = (b.right() as i32 - 1, b.bottom() as i32 - 1);
        for x in x0..=x1 {
            for y in [y0, y1] {
                assert_ne!(
                    terrain.cell(x, y),
                    MAT_DIRT,
                    "{}: diggable dirt on the cave's outer shell at ({x}, {y})",
                    level.name
                );
            }
        }
        for y in y0..=y1 {
            for x in [x0, x1] {
                assert_ne!(
                    terrain.cell(x, y),
                    MAT_DIRT,
                    "{}: diggable dirt on the cave's outer shell at ({x}, {y})",
                    level.name
                );
            }
        }
    }
}

// --------------------------------------------------------------- granular --

/// A sealed cave whose floor is a granular wall the ship falls onto.
const GRANULAR_FIXTURE: &str = r#"
name = "Granular"
gravity = 90.0
fuel = 100.0

[player]
pos = [240.0, 80.0]

[[wall]]
points = [[0, 0], [480, 0], [480, 30], [0, 30]]

[[wall]]
points = [[0, 0], [30, 0], [30, 300], [0, 300]]

[[wall]]
points = [[450, 0], [480, 0], [480, 300], [450, 300]]

[[wall]]
kind = "poly"
material = "granular"
points = [[0, 220], [480, 220], [480, 300], [0, 300]]

[[exit]]
pos = [60.0, 160.0]
size = [40.0, 40.0]
"#;

/// A granular wall holds the ship instead of killing it, and lets go when the
/// wall around the hull is gone — the AUTS trap.
#[test]
fn a_granular_wall_grabs_the_ship_until_it_is_shot_free() {
    let level = Arc::new(Level::parse(GRANULAR_FIXTURE).expect("granular fixture parses"));
    let mut world = World::new(level, Scheme::Classic, 3);
    // Let gravity drop the ship onto the granular floor.
    for _ in 0..300 {
        world.step(InputFrame::default());
    }
    assert!(world.ship.alive, "a granular wall must not be lethal");
    assert!(world.ship.grabbed, "the wall must hold the ship");
    assert!(
        world.ship.body.v.len() < 40.0,
        "the grab must eat the ship's velocity, got {}",
        world.ship.body.v.len()
    );

    // Shoot the wall away: blow the granular cells around the hull out.
    let p = world.ship.body.p + V2::new(0.0, 12.0);
    world.terrain.carve(p, 40.0);
    world.step(InputFrame::default());
    assert!(
        !world.ship.grabbed,
        "the ship is released when the wall is gone"
    );
}

// ------------------------------------------------------------------- mass --

/// Carried mass changes handling: a full tank and a full magazine fly at the
/// reference thrust, and a lighter ship is quicker up to the cap.
#[test]
fn burning_fuel_makes_the_ship_lighter_and_quicker() {
    let level = fixture();
    let mut world = World::new(level, Scheme::Classic, 5);
    let fresh = world.ship.agility();
    assert!(
        (fresh - 1.0).abs() < 1e-6,
        "a freshly launched ship is the reference mass, got {fresh}"
    );
    world.ship.fuel *= 0.2;
    let light = world.ship.agility();
    assert!(light > fresh, "burning fuel must lighten the ship");
    assert!(light <= tuning::MASS_AGILITY_MAX + 1e-6, "and stay capped");
}

/// The stripped-for-speed choice: dumping the magazine lightens the ship, and a
/// base can hand the ammo back.
#[test]
fn jettisoning_the_magazine_lightens_the_ship() {
    let level = fixture();
    let mut world = World::new(level, Scheme::Classic, 5);
    let before = world.ship.agility();
    assert!(world.ship.loadout.ammo > 0, "the ship launches armed");

    let mut f = InputFrame::default();
    f.set(BTN_DUMP, true);
    world.step(f);
    assert_eq!(world.ship.loadout.ammo, 0, "the magazine is dumped");
    let after = world.ship.agility();
    assert!(after > before, "a dumped magazine must lighten the ship");

    // Re-arming at a base undoes it, so the trade is not permanent.
    world.ship.loadout.rearm();
    assert!((world.ship.agility() - before).abs() < 1e-6);
}

// -------------------------------------------------- movers and signals --

/// A cave with a reactor, a hidden exit and a reactor-powered gate.
const GATE_FIXTURE: &str = r#"
name = "Gate"
gravity = 90.0
fuel = 100.0
exit_locked = true

[player]
pos = [240.0, 150.0]

[[wall]]
points = [[0, 0], [480, 0], [480, 30], [0, 30]]

[[wall]]
points = [[0, 230], [480, 230], [480, 260], [0, 260]]

[[wall]]
points = [[0, 0], [30, 0], [30, 260], [0, 260]]

[[wall]]
points = [[450, 0], [480, 0], [480, 260], [450, 260]]

[[exit]]
pos = [70.0, 180.0]
size = [40.0, 40.0]
hidden = true

[[reactor]]
pos = [300.0, 60.0]

[[gate]]
pos = [200.0, 150.0]
size = [40.0, 30.0]
to = [280.0, 150.0]
speed = 60.0
trigger = "reactor"

[[signal]]
when = "reactor"
action = "reveal_exit"
index = 0

[[signal]]
when = "reactor"
action = "start_gate"
index = 0
"#;

/// The reactor raises the level's signal graph: the authored edges reveal the
/// hidden exit and power the gate, rather than the built-in default.
#[test]
fn the_reactor_drives_the_signal_graph() {
    let level = Arc::new(Level::parse(GATE_FIXTURE).expect("gate fixture parses"));
    let mut world = World::new(level, Scheme::Classic, 1);
    assert!(!world.gates[0].active, "a reactor gate starts inert");
    assert!(world.exits[0].hidden, "a hidden exit starts hidden");
    assert!(!world.exits[0].open, "and closed");

    // Kill the reactor with one heavy round.
    let reactor = world.reactors[0].p;
    let mut b = luola::sim::entities::Bullet::fire(
        luola::sim::weapons::WeaponId::Gun,
        reactor,
        0.0,
        V2::ZERO,
        true,
    );
    b.damage = 10;
    world.bullets.push(b);
    world.step(InputFrame::default());

    assert!(world.reactors[0].destroyed, "the reactor is gone");
    assert!(world.gates[0].active, "the signal must power the gate");
    assert!(!world.exits[0].hidden, "the signal must reveal the exit");
}

/// An `always` gate slides on its own, stops bullets, and crushes what it
/// catches.
#[test]
fn a_gate_moves_and_crushes() {
    let level = Arc::new(
        Level::parse(
            r#"
name = "Mover"
gravity = 90.0
fuel = 100.0

[player]
pos = [240.0, 60.0]

[[wall]]
points = [[0, 0], [480, 0], [480, 30], [0, 30]]

[[wall]]
points = [[0, 230], [480, 230], [480, 260], [0, 260]]

[[wall]]
points = [[0, 0], [30, 0], [30, 260], [0, 260]]

[[wall]]
points = [[450, 0], [480, 0], [480, 260], [450, 260]]

[[exit]]
pos = [70.0, 180.0]
size = [40.0, 40.0]

[[gate]]
pos = [200.0, 150.0]
size = [40.0, 40.0]
to = [300.0, 150.0]
speed = 120.0
"#,
        )
        .expect("mover fixture parses"),
    );
    let mut world = World::new(level, Scheme::Classic, 1);
    assert!(
        world.gates[0].active,
        "an always gate is live from tick zero"
    );
    let x0 = world.gates[0].rect.center().x;
    for _ in 0..30 {
        world.step(InputFrame::default());
    }
    let x1 = world.gates[0].rect.center().x;
    assert!((x1 - x0).abs() > 5.0, "the gate must slide, {x0} -> {x1}");

    // A shot stops on the gate and digs nothing.
    let gate = world.gates[0].rect.center();
    world.bullets.push(luola::sim::entities::Bullet::fire(
        luola::sim::weapons::WeaponId::Gun,
        gate,
        0.0,
        V2::ZERO,
        true,
    ));
    world.step(InputFrame::default());
    assert!(world.bullets.is_empty(), "the gate must stop the shot");

    // A ship inside an active gate takes the hit.
    let before = world.stats.damage_taken;
    world.ship.body.p = world.gates[0].rect.center();
    world.step(InputFrame::default());
    assert!(
        world.stats.damage_taken > before,
        "the crusher must hit the ship"
    );
}

/// Signal and hidden-exit authoring is validated at load.
#[test]
fn signals_and_hidden_exits_are_validated() {
    let base = |body: &str| {
        format!(
            "name = \"x\"\ngravity = 90.0\nfuel = 10.0\n[player]\npos = [240.0, 60.0]\n\
             [[wall]]\npoints = [[0,0],[480,0],[480,30],[0,30]]\n\
             [[wall]]\npoints = [[0,230],[480,230],[480,260],[0,260]]\n\
             [[wall]]\npoints = [[0,0],[30,0],[30,260],[0,260]]\n\
             [[wall]]\npoints = [[450,0],[480,0],[480,260],[450,260]]\n\
             [[exit]]\npos = [60.0,180.0]\nsize = [40.0,40.0]\n{body}"
        )
    };

    let e = Level::parse(&base(
        "[[signal]]\nwhen = \"reactor\"\naction = \"explode\"\nindex = 0\n",
    ))
    .unwrap_err();
    assert!(e.to_string().contains("unknown action"), "{e}");

    let e = Level::parse(&base(
        "[[signal]]\nwhen = \"reactor\"\naction = \"open_exit\"\nindex = 9\n",
    ))
    .unwrap_err();
    assert!(e.to_string().contains("targets exit"), "{e}");

    let e = Level::parse(&base(
        "[[exit]]\npos = [120.0,180.0]\nsize = [40.0,40.0]\nhidden = true\n",
    ))
    .unwrap_err();
    assert!(e.to_string().contains("hidden"), "{e}");

    let e = Level::parse(&base(
        "[[gate]]\npos = [200.0,120.0]\ntrigger = \"timer\"\n",
    ))
    .unwrap_err();
    assert!(e.to_string().contains("trigger"), "{e}");
}
