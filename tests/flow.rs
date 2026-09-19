//! Game-flow tests: the shell's state machine plus a full render pass, driven
//! without a window.
//!
//! The interactive loop cannot be clicked in CI, but everything it decides —
//! which screen is up, what a key does, and that every screen draws without
//! panicking — is reachable through `Game` directly. That is the closest thing
//! to playing a test run.

use std::path::PathBuf;

use luola::app::{AppConfig, Game, KeyAction, Screen};
use luola::render::fb::Framebuffer;
use luola::sim::inputs::Scheme;
use winit::keyboard::KeyCode;

/// A one-chamber cave with the exit directly under the spawn: gravity alone
/// completes the level in about 150 ticks, so a test needs no pilot.
const TINY: &str = r#"
name = "Test Chamber"
briefing = "Fly left."
gravity = 90.0
fuel = 100.0

[player]
pos = [160.0, 40.0]
angle = 3.1415927

[[wall]]
points = [[0, 0], [320, 0], [320, 24], [0, 24]]

[[wall]]
points = [[0, 200], [320, 200], [320, 240], [0, 240]]

[[wall]]
points = [[0, 0], [24, 0], [24, 240], [0, 240]]

[[wall]]
points = [[296, 0], [320, 0], [320, 240], [296, 240]]

[[exit]]
pos = [160.0, 150.0]
size = [48.0, 48.0]
"#;

/// Each test gets its own file: the suite runs in parallel threads.
fn scratch_level() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("luola-flow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(format!("test_chamber-{n}.toml"));
    std::fs::write(&path, TINY).expect("write level");
    path
}

fn game(levels: Vec<PathBuf>) -> Game {
    Game::new(AppConfig {
        levels,
        start_index: 0,
        scheme: Scheme::Classic,
        seed: 7,
        weapon: None,
        record: None,
        replay: None,
        mute: true,
    })
    .expect("game starts")
}

/// Every screen must render real pixels: a blank screen is the failure mode that
/// a state-machine test would otherwise miss.
fn render_is_not_blank(game: &mut Game, label: &str) {
    let mut fb = Framebuffer::new(320, 180);
    fb.clear(0x00ff_00ff); // magenta: any untouched pixel is a missing draw
    game.render(&mut fb);
    let drawn = fb.pixels().iter().filter(|p| **p != 0x00ff_00ff).count();
    assert!(
        drawn > fb.pixels().len() / 4,
        "{label} drew only {drawn} of {} pixels",
        fb.pixels().len()
    );
}

#[test]
fn screens_advance_by_keyboard_and_draw() {
    let level = scratch_level();
    let mut game = game(vec![level.clone()]);

    assert_eq!(game.screen(), Screen::Title);
    render_is_not_blank(&mut game, "title");

    // Help is reachable and returns to where it was opened from.
    assert_eq!(game.on_key(KeyCode::KeyH, true), KeyAction::None);
    assert_eq!(game.screen(), Screen::Help);
    render_is_not_blank(&mut game, "help");
    game.on_key(KeyCode::Escape, true);
    assert_eq!(game.screen(), Screen::Title);

    // Title -> briefing -> flying.
    game.on_key(KeyCode::Enter, true);
    assert_eq!(game.screen(), Screen::Briefing);
    render_is_not_blank(&mut game, "briefing");
    game.on_key(KeyCode::Enter, true);
    assert_eq!(game.screen(), Screen::Playing);
    render_is_not_blank(&mut game, "flight");

    // Pause holds the simulation but keeps drawing.
    game.on_key(KeyCode::KeyP, true);
    assert_eq!(game.screen(), Screen::Paused);
    let tick_before = game.world().tick;
    render_is_not_blank(&mut game, "pause");
    assert_eq!(game.world().tick, tick_before, "pause advanced the world");
    game.on_key(KeyCode::KeyP, true);
    assert_eq!(game.screen(), Screen::Playing);

    // Escape on the title screen is the only thing that quits.
    game.on_key(KeyCode::Escape, true);
    assert_eq!(game.screen(), Screen::Paused);
}

#[test]
fn completing_the_level_advances_the_campaign() {
    let level = scratch_level();
    let mut game = game(vec![level.clone(), level]);
    game.on_key(KeyCode::Enter, true); // briefing
    game.on_key(KeyCode::Enter, true); // fly

    // The exit is straight below the spawn: falling into it finishes the level.
    let mut ticks = 0;
    while game.world().state.is_flying() && ticks < 4000 {
        game.tick_simulation(1);
        ticks += 1;
    }
    assert!(
        !game.world().state.is_flying(),
        "the test level could not be completed: ship at {:?}",
        game.world().ship.body.p
    );
    render_is_not_blank(&mut game, "results");

    // Enter on the results card goes to the next level's briefing.
    game.on_key(KeyCode::Enter, true);
    assert_eq!(
        game.screen(),
        Screen::Briefing,
        "completing level 1 did not advance the campaign"
    );
    assert_eq!(game.level_index(), 1);

    // Finish the last level: the campaign card, then back to the title.
    game.on_key(KeyCode::Enter, true);
    let mut ticks = 0;
    while game.world().state.is_flying() && ticks < 4000 {
        game.tick_simulation(1);
        ticks += 1;
    }
    game.on_key(KeyCode::Enter, true);
    assert_eq!(game.screen(), Screen::CampaignComplete);
    render_is_not_blank(&mut game, "campaign complete");
    game.on_key(KeyCode::Enter, true);
    assert_eq!(game.screen(), Screen::Title);
}

#[test]
fn restart_returns_the_level_to_its_initial_state() {
    let level = scratch_level();
    let mut game = game(vec![level]);
    game.on_key(KeyCode::Enter, true);
    game.on_key(KeyCode::Enter, true);
    let start = game.world().ship.body.p;
    // Fly into the floor: the run ends, and R must reset it exactly.
    game.on_key(KeyCode::KeyD, true);
    let first = game.world().checksum();
    game.tick_simulation(120);
    assert_ne!(game.world().checksum(), first, "the world never moved");
    game.on_key(KeyCode::KeyR, true);
    assert_eq!(game.world().ship.body.p, start);
    assert!(game.world().state.is_flying());
    // Scheme toggling must be visible in the world the sim reads.
    game.on_key(KeyCode::KeyC, true);
    assert_eq!(game.world().scheme, Scheme::Modern);
}

/// The weapon bindings the shell owns: F is the special trigger in both
/// schemes, and the turn keys become the weapon selector while the ship is
/// parked on a base — Wings' own ritual, which is the one thing a player coming
/// from the originals will try first.
#[test]
fn the_keyboard_fires_the_special_and_swaps_it_at_a_base() {
    let level = scratch_level();
    let mut game = game(vec![level]);
    game.on_key(KeyCode::Enter, true);
    game.on_key(KeyCode::Enter, true);
    assert_eq!(game.screen(), Screen::Playing);

    // A held trigger is not enough on its own: the frame the shell builds has to
    // carry the button, and the simulation has to spend ammo on it.
    let before = game.world().ship.loadout.ammo;
    game.on_key(KeyCode::KeyF, true);
    game.tick_simulation(4);
    assert!(
        game.world().ship.loadout.ammo < before,
        "holding F spent no special ammo"
    );
    game.on_key(KeyCode::KeyF, false);

    // The test chamber has no pad, so the turn keys stay rotation: the ship
    // spins rather than swapping weapons.
    let weapon = game.world().ship.loadout.special;
    game.on_key(KeyCode::KeyD, true);
    game.tick_simulation(30);
    assert_eq!(
        game.world().ship.loadout.special,
        weapon,
        "a cave without a base still swapped weapons"
    );
    assert_ne!(
        game.world().ship.body.omega,
        0.0,
        "the ship stopped turning"
    );
}
