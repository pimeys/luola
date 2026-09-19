//! `luola` — command line entry point.
//!
//! Interactive play is the default; everything else exists so the game can be
//! tested and verified without a human: level validation, headless simulation,
//! replay verification and offscreen screenshots.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use luola::app::{self, AppConfig};
use luola::headless::{self, SimRunner};
use luola::image;
use luola::math::V2;
use luola::render::camera::Camera;
use luola::render::fb::Framebuffer;
use luola::render::fx::Fx;
use luola::render::hud;
use luola::render::palette as pal;
use luola::render::scene;
use luola::sim::inputs::Scheme;
use luola::sim::level::Level;
use luola::sim::replay::Replay;
use luola::sim::script::Script;
use luola::sim::terrain::{MAT_DIRT, MAT_GRANULAR, MAT_ROCK};
use luola::sim::validate;
use luola::sim::water::Water;
use luola::sim::weapons::{self as weapons_mod, WeaponId};

const USAGE: &str = "\
luola — a cave flyer (gravity shooter)

USAGE:
  luola [OPTIONS]                      play the campaign
  luola --validate <level.toml>...     check levels load, are sealed and reachable
  luola --headless [OPTIONS]           simulate without a window and print a report
  luola --screenshot OUT.png [OPTIONS] render frames offscreen to PNG
  luola --map OUT.png [OPTIONS]      draw the whole level top-down (a design aid)
  luola --list-levels                  show the campaign paths
  luola --weapons                      list the weapon roster and exit

OPTIONS:
  --level <path>      level to play or simulate (default: campaign --index)
  --index <n>         campaign index to start from (default 0)
  --scheme <s>        classic | modern (default classic)
  --weapon <w>        special weapon to launch with, overriding the level
                      (--weapons lists the roster)
  --seed <n>          simulation seed (default 0x10aa1e17)
  --script <s>        idle | wobble | dervish   headless input source
  --ticks <n>         how many simulation ticks to run (default 1800 = 15s)
  --replay <path>     play back or verify a recorded input log
  --record <path>     record this run to a replay file
  --trace             print every simulation event (headless only)
  --shots <n>         number of screenshot frames (default 1)
  --size <WxH>        framebuffer size for screenshots (default 960x540)
  --no-hud            screenshot without the HUD
  --mute              disable audio
  --help              this text
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("luola: {message}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    level: Option<PathBuf>,
    index: usize,
    scheme: Scheme,
    seed: u64,
    weapon: Option<WeaponId>,
    script: Script,
    ticks: u64,
    replay: Option<PathBuf>,
    record: Option<PathBuf>,
    trace: bool,
    /// True when `--ticks` was given; a replay otherwise plays all of its frames.
    ticks_given: bool,
    validate: Vec<PathBuf>,
    headless: bool,
    screenshot: Option<PathBuf>,
    map: Option<PathBuf>,
    shots: u32,
    size: (u32, u32),
    hud: bool,
    mute: bool,
    list_levels: bool,
    list_weapons: bool,
    help: bool,
}

fn run() -> Result<(), String> {
    let args = parse_args(std::env::args().skip(1).collect())?;

    if args.help {
        print!("{USAGE}");
        return Ok(());
    }
    if args.list_levels {
        for path in app::campaign_paths() {
            let mark = if path.exists() { "ok " } else { "MISSING" };
            println!("{mark} {}", path.display());
        }
        return Ok(());
    }
    if args.list_weapons {
        print_weapons();
        return Ok(());
    }
    if !args.validate.is_empty() {
        return validate_levels(&args.validate);
    }
    if let Some(path) = &args.map {
        return level_map(&args, path);
    }
    if let Some(path) = &args.screenshot {
        return screenshots(&args, path);
    }
    if args.headless {
        return headless_run(&args);
    }

    // Interactive play.
    let replay = match &args.replay {
        Some(path) => Some(Replay::load(path).map_err(|e| format!("{}: {e}", path.display()))?),
        None => None,
    };
    let levels = match (&args.level, &replay) {
        (Some(path), _) => vec![path.clone()],
        (None, Some(replay)) => match &replay.level {
            Some(path) => vec![path.clone()],
            None => app::campaign_paths(),
        },
        (None, None) => app::campaign_paths(),
    };
    let config = AppConfig {
        levels,
        start_index: args.index,
        scheme: args.scheme,
        seed: args.seed,
        weapon: args.weapon,
        record: args.record.clone(),
        replay,
        mute: args.mute,
    };
    app::run(config).map_err(|e| e.to_string())
}

/// Which level a run uses: an explicit `--level` wins, then the level recorded
/// in the replay's own header, then the campaign. Verifying a replay against the
/// wrong cave would report a bogus checksum mismatch.
fn level_for_run(args: &Args, replay: Option<&Replay>) -> Result<Arc<Level>, String> {
    if args.level.is_none()
        && let Some(path) = replay.and_then(|r| r.level.clone())
    {
        return Level::load(&path)
            .map(Arc::new)
            .map_err(|e| format!("{} (from the replay): {e}", path.display()));
    }
    load_level(args)
}

fn load_level(args: &Args) -> Result<Arc<Level>, String> {
    let path = match &args.level {
        Some(p) => p.clone(),
        None => app::campaign_paths()
            .get(args.index)
            .cloned()
            .ok_or_else(|| format!("no campaign level at index {}", args.index))?,
    };
    Level::load(&path).map(Arc::new).map_err(|e| e.to_string())
}

/// The roster, as `--weapons` prints it: Wings' own names, with what each one
/// costs in ammo and what it does to a target.
fn print_weapons() {
    for id in weapons_mod::all() {
        let s = weapons_mod::spec(id);
        let kind = match s.kind {
            luola::sim::weapons::Kind::Bolt => "bolt",
            luola::sim::weapons::Kind::Shell => "shell",
            luola::sim::weapons::Kind::Place => "place",
            luola::sim::weapons::Kind::Stream => "stream",
            luola::sim::weapons::Kind::SelfEffect => "self",
        };
        let tag = if weapons_mod::selectable(id) {
            ""
        } else {
            "  (always mounted)"
        };
        println!(
            "{:<18} {:<6} ammo {:>3}  reload {:.2}s  damage {:>2}{}",
            s.name, kind, s.ammo, s.reload, s.damage, tag
        );
    }
}

fn validate_levels(paths: &[PathBuf]) -> Result<(), String> {
    let mut failures = 0;
    for path in paths {
        match Level::load(path) {
            Ok(level) => {
                println!("{}", level.summary());
                let reach = validate::flood(&level);
                println!(
                    "  {} {}",
                    if reach.ok() { "reach" } else { "REACH" },
                    reach.describe()
                );
                if reach.spawn_buried {
                    println!("  ERROR: player spawn is inside solid terrain");
                    failures += 1;
                }
                if !reach.sealed {
                    println!("  ERROR: the cave is not sealed (the ship can leave the level)");
                    failures += 1;
                }
                if !reach.unreachable.is_empty() {
                    println!("  ERROR: unreachable: {}", reach.unreachable.join(", "));
                    failures += 1;
                }
                // Hard rule: a full tank must be enough to fly to the exit even
                // if the pilot collects nothing on the way.
                if let Some(route) = reach.route_px {
                    let fuel = validate::fuel_for_distance(route);
                    println!(
                        "  fuel exit route needs {:.0} of the {:.0} tank",
                        fuel, level.start_fuel
                    );
                    if fuel > level.start_fuel {
                        println!("  ERROR: the exit cannot be reached even on a full tank");
                        failures += 1;
                    }
                }
                // And the whole mission must fit the tank plus the pods on it.
                if let Some(plan) = reach.mission_distance() {
                    let fuel = validate::fuel_for_distance(plan);
                    let budget = validate::fuel_budget(&level);
                    println!(
                        "  fuel {} mission plan needs {:.0} of {:.0} available ({:.0}%)",
                        if fuel <= budget { "ok  " } else { "TIGHT" },
                        fuel,
                        budget,
                        fuel / budget * 100.0
                    );
                    if fuel > budget {
                        println!(
                            "  WARNING: the mission plan needs more fuel than the level offers"
                        );
                    }
                }
                if reach.reachable_fraction() < 0.9 {
                    println!(
                        "  WARNING: only {:.0}% of the open space is reachable",
                        reach.reachable_fraction() * 100.0
                    );
                }
                // Where the unreachable air is: a cave authored from discs traps
                // air under the flanks of its masses, and knowing the position is
                // the difference between a fixable level and a mystery.
                let pockets: Vec<_> = reach.pockets.iter().filter(|p| p.cells >= 16).collect();
                if !pockets.is_empty() {
                    let total: usize = reach.pockets.iter().map(|p| p.cells).sum();
                    println!("  pockets {total} cells of unreachable air:");
                    for p in pockets {
                        println!(
                            "    {} cells at ({:.0}, {:.0})-({:.0}, {:.0})",
                            p.cells,
                            p.rect.x,
                            p.rect.y,
                            p.rect.right(),
                            p.rect.bottom()
                        );
                    }
                }
            }
            Err(e) => {
                println!("{}: ERROR {e}", path.display());
                failures += 1;
            }
        }
    }
    if failures == 0 {
        Ok(())
    } else {
        Err(format!("{failures} level problem(s)"))
    }
}

fn headless_run(args: &Args) -> Result<(), String> {
    let replay = match &args.replay {
        Some(path) => Some(Replay::load(path).map_err(|e| format!("{}: {e}", path.display()))?),
        None => None,
    };
    let level = level_for_run(args, replay.as_ref())?;
    let report = match &replay {
        Some(replay) => {
            let replay = replay.clone();
            // A replay is verified against *its own* frame count unless the
            // caller asks for a prefix: truncating it would "mismatch" every time.
            let limit = args.ticks_given.then_some(args.ticks);
            let report = headless::verify_replay(level.clone(), &replay, limit);
            println!("level {}", level.name);
            match replay.checksum {
                Some(expected) if expected == report.checksum => {
                    println!("replay checksum matches ({expected:#018x})");
                }
                Some(expected) if !args.ticks_given => {
                    return Err(format!(
                        "replay checksum mismatch: recorded {expected:#018x}, reproduced {:#018x} - \
                         the simulation did not reproduce the run exactly",
                        report.checksum
                    ));
                }
                Some(expected) => {
                    println!(
                        "prefix replay: {expected:#018x} recorded for the whole run, {:#018x} after {} ticks",
                        report.checksum, report.ticks
                    );
                }
                None => println!(
                    "replay carries no checksum; reproduced {:#018x}",
                    report.checksum
                ),
            }
            report
        }
        None => {
            let mut runner =
                SimRunner::with_weapon(level.clone(), args.scheme, args.seed, args.weapon);
            if args.trace {
                run_traced(&mut runner, args);
            } else {
                runner.run_script(args.script, args.ticks);
            }
            if let Some(path) = &args.record {
                runner.finish_replay();
                runner
                    .replay
                    .save(path)
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                println!("wrote {}", path.display());
            }
            runner.report()
        }
    };
    println!("{}", report.describe());
    Ok(())
}

fn run_traced(runner: &mut SimRunner, args: &Args) {
    for i in 0..args.ticks {
        let frame = args.script.frame(i);
        let keys = describe_frame(&frame);
        runner.push(frame);
        for event in runner.world.events() {
            println!("{:>6} {:>18} {}", i, keys, headless::describe_event(event));
        }
    }
}

fn describe_frame(frame: &luola::sim::inputs::InputFrame) -> String {
    let mut parts = Vec::new();
    for (bit, name) in [
        (luola::sim::inputs::BTN_ROTATE_CCW, "ccw"),
        (luola::sim::inputs::BTN_ROTATE_CW, "cw"),
        (luola::sim::inputs::BTN_THRUST, "thrust"),
        (luola::sim::inputs::BTN_FIRE, "fire"),
        (luola::sim::inputs::BTN_BEAM, "beam"),
        (luola::sim::inputs::BTN_SPECIAL, "special"),
        (luola::sim::inputs::BTN_WEAPON_PREV, "weapon-"),
        (luola::sim::inputs::BTN_WEAPON_NEXT, "weapon+"),
    ] {
        if frame.has(bit) {
            parts.push(name);
        }
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("+")
    }
}

fn screenshots(args: &Args, base: &Path) -> Result<(), String> {
    // Replay frames take priority over the script when one is supplied.
    let replayed = match &args.replay {
        Some(path) => Some(Replay::load(path).map_err(|e| format!("{}: {e}", path.display()))?),
        None => None,
    };
    let level = level_for_run(args, replayed.as_ref())?;
    let (w, h) = args.size;
    let shots = args.shots.max(1);

    let scheme = replayed.as_ref().map(|r| r.scheme).unwrap_or(args.scheme);
    let seed = replayed.as_ref().map(|r| r.seed).unwrap_or(args.seed);
    // With a replay and no explicit length, render the whole recorded run.
    let ticks = match &replayed {
        Some(r) if !args.ticks_given => r.frames.len().max(1) as u64,
        _ => args.ticks.max(1),
    };

    let mut runner = SimRunner::with_weapon(
        level.clone(),
        scheme,
        seed,
        replayed.as_ref().and_then(|r| r.weapon).or(args.weapon),
    );
    let mut fx = Fx::new();
    let mut cam = Camera::new(w as f32, h as f32);
    cam.snap(runner.world.ship.body.p, level.bounds);

    // Render `shots` frames spread evenly over the run so animation is visible.
    let mut next_shot = 1u32;
    for i in 0..ticks {
        if next_shot > shots {
            break;
        }
        if i >= ticks * u64::from(next_shot) / u64::from(shots) {
            render_frame(&mut runner, &mut fx, &mut cam, args, w, h, base, next_shot)?;
            next_shot += 1;
        }
        let frame = match &replayed {
            Some(r) => r.frames.get(i as usize).copied().unwrap_or_default(),
            None => args.script.frame(i),
        };
        runner.push(frame);
        fx.consume(&runner.world);
        fx.take_audio();
        cam.follow(
            runner.world.ship.body.p,
            runner.world.ship.body.v,
            level.bounds,
            3.2,
        );
        cam.tick();
        fx.update(&runner.world, &mut cam);
    }
    // One last frame at the end of the run, so `--shots 1` shows the result.
    if next_shot <= shots {
        render_frame(&mut runner, &mut fx, &mut cam, args, w, h, base, next_shot)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_frame(
    runner: &mut SimRunner,
    fx: &mut Fx,
    cam: &mut Camera,
    args: &Args,
    w: u32,
    h: u32,
    base: &Path,
    index: u32,
) -> Result<(), String> {
    let mut fb = Framebuffer::new(w as i32, h as i32);
    scene::draw_world(&mut fb, &runner.world, cam);
    fx.particles.draw(&mut fb, cam);
    if args.hud {
        hud::draw_vignette(&mut fb);
        hud::draw_radar(&mut fb, &runner.world, cam);
        hud::draw_indicators(&mut fb, &runner.world, cam);
        hud::draw_status(&mut fb, &runner.world, 1.0, args.scheme);
        hud::draw_objective_distance(&mut fb, &runner.world);
        if let Some(banner) = fx.banner() {
            hud::draw_banner(&mut fb, banner);
        }
    }
    let path = numbered(base, index, args.shots);
    image::save_rgb(&path, w, h, fb.pixels()).map_err(|e| format!("cannot write {path:?}: {e}"))?;
    println!(
        "wrote {} ({}x{}, tick {}, {})",
        path.display(),
        w,
        h,
        runner.world.tick,
        runner.report().describe()
    );
    Ok(())
}

/// Renders the whole level top-down into one image.
///
/// A cave is negative space, and a cave authored from discs cannot be judged one
/// brush at a time: this is the view where the shape of the cave, the pockets
/// the masses leave, and where the water actually settles are visible at once.
fn level_map(args: &Args, path: &Path) -> Result<(), String> {
    let level = load_level(args)?;
    let (w, h) = args.size;
    let b = level.terrain.bounds;
    let scale = (w as f32 / b.w).min(h as f32 / b.h);
    let mut fb = Framebuffer::new(w as i32, h as i32);
    fb.clear(pal::SPACE);

    // The pools as they settle, so the map shows where the water really sits
    // rather than where its polygon was drawn.
    let mut water = Water::new(&level.terrain, level.water);
    for pool in &level.pools {
        water.fill(&level.terrain, pool);
    }
    // Wake everything, then step until nothing is pending: a step that moves
    // nothing is *not* the same as settled, because the first step after a wake
    // only promotes the tiles that were queued.
    water.wake_all();
    let mut guard = 0;
    while !water.is_settled() && guard < 4000 {
        water.step(&level.terrain);
        guard += 1;
    }

    for py in 0..h as i32 {
        for px in 0..w as i32 {
            let wx = (b.x + px as f32 / scale).floor() as i32;
            let wy = (b.y + py as f32 / scale).floor() as i32;
            let c = match level.terrain.cell(wx, wy) {
                MAT_ROCK => pal::ROCK,
                MAT_GRANULAR => pal::GRANULAR,
                MAT_DIRT => pal::DIRT,
                _ if water.mass(wx, wy) > 0 => pal::LIQUID_FILL,
                _ => pal::CAVE_BG,
            };
            fb.set(px, py, c);
        }
    }

    let mut mark = |p: V2, c: u32| {
        let (x, y) = (((p.x - b.x) * scale) as i32, ((p.y - b.y) * scale) as i32);
        fb.rect_fill(x - 3, y - 3, x + 3, y + 3, c);
    };
    for f in &level.fuel_pods {
        mark(f.pos, pal::FUEL_POD);
    }
    for t in &level.turrets {
        mark(t.pos, pal::TURRET);
    }
    for d in &level.drones {
        mark(d.pos, pal::DRONE);
    }
    for m in &level.mines {
        mark(m.pos, pal::MINE);
    }
    for p in &level.pods {
        mark(p.pos, pal::POD);
    }
    for r in &level.reactors {
        mark(r.pos, pal::REACTOR);
    }
    for a in &level.pads {
        mark(a.rect.center(), pal::PAD);
    }
    for a in &level.exits {
        mark(a.rect.center(), pal::EXIT);
    }
    mark(level.player.pos, pal::SHIP);

    image::save_rgb(path, w, h, fb.pixels()).map_err(|e| format!("cannot write {path:?}: {e}"))?;
    println!(
        "wrote {} ({}x{}, {}, {} open cells, {:.0}% reachable)",
        path.display(),
        w,
        h,
        level.name,
        level.terrain.solid_cells(),
        validate::flood(&level).reachable_fraction() * 100.0
    );
    Ok(())
}

fn numbered(base: &Path, index: u32, total: u32) -> PathBuf {
    if total <= 1 {
        return base.to_path_buf();
    }
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "shot".to_string());
    let ext = base
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "png".to_string());
    let parent = base.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    parent.join(format!("{stem}-{index}.{ext}"))
}

fn parse_args(argv: Vec<String>) -> Result<Args, String> {
    let mut args = Args {
        level: None,
        index: 0,
        scheme: Scheme::Classic,
        seed: 0x10aa_1e17,
        weapon: None,
        script: Script::Wobble,
        ticks: 1800,
        replay: None,
        record: None,
        trace: false,
        ticks_given: false,
        validate: Vec::new(),
        headless: false,
        screenshot: None,
        map: None,
        shots: 1,
        size: (960, 540),
        hud: true,
        mute: false,
        list_levels: false,
        list_weapons: false,
        help: false,
    };
    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        let mut next = |name: &str| -> Result<String, String> {
            i += 1;
            argv.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg {
            "--level" => args.level = Some(PathBuf::from(next("--level")?)),
            "--index" => {
                args.index = next("--index")?
                    .parse()
                    .map_err(|_| "--index needs a number".to_string())?
            }
            "--scheme" => {
                let v = next("--scheme")?;
                args.scheme = Scheme::parse(&v).ok_or_else(|| format!("unknown scheme {v:?}"))?;
            }
            "--weapon" => {
                let v = next("--weapon")?;
                args.weapon = Some(weapons_mod::parse(&v).ok_or_else(|| {
                    format!(
                        "unknown weapon {v:?}; the roster is: {}",
                        weapons_mod::roster()
                    )
                })?);
            }
            "--weapons" => args.list_weapons = true,
            "--seed" => {
                let v = next("--seed")?;
                args.seed = if let Some(hex) = v.strip_prefix("0x") {
                    u64::from_str_radix(hex, 16).map_err(|_| "bad --seed".to_string())?
                } else {
                    v.parse().map_err(|_| "bad --seed".to_string())?
                };
            }
            "--script" => {
                let v = next("--script")?;
                args.script = Script::parse(&v).ok_or_else(|| format!("unknown script {v:?}"))?;
            }
            "--ticks" => {
                args.ticks = next("--ticks")?
                    .parse()
                    .map_err(|_| "--ticks needs a number".to_string())?;
                args.ticks_given = true;
            }
            "--replay" => args.replay = Some(PathBuf::from(next("--replay")?)),
            "--record" => args.record = Some(PathBuf::from(next("--record")?)),
            "--trace" => args.trace = true,
            "--validate" => {
                while let Some(p) = argv.get(i + 1) {
                    if p.starts_with("--") {
                        break;
                    }
                    i += 1;
                    args.validate.push(PathBuf::from(p));
                }
                if args.validate.is_empty() {
                    return Err("--validate needs at least one level path".into());
                }
            }
            "--headless" => args.headless = true,
            "--screenshot" => args.screenshot = Some(PathBuf::from(next("--screenshot")?)),
            "--map" => args.map = Some(PathBuf::from(next("--map")?)),
            "--shots" => {
                args.shots = next("--shots")?
                    .parse()
                    .map_err(|_| "--shots needs a number".to_string())?
            }
            "--size" => {
                let v = next("--size")?;
                let (w, h) = v
                    .split_once(['x', 'X'])
                    .ok_or_else(|| "--size wants WxH".to_string())?;
                args.size = (
                    w.parse().map_err(|_| "bad --size width".to_string())?,
                    h.parse().map_err(|_| "bad --size height".to_string())?,
                );
            }
            "--no-hud" => args.hud = false,
            "--mute" => args.mute = true,
            "--list-levels" => args.list_levels = true,
            "--help" | "-h" => args.help = true,
            other => return Err(format!("unknown argument {other:?} (try --help)")),
        }
        i += 1;
    }
    Ok(args)
}
