//! Cosmetic particles.
//!
//! Particles are a pure render-layer effect: they consume simulation events and
//! never feed anything back. The genre carries almost all of its readability in
//! motion — thrust plumes, trails, sparks, screen shake
//! (`docs/game_mechanics.md` §3) — so this is where the feel actually lands.

use crate::math::{Rng, V2};
use crate::render::camera::Camera;
use crate::render::fb::Framebuffer;
use crate::render::palette as pal;
use crate::sim::terrain::MAT_ROCK;
use crate::sim::tuning::DT;
use crate::sim::water::Water;
use crate::sim::weapons::{self, CloudKind, GadgetKind, WeaponId};

const MAX_PARTICLES: usize = 2200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Fast additive spark that leaves a short trail.
    Spark,
    /// Slow fading puff.
    Smoke,
    /// Tumbling fragment drawn as a short line.
    Debris,
    /// Hollow ring, used underwater and for shield pops.
    Bubble,
    /// Expanding shock ring.
    Ring,
    /// Bright short-lived flash.
    Flash,
}

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub p: V2,
    pub v: V2,
    pub life: f32,
    pub max_life: f32,
    pub size: f32,
    pub color: u32,
    pub kind: Kind,
    pub angle: f32,
    pub spin: f32,
    pub drag: f32,
    pub gravity: f32,
}

pub struct Particles {
    items: Vec<Particle>,
    rng: Rng,
    /// Oldest slot to overwrite once the cap is reached.
    cursor: usize,
}

impl Default for Particles {
    fn default() -> Self {
        Self::new()
    }
}

impl Particles {
    pub fn new() -> Self {
        Self {
            items: Vec::with_capacity(MAX_PARTICLES),
            rng: Rng::new(0x1234_abcd),
            cursor: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.cursor = 0;
    }

    /// Builds a particle from the generator and stores it. Taking the RNG as a
    /// closure argument keeps the borrow checker out of the emitter bodies.
    fn emit(&mut self, build: impl FnOnce(&mut Rng) -> Particle) {
        let p = build(&mut self.rng);
        self.push(p);
    }

    fn push(&mut self, p: Particle) {
        if self.items.len() < MAX_PARTICLES {
            self.items.push(p);
        } else {
            let i = self.cursor % MAX_PARTICLES;
            self.items[i] = p;
            self.cursor = self.cursor.wrapping_add(1);
        }
    }

    // ------------------------------------------------------------ emitters --

    /// Muzzle flash for a shot, tinted by the weapon that fired it.
    pub fn weapon_muzzle(&mut self, p: V2, angle: f32, weapon: WeaponId) {
        let color = pal::weapon_color(weapon);
        let n = if weapons::spec(weapon).count > 1 {
            3
        } else {
            5
        };
        for _ in 0..n {
            let a = angle + self.rng.sym(0.35);
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * rng.range(30.0, 90.0),
                life: rng.range(0.06, 0.14),
                max_life: 0.14,
                size: rng.range(1.0, 2.0),
                color,
                kind: Kind::Flash,
                angle: a,
                spin: 0.0,
                drag: 2.0,
                gravity: 0.0,
            });
        }
    }

    /// A cloud appearing: gas, poison, flame, steam. Slow puffs that spread.
    pub fn cloud(&mut self, p: V2, radius: f32, kind: CloudKind) {
        let color = pal::cloud_color(kind);
        let r = radius.clamp(6.0, 70.0);
        let n = (r * 0.5).clamp(6.0, 26.0) as u32;
        let spark = kind == CloudKind::Sparks;
        for _ in 0..n {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let d = self.rng.range(0.0, r * 0.5);
            self.emit(|rng| Particle {
                p: p + V2::from_angle(a) * d,
                v: V2::from_angle(a) * rng.range(8.0, 40.0)
                    + if spark { V2::new(0.0, -50.0) } else { V2::ZERO },
                life: rng.range(0.25, 0.9) * if spark { 1.4 } else { 1.0 },
                max_life: 0.9,
                size: rng.range(1.5, 3.5),
                color,
                kind: if spark { Kind::Spark } else { Kind::Smoke },
                angle: a,
                spin: rng.sym(2.0),
                drag: if spark { 0.6 } else { 1.8 },
                gravity: if spark { 40.0 } else { -6.0 },
            });
        }
    }

    /// A gadget landing or appearing.
    pub fn gadget(&mut self, p: V2, kind: GadgetKind) {
        let color = match kind {
            GadgetKind::Well => pal::WELL_CORE,
            GadgetKind::Troopers => pal::TROOPER,
            GadgetKind::Charge => pal::SHOT_BLAST,
            _ => pal::MINE,
        };
        for _ in 0..8 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * rng.range(12.0, 46.0),
                life: rng.range(0.15, 0.4),
                max_life: 0.4,
                size: rng.range(1.0, 2.0),
                color,
                kind: Kind::Ring,
                angle: a,
                spin: 0.0,
                drag: 2.4,
                gravity: 0.0,
            });
        }
    }

    /// The electric blast: a ring of sparks thrown out to the blast radius.
    pub fn zap(&mut self, p: V2, radius: f32) {
        let n = 26;
        for i in 0..n {
            let a = i as f32 * std::f32::consts::TAU / n as f32;
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * rng.range(radius * 0.7, radius * 1.6),
                life: rng.range(0.12, 0.3),
                max_life: 0.3,
                size: rng.range(1.0, 2.6),
                color: pal::SHOT_FIELD,
                kind: Kind::Spark,
                angle: a,
                spin: 0.0,
                drag: 1.6,
                gravity: 0.0,
            });
        }
    }

    /// The teleporter: a ghost of the ship left where it was, a flash where it
    /// arrived.
    pub fn blink(&mut self, from: V2, to: V2) {
        for i in 0..10 {
            let t = i as f32 / 10.0;
            self.emit(|rng| Particle {
                p: from + (to - from) * t,
                v: V2::new(rng.sym(24.0), rng.sym(24.0)),
                life: rng.range(0.1, 0.35),
                max_life: 0.35,
                size: rng.range(1.0, 2.4),
                color: pal::SHOT_FIELD,
                kind: Kind::Flash,
                angle: 0.0,
                spin: 0.0,
                drag: 2.0,
                gravity: 0.0,
            });
        }
    }

    /// The harpoon biting in or letting go.
    pub fn tether(&mut self, p: V2) {
        for _ in 0..6 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * rng.range(20.0, 70.0),
                life: rng.range(0.08, 0.2),
                max_life: 0.2,
                size: rng.range(1.0, 2.0),
                color: pal::TETHER,
                kind: Kind::Spark,
                angle: a,
                spin: 0.0,
                drag: 2.0,
                gravity: 0.0,
            });
        }
    }

    pub fn explosion(&mut self, p: V2, power: f32) {
        let power = power.clamp(0.15, 1.5);
        let hot = pal::THRUST_HOT;
        let cool = pal::THRUST_COOL;
        let n = (26.0 * power) as u32;
        for _ in 0..n {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let speed = self.rng.range(40.0, 260.0) * power;
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * speed,
                life: rng.range(0.25, 0.8) * power.max(0.6),
                max_life: 0.8,
                size: rng.range(1.0, 2.4),
                color: if rng.next_f32() < 0.5 { hot } else { cool },
                kind: Kind::Spark,
                angle: a,
                spin: 0.0,
                drag: 1.6,
                gravity: 0.35,
            });
        }
        for _ in 0..(10.0 * power) as u32 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * rng.range(8.0, 60.0),
                life: rng.range(0.6, 1.6),
                max_life: 1.6,
                size: rng.range(6.0, 16.0) * power,
                color: pal::WALL_EDGE,
                kind: Kind::Smoke,
                angle: 0.0,
                spin: 0.0,
                drag: 0.6,
                gravity: -0.05,
            });
        }
        for _ in 0..(14.0 * power) as u32 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * rng.range(60.0, 300.0) * power,
                life: rng.range(0.5, 1.4),
                max_life: 1.4,
                size: rng.range(2.0, 4.5),
                color: pal::SHIP_DIM,
                kind: Kind::Debris,
                angle: a,
                spin: rng.sym(14.0),
                drag: 0.9,
                gravity: 1.0,
            });
        }
        self.push(Particle {
            p,
            v: V2::ZERO,
            life: 0.22,
            max_life: 0.22,
            size: 12.0 * power,
            color: hot,
            kind: Kind::Flash,
            angle: 0.0,
            spin: 0.0,
            drag: 0.0,
            gravity: 0.0,
        });
        self.push(Particle {
            p,
            v: V2::ZERO,
            life: 0.45,
            max_life: 0.45,
            size: 10.0,
            color: cool,
            kind: Kind::Ring,
            angle: 0.0,
            spin: 0.0,
            drag: 0.0,
            gravity: 0.0,
        });
    }

    pub fn thrust(&mut self, p: V2, angle: f32, power: f32) {
        let back = angle + std::f32::consts::PI;
        for _ in 0..2 {
            let spread = self.rng.sym(0.45);
            let a = back + spread;
            self.emit(|rng| Particle {
                p: p + V2::from_angle(a) * rng.range(0.0, 3.0),
                v: V2::from_angle(a) * rng.range(90.0, 220.0) * power,
                life: rng.range(0.10, 0.26),
                max_life: 0.26,
                size: rng.range(1.2, 2.6),
                color: if rng.next_f32() < 0.5 {
                    pal::THRUST_HOT
                } else {
                    pal::THRUST_COOL
                },
                kind: Kind::Spark,
                angle: a,
                spin: 0.0,
                drag: 3.0,
                gravity: 0.1,
            });
        }
    }

    /// Bubbles pushed out of the engine while submerged.
    pub fn bubble(&mut self, p: V2) {
        self.emit(|rng| Particle {
            p,
            v: V2::new(rng.sym(14.0), rng.range(-40.0, -12.0)),
            life: rng.range(0.5, 1.2),
            max_life: 1.2,
            size: rng.range(1.0, 2.8),
            color: pal::LIQUID_EDGE,
            kind: Kind::Bubble,
            angle: 0.0,
            spin: 0.0,
            drag: 0.5,
            gravity: -0.35,
        });
    }

    pub fn impact(&mut self, p: V2, normal: V2) {
        for _ in 0..5 {
            let a = normal.angle() + self.rng.sym(0.9);
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * rng.range(40.0, 150.0),
                life: rng.range(0.1, 0.35),
                max_life: 0.35,
                size: rng.range(0.8, 1.8),
                color: pal::WALL_EDGE_HOT,
                kind: Kind::Spark,
                angle: a,
                spin: 0.0,
                drag: 2.0,
                gravity: 0.2,
            });
        }
    }

    pub fn muzzle(&mut self, p: V2, angle: f32, from_player: bool) {
        let color = if from_player {
            pal::BULLET
        } else {
            pal::BULLET_ENEMY
        };
        for _ in 0..3 {
            let a = angle + self.rng.sym(0.3);
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * rng.range(60.0, 160.0),
                life: rng.range(0.05, 0.16),
                max_life: 0.16,
                size: rng.range(0.8, 1.6),
                color,
                kind: Kind::Spark,
                angle: a,
                spin: 0.0,
                drag: 4.0,
                gravity: 0.0,
            });
        }
    }

    pub fn shield_hit(&mut self, p: V2, angle: f32) {
        self.emit(|_rng| Particle {
            p,
            v: V2::ZERO,
            life: 0.35,
            max_life: 0.35,
            size: 12.0,
            color: pal::SHIELD_HIT,
            kind: Kind::Ring,
            angle: 0.0,
            spin: 0.0,
            drag: 0.0,
            gravity: 0.0,
        });
        for _ in 0..12 {
            let a = angle + self.rng.sym(1.4);
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * rng.range(80.0, 220.0),
                life: rng.range(0.12, 0.4),
                max_life: 0.4,
                size: rng.range(1.0, 2.2),
                color: pal::SHIELD,
                kind: Kind::Spark,
                angle: a,
                spin: 0.0,
                drag: 2.4,
                gravity: 0.0,
            });
        }
    }

    pub fn pickup(&mut self, p: V2, color: u32) {
        for _ in 0..10 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            self.emit(|rng| Particle {
                p,
                v: V2::from_angle(a) * rng.range(30.0, 110.0),
                life: rng.range(0.2, 0.5),
                max_life: 0.5,
                size: rng.range(1.0, 2.0),
                color,
                kind: Kind::Spark,
                angle: a,
                spin: 0.0,
                drag: 2.0,
                gravity: -0.2,
            });
        }
    }

    pub fn trail(&mut self, p: V2, color: u32) {
        self.push(Particle {
            p,
            v: V2::ZERO,
            life: 0.16,
            max_life: 0.16,
            size: 1.4,
            color,
            kind: Kind::Spark,
            angle: 0.0,
            spin: 0.0,
            drag: 0.0,
            gravity: 0.0,
        });
    }

    /// Dust thrown out of a fresh crater: bullets kick a handful, a blast a
    /// cloud. `radius` is the crater's, and each grain leaves the crater centre
    /// outward, so the spray reads as displaced material rather than a puff.
    pub fn carve(&mut self, p: V2, radius: f32, material: u8) {
        let r = radius.clamp(0.0, 40.0);
        let dust = if material == MAT_ROCK {
            pal::ROCK_DUST
        } else if material == crate::sim::terrain::MAT_GRANULAR {
            pal::GRANULAR_DUST
        } else {
            pal::DIRT_DUST
        };
        let n = (3.0 + r * 2.2) as u32;
        for _ in 0..n {
            let dir = V2::from_angle(self.rng.range(0.0, std::f32::consts::TAU));
            let speed = self.rng.range(24.0, 90.0 + r * 8.0);
            self.emit(|rng| Particle {
                // Start out on the rim, so the cloud has a hollow centre.
                p: p + dir * rng.range(0.0, r * 0.9),
                v: dir * speed,
                life: rng.range(0.18, 0.55),
                max_life: 0.55,
                size: rng.range(0.8, 2.0),
                color: dust,
                kind: Kind::Debris,
                angle: dir.angle(),
                spin: rng.sym(16.0),
                drag: 2.2,
                gravity: 1.0,
            });
        }
        if r >= 6.0 {
            for _ in 0..(r * 0.5) as u32 {
                let dir = V2::from_angle(self.rng.range(0.0, std::f32::consts::TAU));
                self.emit(|rng| Particle {
                    p: p + dir * rng.range(0.0, r * 0.7),
                    v: dir * rng.range(10.0, 50.0),
                    life: rng.range(0.5, 1.3),
                    max_life: 1.3,
                    size: rng.range(5.0, 12.0),
                    color: dust,
                    kind: Kind::Smoke,
                    angle: 0.0,
                    spin: 0.0,
                    drag: 0.9,
                    gravity: -0.05,
                });
            }
        }
    }

    /// A body hitting water: droplets, bubbles and one opening ring.
    pub fn splash(&mut self, p: V2, power: f32) {
        let power = power.clamp(0.1, 1.0);
        let n = (6.0 + 20.0 * power) as u32;
        for _ in 0..n {
            let a = self.rng.range(-2.6, -0.5);
            let speed = self.rng.range(30.0, 60.0 + 180.0 * power);
            self.emit(|rng| Particle {
                p: p + V2::new(rng.sym(3.0), 0.0),
                v: V2::from_angle(a) * speed,
                life: rng.range(0.25, 0.7),
                max_life: 0.7,
                size: rng.range(0.8, 2.2),
                color: pal::LIQUID_EDGE,
                kind: Kind::Spark,
                angle: a,
                spin: 0.0,
                drag: 1.4,
                gravity: 1.0,
            });
        }
        for _ in 0..(4.0 + 8.0 * power) as u32 {
            self.emit(|rng| Particle {
                p,
                v: V2::new(rng.sym(26.0), rng.range(-30.0, -8.0)),
                life: rng.range(0.4, 1.0),
                max_life: 1.0,
                size: rng.range(1.0, 2.6),
                color: pal::LIQUID_EDGE,
                kind: Kind::Bubble,
                angle: 0.0,
                spin: 0.0,
                drag: 0.6,
                gravity: -0.3,
            });
        }
        self.push(Particle {
            p,
            v: V2::ZERO,
            life: 0.3,
            max_life: 0.3,
            size: 5.0 + 9.0 * power,
            color: pal::LIQUID_EDGE,
            kind: Kind::Ring,
            angle: 0.0,
            spin: 0.0,
            drag: 0.0,
            gravity: 0.0,
        });
    }

    // -------------------------------------------------------------- update --

    pub fn update(&mut self, water: &Water, global_gravity: f32) {
        let dt = DT;
        let params = water.params();
        self.items.retain_mut(|p| {
            p.life -= dt;
            if p.life <= 0.0 {
                return false;
            }
            let gravity = if p.gravity == 0.0 {
                0.0
            } else {
                global_gravity * p.gravity
            };
            let mut accel = V2::new(0.0, gravity);
            let mut drag = p.drag;
            // One grid lookup instead of the old liquid-polygon test: the density
            // and drag a particle feels scale with how deep in the water it is.
            let depth = water.submerged_at(p.p);
            if depth > 0.0 {
                accel *= crate::math::lerp(1.0, params.density.max(0.05), depth);
                drag += params.drag * depth * 0.5;
            }
            p.v += accel * dt;
            if drag > 0.0 {
                p.v *= 1.0 / (1.0 + drag * dt);
            }
            p.p += p.v * dt;
            p.angle += p.spin * dt;
            true
        });
    }

    // ---------------------------------------------------------------- draw --

    pub fn draw(&self, fb: &mut Framebuffer, cam: &Camera) {
        for p in &self.items {
            let (x, y) = cam.world_to_screen(p.p);
            if x < -32.0
                || y < -32.0
                || x > fb.width() as f32 + 32.0
                || y > fb.height() as f32 + 32.0
            {
                continue;
            }
            let t = (p.life / p.max_life).clamp(0.0, 1.0);
            let (xi, yi) = (x.round() as i32, y.round() as i32);
            match p.kind {
                Kind::Spark => {
                    let tail = cam.world_to_screen(p.p - p.v * 0.018);
                    fb.line_fa((tail.0, tail.1), (x, y), p.color, t.clamp(0.15, 1.0));
                    fb.glow(xi, yi, p.size * 2.4, p.color, t * 0.5);
                }
                Kind::Smoke => {
                    fb.blend(xi, yi, p.color, t * 0.35);
                    fb.circle_fill(x, y, p.size * (1.4 - t * 0.4), p.color);
                }
                Kind::Debris => {
                    let dir = V2::from_angle(p.angle) * p.size;
                    let a = cam.world_to_screen(p.p - dir);
                    let b = cam.world_to_screen(p.p + dir);
                    fb.line_fa(a, b, p.color, t.clamp(0.1, 0.9));
                }
                Kind::Bubble => {
                    fb.circle_stroke(x, y, p.size, p.color);
                }
                Kind::Ring => {
                    let r = p.size + (1.0 - t) * p.size * 2.6;
                    fb.circle_stroke(x, y, r, p.color);
                }
                Kind::Flash => {
                    fb.glow(xi, yi, p.size, p.color, t * 0.9);
                    fb.circle_fill(x, y, p.size * 0.25, p.color);
                }
            }
        }
    }
}
