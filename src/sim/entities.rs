//! Entities sharing the same integrator: turrets, drones, mines, reactors,
//! payload pods, fuel pods, pads, exits and projectiles.

use crate::math::{Rect, V2};
use crate::sim::body::Body;
use crate::sim::tuning;
use crate::sim::weapons::{self, CloudKind, Effect, GadgetKind, WeaponId};

#[derive(Clone, Copy, Debug)]
pub struct Turret {
    pub p: V2,
    pub base_angle: f32,
    pub aim: f32,
    pub cooldown: f32,
    pub hp: i32,
    pub alive: bool,
    /// Reactor destruction cuts power to every turret in the level.
    pub powered: bool,
    /// Seconds left before an electric blast's EMP wears off.
    pub emp: f32,
    /// Seconds left frozen by a `Freezer` bolt; a frozen turret cannot fire.
    pub frozen: f32,
}

impl Turret {
    pub fn new(p: V2, angle: f32) -> Self {
        Self {
            p,
            base_angle: angle,
            aim: angle,
            cooldown: 0.0,
            hp: tuning::TURRET_HP,
            alive: true,
            powered: true,
            emp: 0.0,
            frozen: 0.0,
        }
    }

    /// A turret shoots only when the reactor is up, it is not EMPed and it is
    /// not frozen.
    pub fn operational(&self) -> bool {
        self.alive && self.powered && self.emp <= 0.0 && self.frozen <= 0.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Drone {
    pub body: Body,
    pub prev: V2,
    pub hp: i32,
    pub alive: bool,
    /// Seconds since spawn; drives the pulsed thrust cycle.
    pub age: f32,
    pub thrusting: bool,
    /// Seconds left frozen by a `Freezer` bolt.
    pub frozen: f32,
    /// Seconds left tangled in a `Net`.
    pub netted: f32,
}

impl Drone {
    pub fn new(p: V2, angle: f32) -> Self {
        Self {
            body: Body::new(p, angle),
            prev: p,
            hp: tuning::DRONE_HP,
            alive: true,
            age: 0.0,
            thrusting: false,
            frozen: 0.0,
            netted: 0.0,
        }
    }

    /// A frozen or netted drone is a rock: no thrust, no steering.
    pub fn mobile(&self) -> bool {
        self.frozen <= 0.0 && self.netted <= 0.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Mine {
    pub body: Body,
    pub prev: V2,
    pub alive: bool,
    /// Mines arm shortly after spawn so they do not detonate on the author.
    pub armed: bool,
    /// True once the mine has settled on terrain.
    pub resting: bool,
    /// Seconds since spawn; mines arm shortly after being placed.
    pub age: f32,
    /// True for a mine the player laid (`Mine`, `Landmines`) rather than one the
    /// level authored. A friendly mine ignores the ship and hunts hostiles.
    pub from_player: bool,
    /// Seconds until a laid mine gives up, so a level cannot fill with them.
    pub ttl: f32,
    /// Blast radius when it goes off.
    pub blast: f32,
}

impl Mine {
    pub fn new(p: V2) -> Self {
        Self::hostile(p)
    }

    /// A mine authored into the level: it hunts the ship and never expires.
    pub fn hostile(p: V2) -> Self {
        Self {
            body: Body::new(p, 0.0),
            prev: p,
            alive: true,
            armed: false,
            resting: false,
            age: 0.0,
            from_player: false,
            ttl: f32::INFINITY,
            blast: tuning::MINE_BLAST_RADIUS,
        }
    }

    /// A mine the player laid: it ignores the ship and expires.
    pub fn laid(p: V2, ttl: f32, blast: f32) -> Self {
        Self {
            from_player: true,
            ttl,
            blast,
            ..Self::hostile(p)
        }
    }

    pub fn expired(&self) -> bool {
        self.ttl <= 0.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Reactor {
    pub p: V2,
    pub hp: i32,
    pub destroyed: bool,
    /// Seconds left in the collapse countdown once `destroyed`.
    pub critical: f32,
    /// Phase used for the pulsing core.
    pub phase: f32,
}

impl Reactor {
    pub fn new(p: V2) -> Self {
        Self {
            p,
            hp: tuning::REACTOR_HP,
            destroyed: false,
            critical: 0.0,
            phase: 0.0,
        }
    }
}

/// The mission payload: a rigid body on a rod while beamed.
#[derive(Clone, Copy, Debug)]
pub struct Pod {
    pub body: Body,
    pub prev: Body,
    pub alive: bool,
    pub attached: bool,
    /// Rod length fixed when the beam captured it.
    pub rod_len: f32,
    pub delivered: bool,
}

impl Pod {
    pub fn new(p: V2, angle: f32) -> Self {
        Self {
            body: Body::new(p, angle),
            prev: Body::new(p, angle),
            alive: true,
            attached: false,
            rod_len: 0.0,
            delivered: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FuelPod {
    pub p: V2,
    pub amount: f32,
    pub taken: bool,
    pub phase: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Pad {
    pub rect: Rect,
}

/// A gate or crusher: a solid slab that slides between two points.
///
/// A gate is the level's machinery: a door that opens, a piston that closes, a
/// crusher that kills what it catches. It is not part of the terrain grid, so
/// it can never be dug through — the only way past it is to time it or to shut
/// it down. An inactive (hidden or not-yet-triggered) gate is neither drawn nor
/// solid; a signal is what brings it to life (`docs/design.md` §7 signals).
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    /// Travel endpoints, as centres.
    pub from: V2,
    pub to: V2,
    pub size: V2,
    /// Slab speed along the path, px/s.
    pub speed: f32,
    /// Progress along the path, 0 at `from`, 1 at `to`.
    pub t: f32,
    /// Travel direction: +1 towards `to`, -1 back towards `from`.
    pub dir: f32,
    /// What brings it to life.
    pub trigger: crate::sim::level::Trigger,
    /// A hidden gate is not drawn until it activates.
    pub hidden: bool,
    /// An inactive gate is neither drawn nor solid.
    pub active: bool,
    /// Cached world rectangle, updated whenever it moves.
    pub rect: Rect,
}

impl Gate {
    pub fn new(
        from: V2,
        to: V2,
        size: V2,
        speed: f32,
        trigger: crate::sim::level::Trigger,
        hidden: bool,
        phase: f32,
    ) -> Self {
        let active = trigger == crate::sim::level::Trigger::Always && !hidden;
        let t = phase.clamp(0.0, 1.0);
        let mut gate = Self {
            from,
            to,
            size,
            speed: speed.max(0.0),
            t,
            dir: 1.0,
            trigger,
            hidden,
            active,
            rect: Rect::default(),
        };
        gate.update();
        gate
    }

    fn center(&self) -> V2 {
        self.from + (self.to - self.from) * self.t
    }

    fn update(&mut self) {
        self.rect = Rect::centered(self.center(), self.size.x, self.size.y);
    }

    /// Turns the gate on. A signal, or the reactor's default, is what calls it.
    pub fn activate(&mut self) {
        self.active = true;
    }

    /// Advances one tick of travel, ping-ponging between its endpoints.
    pub fn step(&mut self, dt: f32) {
        if !self.active {
            return;
        }
        let path = (self.to - self.from).len();
        if path <= 1e-3 || self.speed <= 0.0 {
            return;
        }
        self.t += self.dir * self.speed * dt / path;
        if self.t >= 1.0 {
            self.t = 1.0;
            self.dir = -1.0;
        } else if self.t <= 0.0 {
            self.t = 0.0;
            self.dir = 1.0;
        }
        self.update();
    }

    /// Squared distance from the gate's rectangle to `p`, zero inside.
    pub fn dist_sq(&self, p: V2) -> f32 {
        let dx = (self.rect.left() - p.x)
            .max(p.x - self.rect.right())
            .max(0.0);
        let dy = (self.rect.top() - p.y)
            .max(p.y - self.rect.bottom())
            .max(0.0);
        dx * dx + dy * dy
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Exit {
    pub rect: Rect,
    pub open: bool,
    /// Hidden exits are only drawn once unlocked (§7 signals).
    pub hidden: bool,
    pub phase: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Bullet {
    pub p: V2,
    pub prev: V2,
    pub v: V2,
    pub life: f32,
    pub from_player: bool,
    /// Which weapon fired it: the spec drives its behaviour and its look.
    pub weapon: WeaponId,
    /// Copied out of the spec at spawn, so the hot loop never chases the table.
    pub damage: i32,
    pub carve: f32,
    pub fill: f32,
    pub radius: f32,
    pub blast: f32,
    pub burst: u8,
    pub pierce: u8,
    pub bounce: u8,
    pub homing: f32,
    pub gravity: f32,
    pub cloud: Option<CloudKind>,
    pub cloud_ttl: f32,
    pub push: f32,
    pub effect: Effect,
    /// Leftover time before a bolt may home again, so a swarm does not jitter.
    pub age: f32,
}

impl Bullet {
    pub fn angle(&self) -> f32 {
        self.v.angle()
    }

    /// A plain shot of `weapon` from `p` along `dir`, with the launcher's own
    /// velocity added: aiming under inertia stays a skill.
    pub fn fire(weapon: WeaponId, p: V2, dir: f32, inherit: V2, from_player: bool) -> Self {
        let s = weapons::spec(weapon);
        let v = V2::from_angle(dir) * s.speed
            + if tuning::BULLET_INHERIT_VELOCITY {
                inherit
            } else {
                V2::ZERO
            };
        Self {
            p,
            prev: p,
            v,
            life: s.life,
            from_player,
            weapon,
            damage: s.damage,
            carve: s.carve,
            fill: s.fill,
            radius: s.radius,
            blast: s.blast,
            burst: s.burst,
            pierce: s.pierce,
            bounce: s.bounce,
            homing: s.homing,
            gravity: s.gravity,
            cloud: s.cloud,
            cloud_ttl: s.cloud_ttl,
            push: s.push,
            effect: s.effect,
            age: 0.0,
        }
    }

    /// A fragment: the same weapon, weaker, without a blast of its own, so a
    /// cluster cannot recurse into another cluster.
    pub fn fragment(parent: &Bullet, p: V2, dir: f32) -> Self {
        Self {
            p,
            prev: p,
            v: V2::from_angle(dir) * (parent.v.len() * 0.7).max(120.0),
            life: parent.life.min(0.5) + 0.15,
            blast: 0.0,
            burst: 0,
            carve: parent.carve * 0.4,
            damage: (parent.damage / 2).max(1),
            homing: 0.0,
            ..*parent
        }
    }

    pub fn is_shell(&self) -> bool {
        matches!(weapons::spec(self.weapon).kind, weapons::Kind::Shell)
    }
}

/// A lingering body in the air: poison, gas, flame, a water jet, sparks.
///
/// Clouds are what makes the area weapons worth their ammo: the shot is only
/// the delivery, the cloud is the weapon.
#[derive(Clone, Copy, Debug)]
pub struct Cloud {
    pub p: V2,
    pub v: V2,
    pub kind: CloudKind,
    pub radius: f32,
    pub ttl: f32,
    pub age: f32,
    /// Hit points per second dealt to each hostile inside.
    pub dps: f32,
    /// Damage accumulated since the last whole hit point was applied.
    pub acc: f32,
    pub push: f32,
    pub from_player: bool,
    /// The weapon that made it, for the renderer and the checksum.
    pub weapon: WeaponId,
}

/// Something laid in the world that lives its own life: a charge, a gravity
/// well, a squad of troopers.
#[derive(Clone, Copy, Debug)]
pub struct Gadget {
    pub p: V2,
    pub prev: V2,
    pub v: V2,
    pub kind: GadgetKind,
    pub weapon: WeaponId,
    pub from_player: bool,
    pub age: f32,
    pub ttl: f32,
    pub armed: bool,
    pub resting: bool,
    /// Blast radius for a charge that goes off.
    pub blast: f32,
    /// Reach of the thing: a well's pull radius.
    pub radius: f32,
    /// Shooting timer for troopers.
    pub cd: f32,
}
