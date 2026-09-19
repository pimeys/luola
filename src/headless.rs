//! Headless simulation driver.
//!
//! Everything the interactive app does to the simulation — step it with a script
//! or with a replay log, keep the input log, hash the result — is available here
//! without a window, an audio device or a GPU. That is what makes `--headless`,
//! `--record`/`--replay` verification and the determinism tests possible.

use std::path::PathBuf;
use std::sync::Arc;

use crate::math::V2;
use crate::sim::events::Event;
use crate::sim::inputs::{InputFrame, Scheme};
use crate::sim::level::Level;
use crate::sim::replay::Replay;
use crate::sim::script::Script;
use crate::sim::weapons::WeaponId;
use crate::sim::world::{RunState, Score, Stats, World};

#[derive(Clone, Debug)]
pub struct RunReport {
    pub ticks: u64,
    pub elapsed: f32,
    pub checksum: u64,
    pub state: RunState,
    pub score: Score,
    pub stats: Stats,
    pub ship_pos: V2,
    pub ship_alive: bool,
    pub fuel: f32,
    pub bullets: usize,
    /// Shots and clouds alive at the end of the run.
    pub clouds: usize,
    pub gadgets: usize,
    /// The special the run ended holding, and how much of it is left.
    pub weapon: &'static str,
    pub ammo: u16,
    pub pods_alive: usize,
    pub turrets_alive: usize,
    pub drones_alive: usize,
    pub mines_alive: usize,
    /// Number of events the simulation emitted over the whole run.
    pub events: u64,
}

impl RunReport {
    pub fn describe(&self) -> String {
        let status = match self.state {
            RunState::Flying => "flying".to_string(),
            RunState::ShipLost { .. } => "ship lost".to_string(),
            RunState::Failed { reason, .. } => format!("failed: {}", reason.label()),
            RunState::Complete { outcome, .. } => {
                format!(
                    "complete: {} ({} points)",
                    outcome.label(),
                    self.score.total()
                )
            }
        };
        format!(
            "{} | {:.1}s {} ticks | fuel {:.1} | ship ({:.1}, {:.1}) {} | turrets {} drones {} mines {} pods {} | bullets {} clouds {} gadgets {} | {} x{} | events {} | checksum {:#018x}",
            status,
            self.elapsed,
            self.ticks,
            self.fuel,
            self.ship_pos.x,
            self.ship_pos.y,
            if self.ship_alive { "alive" } else { "lost" },
            self.turrets_alive,
            self.drones_alive,
            self.mines_alive,
            self.pods_alive,
            self.bullets,
            self.clouds,
            self.gadgets,
            self.weapon,
            self.ammo,
            self.events,
            self.checksum,
        )
    }
}

/// Owns a world plus the input log recorded against it.
pub struct SimRunner {
    pub world: World,
    pub replay: Replay,
    pub events: u64,
    level_path: Option<PathBuf>,
}

impl SimRunner {
    pub fn new(level: Arc<Level>, scheme: Scheme, seed: u64) -> Self {
        Self::with_weapon(level, scheme, seed, None)
    }

    /// A run with an explicit launch weapon (`--weapon`, or a replay's header).
    pub fn with_weapon(
        level: Arc<Level>,
        scheme: Scheme,
        seed: u64,
        weapon: Option<WeaponId>,
    ) -> Self {
        let level_path = level.source.clone();
        let mut replay = Replay::new(level_path.clone(), level.name.clone(), scheme, seed);
        let mut world = World::new(level, scheme, seed);
        match weapon {
            Some(w) => {
                world.arm(w);
                replay.weapon = Some(w);
            }
            None => replay.weapon = Some(world.ship.loadout.special),
        }
        Self {
            world,
            replay,
            events: 0,
            level_path,
        }
    }

    pub fn push(&mut self, frame: InputFrame) {
        self.world.step(frame);
        self.events += self.world.events().len() as u64;
        self.replay.push(frame);
    }

    pub fn run_script(&mut self, script: Script, ticks: u64) {
        for _ in 0..ticks {
            self.push(script.frame(self.world.tick));
        }
    }

    pub fn run_frames(&mut self, frames: &[InputFrame]) {
        for frame in frames {
            self.push(*frame);
        }
    }

    pub fn report(&self) -> RunReport {
        let w = &self.world;
        RunReport {
            ticks: w.tick,
            elapsed: w.elapsed(),
            checksum: w.checksum(),
            state: w.state,
            score: w.score,
            stats: w.stats,
            ship_pos: w.ship.body.p,
            ship_alive: w.ship.alive,
            fuel: w.ship.fuel,
            bullets: w.bullets.len(),
            clouds: w.clouds.len(),
            gadgets: w.gadgets.len(),
            weapon: crate::sim::weapons::spec(w.ship.loadout.special).name,
            ammo: w.ship.loadout.ammo,
            pods_alive: w.pods_alive(),
            turrets_alive: w.turrets.len(),
            drones_alive: w.drones.len(),
            mines_alive: w.mines.len(),
            events: self.events,
        }
    }

    /// Fills in the replay's checksum/result lines from the finished world.
    pub fn finish_replay(&mut self) {
        self.replay.checksum = Some(self.world.checksum());
        self.replay.result = Some(format!(
            "{} {:.1}s score={}",
            match self.world.state {
                RunState::Flying => "running",
                RunState::ShipLost { .. } => "ship-lost",
                RunState::Failed { reason, .. } => reason.label(),
                RunState::Complete { outcome, .. } => outcome.label(),
            },
            self.world.elapsed(),
            self.world.score.total(),
        ));
    }

    pub fn level_path(&self) -> Option<&PathBuf> {
        self.level_path.as_ref()
    }
}

/// Re-simulates a recorded input log and reports what came out.
pub fn verify_replay(level: Arc<Level>, replay: &Replay, max_ticks: Option<u64>) -> RunReport {
    let mut runner = SimRunner::with_weapon(level, replay.scheme, replay.seed, replay.weapon);
    let frames = match max_ticks {
        Some(n) => &replay.frames[..(n as usize).min(replay.frames.len())],
        None => &replay.frames[..],
    };
    runner.run_frames(frames);
    runner.report()
}

/// Human-readable one-line summary of a single event, used by `--trace`.
pub fn describe_event(e: &Event) -> String {
    match e {
        Event::Thrust { p, power, .. } => format!("thrust {power:.1} @ {:.0},{:.0}", p.x, p.y),
        Event::Bubble { p } => format!("bubble @ {:.0},{:.0}", p.x, p.y),
        Event::Carve {
            p,
            radius,
            material,
        } => format!(
            "dig {} r={radius:.1} @ {:.0},{:.0}",
            if *material == crate::sim::terrain::MAT_ROCK {
                "rock"
            } else if *material == crate::sim::terrain::MAT_GRANULAR {
                "granular"
            } else {
                "dirt"
            },
            p.x,
            p.y
        ),
        Event::Splash { p, power } => format!("splash {power:.1} @ {:.0},{:.0}", p.x, p.y),
        Event::Bullet {
            from_player,
            p,
            weapon,
            ..
        } => format!(
            "shot {} ({}) @ {:.0},{:.0}",
            if *from_player { "player" } else { "enemy" },
            crate::sim::weapons::spec(*weapon).name,
            p.x,
            p.y
        ),
        Event::Special { weapon, p, .. } => format!(
            "special {} @ {:.0},{:.0}",
            crate::sim::weapons::spec(*weapon).name,
            p.x,
            p.y
        ),
        Event::Blast { p, radius, weapon } => format!(
            "blast {} r={radius:.0} @ {:.0},{:.0}",
            crate::sim::weapons::spec(*weapon).name,
            p.x,
            p.y
        ),
        Event::Fill { p, radius } => format!("fill r={radius:.0} @ {:.0},{:.0}", p.x, p.y),
        Event::Cloud { p, radius, kind } => {
            format!("cloud {kind:?} r={radius:.0} @ {:.0},{:.0}", p.x, p.y)
        }
        Event::Gadget { p, kind } => format!("gadget {kind:?} @ {:.0},{:.0}", p.x, p.y),
        Event::Freeze { p } => format!("freeze @ {:.0},{:.0}", p.x, p.y),
        Event::NetHit { p } => format!("net @ {:.0},{:.0}", p.x, p.y),
        Event::Emp { p, radius } => format!("emp r={radius:.0} @ {:.0},{:.0}", p.x, p.y),
        Event::Blink { from, to } => format!(
            "blink {:.0},{:.0} -> {:.0},{:.0}",
            from.x, from.y, to.x, to.y
        ),
        Event::Tether { p, on } => format!(
            "tether {} @ {:.0},{:.0}",
            if *on { "on" } else { "off" },
            p.x,
            p.y
        ),
        Event::Grab { p, on } => format!(
            "grab {} @ {:.0},{:.0}",
            if *on { "on" } else { "off" },
            p.x,
            p.y
        ),
        Event::Jettison { p } => format!("jettison @ {:.0},{:.0}", p.x, p.y),
        Event::Impact { p, .. } => format!("impact @ {:.0},{:.0}", p.x, p.y),
        Event::Explosion { p, power } => format!("explosion {power:.1} @ {:.0},{:.0}", p.x, p.y),
        Event::ShieldHit { p, .. } => format!("shield hit @ {:.0},{:.0}", p.x, p.y),
        Event::Pickup { p, fuel } => format!("pickup fuel={fuel} @ {:.0},{:.0}", p.x, p.y),
        Event::Refuel { p, amount } => format!("refuel +{amount:.0} @ {:.0},{:.0}", p.x, p.y),
        Event::PadLanding { p } => format!("pad @ {:.0},{:.0}", p.x, p.y),
        Event::Beam { on } => format!("beam {}", if *on { "on" } else { "off" }),
        Event::Landing { p, speed } => format!("landing {speed:.0} px/s @ {:.0},{:.0}", p.x, p.y),
        Event::PodAttached { p } => format!("pod attached @ {:.0},{:.0}", p.x, p.y),
        Event::PodLost { p } => format!("pod lost @ {:.0},{:.0}", p.x, p.y),
        Event::ReactorHit { p } => format!("reactor hit @ {:.0},{:.0}", p.x, p.y),
        Event::ReactorCritical { p } => format!("reactor critical @ {:.0},{:.0}", p.x, p.y),
        Event::ReactorDestroyed { p } => format!("reactor destroyed @ {:.0},{:.0}", p.x, p.y),
        Event::TurretDestroyed { p } => format!("turret destroyed @ {:.0},{:.0}", p.x, p.y),
        Event::DroneDestroyed { p } => format!("drone destroyed @ {:.0},{:.0}", p.x, p.y),
        Event::MineBlast { p } => format!("mine blast @ {:.0},{:.0}", p.x, p.y),
        Event::ShipLost { p } => format!("ship lost @ {:.0},{:.0}", p.x, p.y),
        Event::AlarmPulse { .. } => "alarm".to_string(),
        Event::ExitOpen { p } => format!("exit open @ {:.0},{:.0}", p.x, p.y),
        Event::LevelComplete { p } => format!("level complete @ {:.0},{:.0}", p.x, p.y),
    }
}
