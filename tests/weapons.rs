//! The Wings weapon system: the roster, the base ritual and what each family of
//! weapon actually does (`docs/finnish_cave_flyer_weapons.md` §4, §7).
//!
//! These tests defend the contracts the port is judged on: the roster is Wings'
//! own data, every one of the 33 specials can be fired and costs ammo, a special
//! is only swapped at a base, and the behaviour flags (blast, cloud, fill,
//! freeze, net, tether, blink, EMP) do what the roster says they do.

use std::sync::Arc;

use luola::math::V2;
use luola::sim::events::Event;
use luola::sim::inputs::{
    BTN_FIRE, BTN_SPECIAL, BTN_THRUST, BTN_WEAPON_NEXT, BTN_WEAPON_PREV, InputFrame, Scheme,
};
use luola::sim::level::Level;
use luola::sim::weapons::{self, Effect, GadgetKind, Kind, WeaponId};
use luola::sim::world::World;

/// A sealed box with open air, a floor, a ceiling, a pad on the floor and an
/// exit. 480x300, so a shot has room to fly but always lands inside the level.
const ARENA: &str = r#"
name = "Weapon Arena"
gravity = 90.0
fuel = 600.0

[player]
pos = [120.0, 120.0]

[[wall]]
points = [[0, 0], [480, 0], [480, 20], [0, 20]]

[[wall]]
points = [[0, 280], [480, 280], [480, 300], [0, 300]]

[[wall]]
points = [[0, 0], [20, 0], [20, 300], [0, 300]]

[[wall]]
points = [[460, 0], [480, 0], [480, 300], [460, 300]]

[[exit]]
pos = [430.0, 60.0]
size = [40.0, 40.0]

[[pad]]
pos = [300.0, 275.0]
size = [64.0, 10.0]
"#;

/// The same box with the ship already parked on the pad: the base state.
const PAD_ARENA: &str = r#"
name = "Base"
gravity = 2.0
fuel = 600.0

[weapons]
available = ["shotgun", "mine", "teleport", "nuke"]
start = "shotgun"

[player]
pos = [300.0, 260.0]

[[wall]]
points = [[0, 0], [480, 0], [480, 20], [0, 20]]

[[wall]]
points = [[0, 280], [480, 280], [480, 300], [0, 300]]

[[wall]]
points = [[0, 0], [20, 0], [20, 300], [0, 300]]

[[wall]]
points = [[460, 0], [480, 0], [480, 300], [460, 300]]

[[exit]]
pos = [430.0, 60.0]
size = [40.0, 40.0]

[[pad]]
pos = [300.0, 275.0]
size = [64.0, 10.0]
"#;

/// Weak gravity, so a test can watch something for several seconds without the
/// ship falling onto the floor and dying.
const FIELD_ARENA: &str = r#"
name = "Weapon Field"
gravity = 4.0
fuel = 600.0

[player]
pos = [120.0, 90.0]

[[wall]]
points = [[0, 0], [480, 0], [480, 20], [0, 20]]

[[wall]]
points = [[0, 280], [480, 280], [480, 300], [0, 300]]

[[wall]]
points = [[0, 0], [20, 0], [20, 300], [0, 300]]

[[wall]]
points = [[460, 0], [480, 0], [480, 300], [460, 300]]

[[exit]]
pos = [430.0, 60.0]
size = [40.0, 40.0]
"#;

/// A cave with things to shoot: a turret up and to the right of the ship, a
/// drone in between, and a reactor behind the turret.
const TARGET_ARENA: &str = r#"
name = "Targets"
gravity = 30.0
fuel = 600.0

[player]
pos = [120.0, 120.0]

[[wall]]
points = [[0, 0], [480, 0], [480, 20], [0, 20]]

[[wall]]
points = [[0, 280], [480, 280], [480, 300], [0, 300]]

[[wall]]
points = [[0, 0], [20, 0], [20, 300], [0, 300]]

[[wall]]
points = [[460, 0], [480, 0], [480, 300], [460, 300]]

[[exit]]
pos = [430.0, 60.0]
size = [40.0, 40.0]

[[turret]]
pos = [400.0, 120.0]

[[drone]]
pos = [260.0, 120.0]

[[reactor]]
pos = [420.0, 240.0]
"#;

/// A basin the water settles into, in the middle of the box.
const WATER_ARENA: &str = r#"
name = "Water"
gravity = 4.0
fuel = 600.0

[player]
pos = [60.0, 255.0]

[[wall]]
points = [[0, 0], [480, 0], [480, 20], [0, 20]]

[[wall]]
points = [[0, 280], [480, 280], [480, 300], [0, 300]]

[[wall]]
points = [[0, 0], [20, 0], [20, 300], [0, 300]]

[[wall]]
points = [[460, 0], [480, 0], [480, 300], [460, 300]]

[[exit]]
pos = [430.0, 60.0]
size = [40.0, 40.0]

[[liquid]]
points = [[230, 200], [430, 200], [430, 279], [230, 279]]
"#;

fn level(text: &str) -> Arc<Level> {
    Arc::new(Level::parse(text).expect("fixture parses"))
}

/// A world with `weapon` mounted, as if the pilot had taken it off a base.
fn armed(text: &str, weapon: WeaponId) -> World {
    let mut w = World::new(level(text), Scheme::Classic, 0x5eed);
    w.arm(weapon);
    w
}

/// Steps `ticks` with a constant frame and returns every event of the run.
fn run(world: &mut World, ticks: u32, frame: InputFrame) -> Vec<Event> {
    let mut out = Vec::new();
    for _ in 0..ticks {
        world.step(frame);
        out.extend_from_slice(world.events());
    }
    out
}

fn held(button: u16) -> InputFrame {
    InputFrame::default().with(button, true)
}

fn has(events: &[Event], pred: impl Fn(&Event) -> bool) -> bool {
    events.iter().any(pred)
}

/// Wings' `WEAPONS.DAT`, as extracted from the shipped v1.40 data file: 35
/// records, of which `Base` (record 20) and `Cannon` (21) are level furniture
/// rather than weapons. The roster in the game has to be exactly the other 33,
/// in the file's own order.
#[test]
fn the_roster_is_wings_weapons_dat() {
    const WINGS: [&str; 33] = [
        "autofire",
        "dumbfire",
        "troopers",
        "ion cannon",
        "multicannon",
        "shotgun",
        "splinterbomb",
        "bomb",
        "mine",
        "missile",
        "freezer",
        "poison",
        "harpoon",
        "nucleus",
        "grenade launcher",
        "dirtball",
        "digger",
        "hellfire",
        "torpedo",
        "landmines",
        "rockets",
        "bats",
        "teleport",
        "gravitor",
        "plastic explosive",
        "watercannon",
        "fireworks",
        "bouncer",
        "net",
        "shield",
        "electric blast",
        "poison gas",
        "nuke",
    ];
    let names: Vec<&str> = weapons::SPECIALS
        .iter()
        .map(|id| weapons::spec(*id).name)
        .collect();
    assert_eq!(
        names,
        WINGS.to_vec(),
        "the roster drifted from the data file"
    );
    assert_eq!(weapons::all().count(), 34, "gun plus 33 specials");
    assert_eq!(weapons::COUNT, 34);
    assert!(!weapons::selectable(WeaponId::Gun));
    assert!(weapons::SPECIALS.iter().all(|w| weapons::selectable(*w)));
    // `Base` and `Cannon` are in the same table and are not weapons here.
    assert!(weapons::parse("base").is_none());
    assert!(weapons::parse("cannon").is_none());
    // Every name round-trips, whatever the level author types.
    for id in weapons::all() {
        let name = weapons::spec(id).name;
        assert_eq!(weapons::parse(name), Some(id));
        assert_eq!(weapons::parse(&name.replace(' ', "_")), Some(id));
        assert_eq!(
            weapons::parse(&name.replace(' ', "-").to_uppercase()),
            Some(id)
        );
    }
}

/// Every weapon the game offers must be fireable, cost ammo, and leave the
/// right kind of trace. This is the test that catches a roster row that was
/// added to the table but never wired to a firing path.
#[test]
fn every_special_fires_and_costs_ammo() {
    for id in weapons::SPECIALS {
        let spec = weapons::spec(id);
        let mut w = armed(ARENA, id);
        let before = w.ship.loadout.ammo;
        let start = w.ship.body.p;
        // Aim right into open air, trigger down, for long enough that even the
        // slowest reload (nuke, 3 s) gets a shot away.
        let events = run(&mut w, 500, held(BTN_SPECIAL));

        assert!(
            w.ship.loadout.ammo < before,
            "{}: fired but spent no ammo",
            spec.name
        );
        assert!(
            has(
                &events,
                |e| matches!(e, Event::Special { weapon, .. } if *weapon == id)
            ),
            "{}: no Special event",
            spec.name
        );

        let trace = match spec.kind {
            Kind::Bolt | Kind::Shell => !w.bullets.is_empty() || w.terrain.digest() != 0,
            Kind::Stream => !w.clouds.is_empty() || !events.is_empty(),
            Kind::Place => match spec.gadget {
                Some(GadgetKind::Mine) | Some(GadgetKind::Landmine) => {
                    w.mines.iter().any(|m| m.from_player)
                }
                _ => !w.gadgets.is_empty(),
            },
            Kind::SelfEffect => match spec.effect {
                Effect::Shield => w.ship.shield_field > 0.0,
                Effect::Blink => w.ship.body.p.dist(start) > 10.0,
                Effect::Emp => true,
                _ => false,
            },
        };
        assert!(trace, "{}: fired but left no trace", spec.name);
    }
}

/// The gun is the one thing that never runs out — Wings' normal gun, present in
/// every ship whatever the special is.
#[test]
fn the_gun_never_runs_out() {
    let mut w = armed(ARENA, WeaponId::Mine);
    let ammo = w.ship.loadout.ammo;
    let events = run(&mut w, 600, held(BTN_FIRE));
    let shots = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::Bullet {
                    from_player: true,
                    ..
                }
            )
        })
        .count();
    assert!(shots > 20, "the gun fired {shots} times in five seconds");
    assert_eq!(w.ship.loadout.ammo, ammo, "the gun spent special ammo");
    assert_eq!(weapons::spec(WeaponId::Gun).ammo, 0, "the gun has a limit");
}

/// The base ritual: parked and slow on a pad, the turn keys step through the
/// level's roster, and the new weapon arrives fully armed.
#[test]
fn a_base_swaps_the_weapon_and_rearms_it() {
    let mut w = World::new(level(PAD_ARENA), Scheme::Classic, 7);
    assert_eq!(w.ship.loadout.special, WeaponId::Shotgun);
    assert!(w.docked_on_pad(), "the fixture starts parked");
    assert_eq!(w.level.weapons.len(), 4);

    run(&mut w, 20, held(BTN_WEAPON_NEXT));
    assert_eq!(w.ship.loadout.special, WeaponId::Mine, "next in the list");
    assert_eq!(
        w.ship.loadout.ammo,
        weapons::spec(WeaponId::Mine).ammo,
        "a new weapon arrives loaded"
    );

    run(&mut w, 20, held(BTN_WEAPON_PREV));
    assert_eq!(w.ship.loadout.special, WeaponId::Shotgun, "and back again");

    // Wrapping: one more step back lands on the last weapon in the list.
    run(&mut w, 20, held(BTN_WEAPON_PREV));
    assert_eq!(w.ship.loadout.special, WeaponId::Nuke);
}

/// And the ritual is a ritual: it only works at a base. Wings changes weapons
/// with the turn buttons because that is what you have when you are parked.
#[test]
fn a_weapon_cannot_be_swapped_in_mid_air() {
    // Flying above the pad's zone is flying, not docking.
    let mut w = World::new(level(PAD_ARENA), Scheme::Classic, 7);
    w.ship.body.p.y -= 200.0;
    w.ship.prev = w.ship.body;
    assert!(!w.docked_on_pad(), "the ship is not on the pad any more");
    let weapon = w.ship.loadout.special;
    run(&mut w, 60, held(BTN_WEAPON_NEXT));
    assert_eq!(w.ship.loadout.special, weapon, "swapped in mid-air");

    // And neither is crossing the pad too fast to land on it.
    let mut w = World::new(level(PAD_ARENA), Scheme::Classic, 7);
    w.ship.body.v = V2::new(90.0, 0.0);
    assert!(!w.docked_on_pad(), "a ship at speed is not docked");
    let weapon = w.ship.loadout.special;
    run(&mut w, 4, held(BTN_WEAPON_NEXT));
    assert_eq!(w.ship.loadout.special, weapon, "swapped while overspeeding");
}

/// Landing on a base re-arms the special, exactly as it refuels the tank: this
/// is the whole reason to leave the fight and come back.
#[test]
fn a_base_rearms_an_empty_special() {
    let mut w = World::new(level(PAD_ARENA), Scheme::Classic, 7);
    w.ship.loadout.ammo = 0;
    assert!(!w.ship.loadout.ready());
    let events = run(&mut w, 4, InputFrame::default());
    assert!(
        has(&events, |e| matches!(e, Event::PadLanding { .. })),
        "the pad did not fire"
    );
    assert_eq!(
        w.ship.loadout.ammo,
        weapons::spec(w.ship.loadout.special).ammo
    );
    assert!(w.ship.loadout.ready());
}

/// An empty special stays empty until a base: no ammo, no shot, no event.
#[test]
fn an_empty_special_fires_nothing() {
    let mut w = armed(ARENA, WeaponId::Shotgun);
    w.ship.loadout.ammo = 0;
    let events = run(&mut w, 240, held(BTN_SPECIAL));
    assert!(!has(&events, |e| matches!(e, Event::Special { .. })));
    assert!(w.bullets.is_empty());
}

/// A mine the player laid is a weapon, not a trap for the pilot: it ignores the
/// ship that dropped it.
#[test]
fn a_laid_mine_ignores_its_owner() {
    let mut w = armed(FIELD_ARENA, WeaponId::Mine);
    run(&mut w, 40, held(BTN_SPECIAL));
    assert_eq!(w.laid_mines(), 1, "the mine was not laid");
    // Sit on top of it for three seconds: nothing should happen.
    let events = run(&mut w, 360, InputFrame::default());
    assert!(
        !has(&events, |e| matches!(e, Event::MineBlast { .. })),
        "the mine went off on its owner"
    );
    assert_eq!(w.laid_mines(), 1);
    assert!(w.ship.alive);
}

/// The other half: laid mines do hunt what is hostile, and their blast is what
/// the level's own mines always were.
#[test]
fn a_laid_mine_detonates_on_a_hostile() {
    let mut w = armed(TARGET_ARENA, WeaponId::Mine);
    // Fire the mine towards the drone and wait for it to arm and settle.
    let events = run(&mut w, 240, held(BTN_SPECIAL));
    run(&mut w, 480, InputFrame::default());
    assert!(
        has(&events, |e| matches!(e, Event::MineBlast { .. }))
            || w.laid_mines() == 0
            || !w.drones.iter().any(|d| d.alive),
        "the mine never engaged the drone"
    );
}

/// `Freezer` stops a turret shooting: the observation is that a turret which
/// would otherwise be firing at the ship in its line of sight goes silent.
#[test]
fn a_frozen_turret_cannot_fire() {
    let control = {
        let mut w = World::new(level(TARGET_ARENA), Scheme::Classic, 3);
        run(&mut w, 240, InputFrame::default())
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::Bullet {
                        from_player: false,
                        ..
                    }
                )
            })
            .count()
    };
    assert!(control > 0, "the control turret never fired at all");

    let mut w = armed(TARGET_ARENA, WeaponId::Freezer);
    // Sit directly above the turret, so the shot is a short vertical one and
    // the drone parked in the same lane cannot eat it.
    w.ship.body.p = V2::new(400.0, 60.0);
    w.ship.body.angle = std::f32::consts::FRAC_PI_2;
    let events = run(&mut w, 60, held(BTN_SPECIAL));
    assert!(
        has(&events, |e| matches!(e, Event::Freeze { .. })),
        "the bolt never landed"
    );
    assert!(
        w.turrets.iter().any(|t| !t.operational()),
        "the turret shrugged the freeze off"
    );
    // And it stays off: a frozen turret is silent for the freeze's duration.
    let during = run(&mut w, 180, InputFrame::default())
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::Bullet {
                    from_player: false,
                    ..
                }
            )
        })
        .count();
    assert_eq!(during, 0, "a frozen turret fired {during} shots");
}

/// `Net` tangles a drone: a netted drone is a rock, so it stops closing on the
/// ship while the net holds.
#[test]
fn a_netted_drone_stops_closing() {
    let mut w = armed(TARGET_ARENA, WeaponId::Net);
    let events = run(&mut w, 200, held(BTN_SPECIAL));
    assert!(
        has(&events, |e| matches!(e, Event::NetHit { .. }))
            || w.drones.iter().any(|d| d.netted > 0.0)
            || !w.drones.iter().any(|d| d.alive),
        "the net never caught the drone"
    );
    let netted = w.drones.iter().find(|d| d.alive && d.netted > 0.0);
    if let Some(d) = netted {
        assert!(!d.mobile(), "a netted drone is still flying");
        assert!(!d.thrusting, "a netted drone is still thrusting");
    }
}

/// `Dirtball` is the one weapon that builds: the cave gains solid cells.
#[test]
fn a_dirtball_adds_terrain() {
    let mut w = armed(ARENA, WeaponId::Dirtball);
    let before = w.terrain.solid_cells();
    let events = run(&mut w, 300, held(BTN_SPECIAL));
    assert!(
        has(&events, |e| matches!(e, Event::Fill { .. })),
        "no terrain was added"
    );
    assert!(
        w.terrain.solid_cells() > before,
        "solid cells {} -> {}",
        before,
        w.terrain.solid_cells()
    );
}

/// `Digger` is the tunnelling tool: aim it at a wall and the wall gains a hole
/// far deeper than the bullet-sized crater every other weapon leaves.
#[test]
fn a_digger_tunnels() {
    let gun = {
        let mut w = armed(ARENA, WeaponId::Gun);
        let before = w.terrain.solid_cells();
        run(&mut w, 200, held(BTN_FIRE));
        before - w.terrain.solid_cells()
    };
    let mut w = armed(ARENA, WeaponId::Digger);
    let before = w.terrain.solid_cells();
    run(&mut w, 400, held(BTN_SPECIAL));
    let dug = before - w.terrain.solid_cells();
    assert!(dug > 0, "the digger removed nothing");
    assert!(
        dug > gun,
        "the digger ({dug}) is no better than the gun ({gun})"
    );
}

/// Water stops a shot: drag eats it within a body's depth, so there is a bound
/// on how far a round can bore into a pool. The torpedo is the weapon that
/// ploughs straight through instead, which is what `water_ok` is for.
#[test]
fn water_stops_shots_but_not_the_torpedo() {
    /// The deepest point any player shot reached over `ticks`, in world x.
    fn deepest(weapon: WeaponId, ticks: u32) -> f32 {
        let mut w = armed(WATER_ARENA, weapon);
        let before = w.bullets.len();
        w.step(held(BTN_SPECIAL).with(BTN_FIRE, true));
        assert!(w.bullets.len() > before, "no shot left the ship");
        let mut deep = 0.0f32;
        for _ in 0..ticks {
            w.step(InputFrame::default());
            for b in &w.bullets {
                deep = deep.max(b.p.x);
            }
        }
        deep
    }

    // The pool spans x 230..430. A 430 px/s round is dragged to below the water
    // floor within ~86 px of entering it, so it can never see the far side.
    let gun = deepest(WeaponId::Gun, 300);
    assert!(
        gun < 340.0,
        "a gun round got to x {gun:.0}, past the pool's far wall"
    );

    // The torpedo is made for this: it crosses and dies on the east wall.
    let torp = deepest(WeaponId::Torpedo, 300);
    assert!(
        torp > gun + 60.0,
        "the torpedo ({torp:.0}) did not out-reach the gun ({gun:.0})"
    );
}

/// The teleporter moves the ship along its aim — and refuses to fire into rock,
/// which is the difference between a movement tool and a suicide button.
#[test]
fn the_teleporter_moves_and_refuses_rock() {
    let mut w = armed(ARENA, WeaponId::Teleport);
    let from = w.ship.body.p;
    let ammo = w.ship.loadout.ammo;
    let events = run(&mut w, 8, held(BTN_SPECIAL));
    assert!(has(&events, |e| matches!(e, Event::Blink { .. })));
    assert!(w.ship.body.p.dist(from) > 100.0, "the ship did not move");
    assert_eq!(w.ship.loadout.ammo, ammo - 1);

    // Now face the west wall, which is 100 px away at most: a blink with
    // nowhere to go must not be charged for.
    let mut w = World::new(level(ARENA), Scheme::Classic, 0x5eed);
    w.arm(WeaponId::Teleport);
    w.ship.body.angle = std::f32::consts::PI;
    w.ship.body.p = V2::new(40.0, 120.0);
    let ammo = w.ship.loadout.ammo;
    let events = run(&mut w, 8, held(BTN_SPECIAL));
    assert!(!has(&events, |e| matches!(e, Event::Blink { .. })));
    assert_eq!(w.ship.loadout.ammo, ammo, "a refused blink cost ammo");
}

/// The harpoon bites into rock, holds, and reels the ship in while the trigger
/// is down.
#[test]
fn the_harpoon_tethers_and_reels() {
    let mut w = armed(ARENA, WeaponId::Harpoon);
    // Aim at the ceiling.
    w.ship.body.angle = -std::f32::consts::FRAC_PI_2;
    let events = run(&mut w, 40, held(BTN_SPECIAL));
    assert!(
        has(&events, |e| matches!(e, Event::Tether { on: true, .. })),
        "the harpoon never bit"
    );
    let Some((anchor, _)) = w.ship.tether else {
        panic!("no tether after a hit");
    };
    let before = w.ship.body.p.dist(anchor);
    run(&mut w, 60, held(BTN_SPECIAL));
    let after = w.ship.body.p.dist(anchor);
    assert!(
        after < before - 5.0,
        "the tether did not pull the ship in: {before:.1} -> {after:.1}"
    );
    // Let go and the tether is gone.
    run(&mut w, 2, InputFrame::default());
    assert!(w.ship.tether.is_none(), "the tether outlived the trigger");
}

/// The blast rule that keeps bombs honest: a weapon that goes off in your face
/// costs you the shield hit, and the same cave does nothing to you when you are
/// far from the crater.
#[test]
fn a_blast_hurts_the_ship_that_stands_in_it() {
    // The gun has no blast, so the same geometry with the gun is free.
    let mut control = armed(ARENA, WeaponId::Gun);
    control.ship.body.angle = -std::f32::consts::FRAC_PI_2;
    run(&mut control, 200, held(BTN_FIRE));
    assert_eq!(
        control.stats.damage_taken, 0,
        "the gun hurt its own shooter"
    );

    let mut w = armed(ARENA, WeaponId::Nuke);
    w.ship.body.angle = -std::f32::consts::FRAC_PI_2;
    run(&mut w, 300, held(BTN_SPECIAL));
    assert!(
        w.stats.damage_taken > 0,
        "a nuke fired into the ceiling overhead left the ship untouched"
    );
}

/// The `Shield` weapon keeps the shield charged while it runs: two hits inside
/// the down window are survivable, which is the whole point of carrying it.
#[test]
fn the_shield_weapon_absorbs_a_second_hit() {
    // Control: hit the ceiling, fall back into it, and die on the second hit.
    let mut plain = armed(ARENA, WeaponId::Gun);
    plain.ship.body.angle = -std::f32::consts::FRAC_PI_2;
    run(&mut plain, 300, held(BTN_THRUST).with(BTN_FIRE, true));
    assert!(plain.stats.damage_taken > 0, "the control never hit rock");

    let mut w = armed(ARENA, WeaponId::Shield);
    w.ship.body.angle = -std::f32::consts::FRAC_PI_2;
    let events = run(&mut w, 20, held(BTN_SPECIAL));
    assert!(
        has(&events, |e| matches!(e, Event::Special { .. })) && w.ship.shield_field > 0.0,
        "the shield bubble never went up"
    );
    run(&mut w, 280, held(BTN_THRUST));
    assert!(w.ship.alive, "the shield bubble did not hold");
}

/// The electric blast is an EMP: everything it can reach loses power at once.
#[test]
fn the_electric_blast_knocks_out_the_defences() {
    let mut w = World::new(level(TARGET_ARENA), Scheme::Classic, 5);
    w.arm(WeaponId::ElectricBlast);
    // Sit next to the turret and let it have it.
    w.ship.body.p = V2::new(360.0, 120.0);
    let events = run(&mut w, 20, held(BTN_SPECIAL));
    assert!(has(&events, |e| matches!(e, Event::Emp { .. })), "no EMP");
    assert!(
        w.turrets.iter().any(|t| t.emp > 0.0) || !w.turrets.iter().any(|t| t.alive),
        "the turret kept its power"
    );
}

/// A level narrows the roster, which is Wings' `W_SELECT.DAT` and the weapon
/// flagged "1". The mounted weapon has to be the one the cave named.
#[test]
fn a_level_chooses_its_weapons() {
    let l = level(PAD_ARENA);
    assert_eq!(l.weapons.len(), 4);
    assert_eq!(l.start_weapon, WeaponId::Shotgun);
    let w = World::new(l, Scheme::Classic, 7);
    assert_eq!(w.ship.loadout.special, WeaponId::Shotgun);
    assert_eq!(w.ship.loadout.ammo, weapons::spec(WeaponId::Shotgun).ammo);

    // No table at all: Wings' full roster, in file order.
    let all = level(ARENA);
    assert_eq!(all.weapons, weapons::SPECIALS.to_vec());
    assert_eq!(all.start_weapon, WeaponId::Autofire);

    // Malformed tables are load errors, not levels where a weapon quietly
    // never appears.
    let unknown = ARENA.replace(
        "[player]",
        "[weapons]\navailable = [\"banana\"]\n\n[player]",
    );
    let err = Level::parse(&unknown).expect_err("unknown weapon must be rejected");
    assert!(err.to_string().contains("banana"), "{err}");
    let not_available = ARENA.replace(
        "[player]",
        "[weapons]\navailable = [\"mine\"]\nstart = \"nuke\"\n\n[player]",
    );
    let err = Level::parse(&not_available).expect_err("start outside the list must be rejected");
    assert!(err.to_string().contains("nuke"), "{err}");
    let empty = ARENA.replace("[player]", "[weapons]\navailable = []\n\n[player]");
    assert!(
        Level::parse(&empty).is_err(),
        "an empty pool must be rejected"
    );
}

/// Records a short run with `weapon` mounted and hands back the replay text.
fn record_with(level: Arc<Level>, weapon: WeaponId) -> String {
    use luola::headless::SimRunner;
    let mut runner = SimRunner::with_weapon(level, Scheme::Classic, 11, Some(weapon));
    for _ in 0..120 {
        runner.push(held(BTN_SPECIAL));
    }
    runner.finish_replay();
    runner.replay.to_text()
}

/// A replay records the launch weapon, so a run with `--weapon` verifies
/// against the weapon it actually used rather than the level's default.
#[test]
fn a_replay_carries_the_launch_weapon() {
    use luola::headless::SimRunner;
    use luola::sim::replay::Replay;

    let l = level(PAD_ARENA);

    // The header is line-oriented and weapon names contain spaces, so the name
    // is the whole rest of the line: `electric blast` is one weapon, not a
    // weapon called `electric`.
    for (id, name) in [
        (WeaponId::Nuke, "nuke"),
        (WeaponId::ElectricBlast, "electric blast"),
        (WeaponId::GrenadeLauncher, "grenade launcher"),
    ] {
        let text = record_with(l.clone(), id);
        assert!(text.contains(&format!("weapon {name}")), "{name}: {text}");
        let parsed = Replay::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(parsed.weapon, Some(id), "{name}");
    }

    let mut runner = SimRunner::with_weapon(l.clone(), Scheme::Classic, 11, Some(WeaponId::Nuke));
    assert_eq!(runner.world.ship.loadout.special, WeaponId::Nuke);
    for i in 0..300 {
        runner.push(held(BTN_SPECIAL).with(BTN_FIRE, (i / 20u64).is_multiple_of(2)));
    }
    runner.finish_replay();
    let text = runner.replay.to_text();
    assert!(text.contains("weapon nuke"), "the header lost the weapon");

    let parsed = Replay::parse(&text).expect("replay parses");
    assert_eq!(parsed.weapon, Some(WeaponId::Nuke));
    let report = luola::headless::verify_replay(l, &parsed, None);
    assert_eq!(
        report.checksum,
        runner.world.checksum(),
        "the replay did not reproduce the run"
    );

    // And the same run against the level's own starting weapon must disagree:
    // the loadout is part of the run, not decoration.
    let other = Replay {
        weapon: None,
        ..parsed
    };
    let wrong = luola::headless::verify_replay(level(PAD_ARENA), &other, None);
    assert_ne!(wrong.checksum, report.checksum);
}
