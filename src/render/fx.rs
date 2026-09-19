//! Event fan-out: simulation events become particles, sound and screen shake.
//!
//! This is the only place that knows how the game *looks and sounds*; the
//! simulation knows neither.

use crate::audio::AudioCmd;
use crate::math::V2;
use crate::render::camera::Camera;
use crate::render::palette as pal;
use crate::render::particles::Particles;
use crate::sim::events::{Event, Outcome};
use crate::sim::inputs::Scheme;
use crate::sim::tuning::{CARVE_BLAST, DT};
use crate::sim::weapons::{CloudKind, GadgetKind};
use crate::sim::world::World;

/// Transient headline shown above the HUD.
#[derive(Clone, Debug)]
pub struct Banner {
    pub text: String,
    pub sub: String,
    pub ttl: f32,
    pub max_ttl: f32,
    pub color: u32,
}

impl Banner {
    pub fn alpha(&self) -> f32 {
        let t = self.ttl / self.max_ttl;
        (t * 2.2).clamp(0.0, 1.0)
    }
}

pub struct Fx {
    pub particles: Particles,
    pub shake: f32,
    banner: Option<Banner>,
    queue: Vec<Banner>,
    audio: Vec<AudioCmd>,
    thrusting: bool,
    alarm: bool,
    /// Total number of events consumed; handy for smoke tests.
    pub consumed: u64,
}

impl Default for Fx {
    fn default() -> Self {
        Self::new()
    }
}

impl Fx {
    pub fn new() -> Self {
        Self {
            particles: Particles::new(),
            shake: 0.0,
            banner: None,
            queue: Vec::new(),
            audio: Vec::new(),
            thrusting: false,
            alarm: false,
            consumed: 0,
        }
    }

    pub fn clear(&mut self) {
        self.particles.clear();
        self.banner = None;
        self.queue.clear();
        self.audio.clear();
        self.thrusting = false;
        self.alarm = false;
    }

    pub fn banner(&self) -> Option<&Banner> {
        self.banner.as_ref()
    }

    pub fn show_banner(&mut self, text: &str, sub: &str, color: u32, ttl: f32) {
        self.queue.push(Banner {
            text: text.into(),
            sub: sub.into(),
            ttl,
            max_ttl: ttl,
            color,
        });
    }

    /// Drains queued sound commands for the audio engine.
    pub fn take_audio(&mut self) -> Vec<AudioCmd> {
        std::mem::take(&mut self.audio)
    }

    pub fn push_audio(&mut self, cmd: AudioCmd) {
        self.audio.push(cmd);
    }

    /// Translates this tick's events. Called once per simulation step.
    pub fn consume(&mut self, world: &World) {
        let mut thrust = false;
        for event in world.events() {
            self.consumed += 1;
            match *event {
                Event::Thrust { p, angle, power } => {
                    thrust = true;
                    self.particles.thrust(p, angle, power);
                }
                Event::Bubble { p } => self.particles.bubble(p),
                Event::Bullet {
                    p,
                    angle,
                    from_player,
                    weapon,
                } => {
                    if from_player {
                        self.particles.weapon_muzzle(p, angle, weapon);
                    } else {
                        self.particles.muzzle(p, angle, false);
                    }
                    self.audio.push(if from_player {
                        AudioCmd::Fire
                    } else {
                        AudioCmd::TurretFire
                    });
                }
                Event::Impact { p, normal } => self.particles.impact(p, normal),
                Event::Carve {
                    p,
                    radius,
                    material,
                } => {
                    self.particles.carve(p, radius, material);
                    // A bullet bores a hole; a blast takes the floor out with it.
                    // Only the blast is worth shaking the view for.
                    self.audio.push(AudioCmd::Thud {
                        power: (radius / CARVE_BLAST).clamp(0.05, 1.0),
                    });
                    if radius >= 8.0 {
                        self.shake += (radius / CARVE_BLAST).min(1.0) * 3.0;
                    }
                }
                Event::Splash { p, power } => {
                    self.particles.splash(p, power);
                    self.audio.push(AudioCmd::Pickup);
                }
                Event::Explosion { p, power } => {
                    self.particles.explosion(p, power);
                    self.shake += 3.0 + power * 7.0;
                    self.audio.push(AudioCmd::Explosion { size: power });
                }
                // ------------------------------------------------- weapons --
                Event::Special { weapon, p, angle } => {
                    self.particles.weapon_muzzle(p, angle, weapon);
                    self.audio.push(AudioCmd::special(weapon));
                }
                Event::Blast { p, radius, .. } => {
                    let power = (radius / 30.0).clamp(0.5, 3.0);
                    self.particles.explosion(p, power.min(1.5));
                    self.shake += 2.0 + power * 5.0;
                    self.audio.push(AudioCmd::Explosion {
                        size: power.clamp(0.2, 1.0),
                    });
                }
                Event::Fill { p, radius } => {
                    self.particles
                        .carve(p, radius, crate::sim::terrain::MAT_DIRT);
                    self.audio.push(AudioCmd::Thud { power: 0.4 });
                }
                Event::Cloud { p, radius, kind } => {
                    self.particles.cloud(p, radius, kind);
                }
                Event::Gadget { p, kind } => {
                    self.particles.gadget(p, kind);
                    self.audio.push(AudioCmd::UiBlip);
                }
                Event::Freeze { p } => {
                    self.particles.cloud(p, 14.0, CloudKind::Water);
                    self.audio.push(AudioCmd::Freeze);
                }
                Event::NetHit { p } => {
                    self.particles.gadget(p, GadgetKind::Troopers);
                    self.audio.push(AudioCmd::UiSelect);
                }
                Event::Emp { p, radius } => {
                    self.particles.zap(p, radius);
                    self.shake += 8.0;
                    self.audio.push(AudioCmd::Zap);
                }
                Event::Blink { from, to } => {
                    self.particles.blink(from, to);
                    self.audio.push(AudioCmd::Warp);
                }
                Event::Tether { p, on } => {
                    if on {
                        self.particles.tether(p);
                        self.audio.push(AudioCmd::UiSelect);
                    } else {
                        self.audio.push(AudioCmd::UiBlip);
                    }
                }
                Event::Grab { p, on } => {
                    if on {
                        self.particles.impact(p, V2::new(0.0, -1.0));
                        self.shake += 3.0;
                        self.audio.push(AudioCmd::Thud { power: 0.5 });
                        self.show_banner(
                            "GRANULAR WALL",
                            "SHOOT YOURSELF FREE",
                            pal::HUD_WARNING,
                            1.4,
                        );
                    } else {
                        self.audio.push(AudioCmd::UiBlip);
                    }
                }
                Event::Jettison { .. } => {
                    self.audio.push(AudioCmd::UiSelect);
                    self.show_banner("LOAD DUMPED", "FLYING LIGHTER", pal::HUD_ACCENT, 1.2);
                }
                Event::ShieldHit { p, angle } => {
                    self.particles.shield_hit(p, angle);
                    self.shake += 4.0;
                    self.audio.push(AudioCmd::ShieldHit);
                }
                Event::Pickup { p, fuel } => {
                    self.particles
                        .pickup(p, if fuel { pal::FUEL_POD } else { pal::HUD_ACCENT });
                    self.audio.push(AudioCmd::Pickup);
                }
                Event::Refuel { amount, .. } => {
                    if amount > 0.0 {
                        self.audio.push(AudioCmd::UiBlip);
                    }
                }
                Event::PadLanding { p } => {
                    self.particles.pickup(p, pal::PAD);
                    self.audio.push(AudioCmd::UiSelect);
                    self.show_banner("REPAIRED", "FUEL AND SHIELD RESTORED", pal::PAD, 1.4);
                }
                Event::Landing { p, speed } => {
                    self.particles.impact(p, V2::new(0.0, -1.0));
                    self.shake += (speed / 400.0).min(1.0) * 2.0;
                    self.audio.push(AudioCmd::Thud {
                        power: (speed / 400.0).clamp(0.0, 1.0),
                    });
                }
                Event::Beam { on } => {
                    self.audio.push(if on {
                        AudioCmd::UiSelect
                    } else {
                        AudioCmd::UiBlip
                    });
                }
                Event::PodAttached { p } => {
                    self.particles.pickup(p, pal::POD);
                    self.show_banner("PAYLOAD LOCKED", "TARGET LOCKED ON THE BEAM", pal::POD, 1.6);
                }
                Event::PodLost { .. } => {
                    self.show_banner("PAYLOAD LOST", "MISSION FAILED", pal::HUD_BAD, 2.6);
                }
                Event::ReactorHit { .. } => {
                    self.audio.push(AudioCmd::Thud { power: 0.5 });
                }
                Event::ReactorCritical { .. } => {
                    self.show_banner("REACTOR CRITICAL", "", pal::REACTOR_HOT, 2.0);
                }
                Event::ReactorDestroyed { p } => {
                    self.particles.explosion(p, 1.3);
                    self.shake += 14.0;
                    self.alarm = true;
                    self.audio.push(AudioCmd::Alarm { on: true });
                    self.show_banner(
                        "REACTOR DESTROYED",
                        "DEFENCES OFFLINE - ESCAPE NOW",
                        pal::REACTOR_HOT,
                        3.0,
                    );
                }
                Event::TurretDestroyed { .. } => {
                    self.shake += 2.0;
                }
                Event::DroneDestroyed { .. } => {
                    self.shake += 1.5;
                }
                Event::MineBlast { p } => {
                    self.particles.explosion(p, 0.6);
                    self.shake += 5.0;
                    self.audio.push(AudioCmd::Explosion { size: 0.5 });
                }
                Event::ShipLost { p } => {
                    self.particles.explosion(p, 1.4);
                    self.shake += 16.0;
                    self.alarm = false;
                    self.audio.push(AudioCmd::AllStop);
                }
                Event::AlarmPulse { .. } => {
                    self.alarm = true;
                    self.audio.push(AudioCmd::Alarm { on: true });
                }
                Event::ExitOpen { .. } => {
                    self.audio.push(AudioCmd::UiSelect);
                    self.show_banner("EXIT OPEN", "GET OUT", pal::EXIT, 2.2);
                }
                Event::LevelComplete { p } => {
                    self.particles.explosion(p, 0.4);
                    self.alarm = false;
                    self.audio.push(AudioCmd::AllStop);
                }
            }
        }

        // The engine loop is state, not an event: tell audio when it changes.
        if thrust != self.thrusting {
            self.thrusting = thrust;
            self.audio.push(AudioCmd::Thrust {
                on: thrust,
                power: if thrust { 1.0 } else { 0.0 },
            });
        }
        if self.alarm && world.escape.is_none() {
            self.alarm = false;
            self.audio.push(AudioCmd::Alarm { on: false });
        }
    }

    /// Advances particle physics, banners and shake decay.
    ///
    /// Particles feel the water and gravity the simulation actually has, so they
    /// settle where the ship would.
    pub fn update(&mut self, world: &World, cam: &mut Camera) {
        self.particles.update(&world.water, world.level.gravity);
        cam.kick(self.shake * DT * 6.0);
        self.shake *= 0.86;
        if self.shake < 0.01 {
            self.shake = 0.0;
        }
        if let Some(b) = self.banner.as_mut() {
            b.ttl -= DT;
            if b.ttl <= 0.0 {
                self.banner = None;
            }
        }
        if self.banner.is_none()
            && let Some(next) = self.queue.first().cloned()
        {
            self.queue.remove(0);
            self.banner = Some(next);
        }
    }

    pub fn outcome_banner(&mut self, outcome: Outcome, score: i32) {
        let (text, color) = match outcome {
            Outcome::Escaped => ("ESCAPED", pal::HUD_GOOD),
            Outcome::PodBeamed => ("PAYLOAD DELIVERED", pal::POD),
            Outcome::ReactorDestroyed => ("REACTOR DESTROYED", pal::REACTOR),
        };
        self.show_banner(text, &format!("{score} POINTS"), color, 3.5);
    }

    /// Hint text for the control scheme, shown while the player is learning.
    ///
    /// Two weapons and a base ritual are a lot to drop on someone with no
    /// manual, so the hint names the special trigger and where it can be
    /// changed. It stays one line: the HUD draws it in a panel at the bottom.
    pub fn scheme_hint(scheme: Scheme) -> &'static str {
        match scheme {
            Scheme::Classic => "A/D TURN  W THRUST  SPACE GUN  F SPECIAL  E BEAM  X DUMP",
            Scheme::Modern => "WASD FLY  MOUSE AIM  LMB GUN  F SPECIAL  RMB BEAM  X DUMP",
        }
    }
}
