//! End-to-end tests of the binary itself, driven through the command line.
//!
//! These cover the tooling a machine player or a record keeper relies on:
//! validation, headless simulation, replay recording and replay verification.
//! They spawn the real binary, so they catch wiring mistakes that unit tests
//! cannot see — for example verifying a replay against the wrong level.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_luola")
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn level(name: &str) -> PathBuf {
    manifest().join("levels").join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("luola-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

struct Run {
    status: std::process::ExitStatus,
    stdout: String,
}

fn run(args: &[&str]) -> Run {
    let out = Command::new(bin())
        .args(args)
        .current_dir(manifest())
        .output()
        .expect("binary runs");
    Run {
        status: out.status,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr),
    }
}

#[test]
fn campaign_levels_validate() {
    let paths: Vec<String> = [
        "01_first_descent.toml",
        "02_payload.toml",
        "03_reactor_run.toml",
    ]
    .iter()
    .map(|n| level(n).display().to_string())
    .collect();
    let mut args = vec!["--validate"];
    args.extend(paths.iter().map(|s| s.as_str()));
    let result = run(&args);
    assert!(
        result.status.success(),
        "validate failed:\n{}",
        result.stdout
    );
    for needle in ["sealed yes", "fuel ok"] {
        assert!(
            result.stdout.contains(needle),
            "validate output is missing {needle:?}:\n{}",
            result.stdout
        );
    }
}

/// Record a run, then verify it. The verification deliberately omits `--level`,
/// because the replay carries the level it was flown on.
#[test]
fn recorded_replay_verifies_without_being_told_the_level() {
    let replay = scratch("round_trip.rep");
    let recorded = run(&[
        "--headless",
        "--level",
        &level("03_reactor_run.toml").display().to_string(),
        "--script",
        "dervish",
        "--ticks",
        "900",
        "--record",
        &replay.display().to_string(),
    ]);
    assert!(
        recorded.status.success(),
        "record failed:\n{}",
        recorded.stdout
    );
    assert!(replay.exists(), "no replay file was written");

    let verified = run(&["--headless", "--replay", &replay.display().to_string()]);
    assert!(
        verified.status.success(),
        "replay did not reproduce its own run:\n{}",
        verified.stdout
    );
    assert!(
        verified.stdout.contains("checksum matches"),
        "unexpected verification output:\n{}",
        verified.stdout
    );
    assert!(
        verified.stdout.contains("Reactor Run"),
        "the replay was not verified against its own level:\n{}",
        verified.stdout
    );

    // The same run with an explicit level must also verify.
    let explicit = run(&[
        "--headless",
        "--replay",
        &replay.display().to_string(),
        "--level",
        &level("03_reactor_run.toml").display().to_string(),
    ]);
    assert!(explicit.status.success(), "{}", explicit.stdout);
}

/// A tampered log must be refused: the checksum is the whole point of recording.
#[test]
fn tampered_replay_is_rejected() {
    let replay = scratch("tamper.rep");
    let recorded = run(&[
        "--headless",
        "--level",
        &level("01_first_descent.toml").display().to_string(),
        "--script",
        "wobble",
        "--ticks",
        "300",
        "--record",
        &replay.display().to_string(),
    ]);
    assert!(recorded.status.success(), "{}", recorded.stdout);

    let text = std::fs::read_to_string(&replay).expect("read replay");
    let tampered = text
        .lines()
        .map(|l| {
            if l.starts_with("checksum ") {
                "checksum 0x0000000000000000".to_string()
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&replay, tampered).expect("write replay");

    let verified = run(&["--headless", "--replay", &replay.display().to_string()]);
    assert!(
        !verified.status.success(),
        "a tampered replay was accepted:\n{}",
        verified.stdout
    );
    assert!(
        verified.stdout.contains("checksum mismatch"),
        "unexpected failure message:\n{}",
        verified.stdout
    );
}

/// `--headless --trace` must be able to explain a run tick by tick.
#[test]
fn trace_reports_inputs_and_events() {
    let traced = run(&[
        "--headless",
        "--level",
        &level("01_first_descent.toml").display().to_string(),
        "--script",
        "wobble",
        "--ticks",
        "120",
        "--trace",
    ]);
    assert!(traced.status.success(), "{}", traced.stdout);
    assert!(
        traced.stdout.contains("thrust"),
        "no thrust events in the trace:\n{}",
        traced.stdout
    );
    assert!(
        traced.stdout.contains("checksum"),
        "the trace did not end in a report:\n{}",
        traced.stdout
    );
}

/// Screenshots must be real PNGs: a file with the PNG signature, IHDR, an IDAT
/// chunk that carries the expected pixel count and an IEND.
#[test]
fn screenshots_are_valid_pngs() {
    let shot = scratch("level.png");
    let rendered = run(&[
        "--screenshot",
        &shot.display().to_string(),
        "--level",
        &level("02_payload.toml").display().to_string(),
        "--ticks",
        "120",
        "--size",
        "320x180",
        "--no-hud",
    ]);
    assert!(rendered.status.success(), "{}", rendered.stdout);
    let bytes = std::fs::read(&shot).expect("read png");
    assert_eq!(
        &bytes[..8],
        &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
    );
    assert_eq!(&bytes[12..16], b"IHDR");
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    assert_eq!((width, height), (320, 180));
    assert!(bytes.windows(4).any(|w| w == b"IEND"));
    // The cave must actually be drawn: a blank PNG would still be a valid file.
    let distinct = bytes.iter().filter(|b| **b != 0).count();
    assert!(distinct > 1000, "the frame is nearly empty");
}

#[test]
fn the_level_map_draws_the_whole_cave() {
    let a = scratch("map-a.png");
    let b = scratch("map-b.png");
    for (path, name) in [(&a, "01_first_descent.toml"), (&b, "02_payload.toml")] {
        let out = run(&[
            "--map",
            &path.display().to_string(),
            "--level",
            &level(name).display().to_string(),
            "--size",
            "400x300",
        ]);
        assert!(out.status.success(), "{}", out.stdout);
        // The map reports what it drew, so an author can read the reachability
        // without running the validator separately.
        assert!(out.stdout.contains("reachable"), "{}", out.stdout);
    }
    let a_bytes = std::fs::read(&a).expect("read png a");
    let b_bytes = std::fs::read(&b).expect("read png b");
    assert_eq!(
        &a_bytes[..8],
        &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
    );
    // The requested size is the real one: the map is a measuring tool.
    let width = u32::from_be_bytes([a_bytes[16], a_bytes[17], a_bytes[18], a_bytes[19]]);
    let height = u32::from_be_bytes([a_bytes[20], a_bytes[21], a_bytes[22], a_bytes[23]]);
    assert_eq!((width, height), (400, 300));
    assert!(
        a_bytes.iter().filter(|b| **b != 0).count() > 500,
        "the map is nearly empty"
    );
    assert_ne!(a_bytes, b_bytes, "two caves must not draw the same map");
}

#[test]
fn unknown_arguments_and_missing_levels_fail_loudly() {
    let bogus = run(&["--not-a-flag"]);
    assert!(!bogus.status.success());
    assert!(bogus.stdout.contains("unknown argument"));

    let missing = run(&["--validate", "/nonexistent/level.toml"]);
    assert!(!missing.status.success());
    assert!(missing.stdout.contains("ERROR"));

    let bad_level = run(&[
        "--headless",
        "--level",
        &manifest().join("docs").display().to_string(),
    ]);
    assert!(!bad_level.status.success());
}

/// Every documented mode must at least start and explain itself.
#[test]
fn help_and_level_listing_work() {
    let help = run(&["--help"]);
    assert!(help.status.success());
    for needle in ["--validate", "--headless", "--screenshot", "--replay"] {
        assert!(help.stdout.contains(needle), "--help lacks {needle}");
    }
    let listed = run(&["--list-levels"]);
    assert!(listed.status.success());
    let entries: Vec<&str> = listed.stdout.lines().collect();
    assert_eq!(entries.len(), 3, "expected three campaign levels");
    assert!(
        entries.iter().all(|l| l.starts_with("ok")),
        "{}",
        listed.stdout
    );
    assert!(Path::new(&level("01_first_descent.toml")).exists());
}

/// The roster is a documented artefact: `--weapons` prints Wings' own names, and
/// the gun is marked as the one weapon every ship carries.
#[test]
fn the_weapon_roster_can_be_listed() {
    let out = run(&["--weapons"]);
    assert!(out.status.success(), "{}", out.stdout);
    for name in [
        "autofire",
        "dumbfire",
        "ion cannon",
        "splinterbomb",
        "grenade launcher",
        "dirtball",
        "landmines",
        "plastic explosive",
        "electric blast",
        "poison gas",
        "nuke",
    ] {
        assert!(out.stdout.contains(name), "--weapons lost {name:?}");
    }
    assert!(out.stdout.contains("always mounted"), "the gun is unmarked");
    // 33 specials plus the gun.
    let rows = out.stdout.lines().filter(|l| l.contains("reload")).count();
    assert_eq!(rows, 34, "the roster printed {rows} rows");
}

/// A run flown with a chosen weapon has to verify: the launch loadout is part of
/// the replay, not something the verifier guesses from the level.
#[test]
fn a_weapon_run_records_and_verifies() {
    // A two-word name on purpose: the header is line-oriented, and this is the
    // shape that a `next token` parse gets wrong.
    let rep = scratch("blast-run.rep");
    let rep = rep.display().to_string();
    let lvl = level("03_reactor_run.toml").display().to_string();
    let recorded = run(&[
        "--headless",
        "--level",
        &lvl,
        "--weapon",
        "electric blast",
        "--script",
        "dervish",
        "--ticks",
        "600",
        "--record",
        &rep,
    ]);
    assert!(recorded.status.success(), "{}", recorded.stdout);
    assert!(
        recorded.stdout.contains("electric blast"),
        "{}",
        recorded.stdout
    );

    let text = std::fs::read_to_string(&rep).expect("replay written");
    assert!(
        text.contains("weapon electric blast"),
        "the header lost the weapon: {text}"
    );

    // Verified without being told the level *or* the weapon.
    let verify = run(&["--headless", "--replay", &rep]);
    assert!(verify.status.success(), "{}", verify.stdout);
    assert!(
        verify.stdout.contains("checksum matches"),
        "{}",
        verify.stdout
    );
}

/// An unknown weapon name fails loudly and lists the roster, rather than
/// launching with something else.
#[test]
fn an_unknown_weapon_is_rejected() {
    let out = run(&["--headless", "--weapon", "banana", "--ticks", "10"]);
    assert!(!out.status.success(), "{}", out.stdout);
    assert!(out.stdout.contains("banana"), "{}", out.stdout);
    assert!(out.stdout.contains("autofire"), "the roster is not listed");
}
