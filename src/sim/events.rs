//! Simulation events.
//!
//! The simulation itself is purely numeric: it emits events and the renderer,
//! audio engine and HUD decide what they look and sound like. Particles, screen
//! shake and sound therefore never feed back into the physics, which keeps
//! replays bit-exact.

use crate::math::V2;
use crate::sim::weapons::{CloudKind, GadgetKind, WeaponId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Continuous thrust plume while the engine is burning.
    Thrust {
        p: V2,
        angle: f32,
        power: f32,
    },
    /// A ship's engine puffing in a medium (bubbles in water).
    Bubble {
        p: V2,
    },
    Bullet {
        p: V2,
        angle: f32,
        from_player: bool,
        weapon: WeaponId,
    },
    /// Bullet hitting wall geometry.
    Impact {
        p: V2,
        normal: V2,
    },
    /// Terrain removed by a bullet or a blast: dust of the right colour, and the
    /// thud of the cave coming apart.
    Carve {
        p: V2,
        radius: f32,
        material: u8,
    },
    /// A body dropping into water.
    Splash {
        p: V2,
        power: f32,
    },
    Explosion {
        p: V2,
        power: f32,
    },
    ShieldHit {
        p: V2,
        angle: f32,
    },
    /// The special weapon fired: the renderer picks the muzzle look and the
    /// audio layer the voice from the weapon.
    Special {
        weapon: WeaponId,
        p: V2,
        angle: f32,
    },
    /// A shot detonated: blast radius in px.
    Blast {
        p: V2,
        radius: f32,
        weapon: WeaponId,
    },
    /// Dirt added to the cave (`Dirtball`), the only weapon that builds.
    Fill {
        p: V2,
        radius: f32,
    },
    /// A cloud appeared: poison, gas, flame, a water jet, sparks.
    Cloud {
        p: V2,
        radius: f32,
        kind: CloudKind,
    },
    /// A gadget was laid: a charge, a gravity well, a pair of troopers.
    Gadget {
        p: V2,
        kind: GadgetKind,
    },
    /// A target froze solid.
    Freeze {
        p: V2,
    },
    /// A drone was tangled in a net.
    NetHit {
        p: V2,
    },
    /// An electric blast knocked out everything around the ship.
    Emp {
        p: V2,
        radius: f32,
    },
    /// The teleporter moved the ship.
    Blink {
        from: V2,
        to: V2,
    },
    /// The harpoon bit into the rock (`on`), or let go.
    Tether {
        p: V2,
        on: bool,
    },
    /// A granular wall took hold of the ship (`on`), or let it go. While held,
    /// the ship can still turn and shoot, and shooting the wall frees it.
    Grab {
        p: V2,
        on: bool,
    },
    /// The pilot dumped the special weapon's magazine to fly lighter.
    Jettison {
        p: V2,
    },
    Pickup {
        p: V2,
        fuel: bool,
    },
    Refuel {
        p: V2,
        amount: f32,
    },
    PadLanding {
        p: V2,
    },
    Beam {
        on: bool,
    },
    /// A payload settling onto rock: dust and a thud, no destruction.
    Landing {
        p: V2,
        speed: f32,
    },
    PodAttached {
        p: V2,
    },
    PodLost {
        p: V2,
    },
    ReactorHit {
        p: V2,
    },
    ReactorCritical {
        p: V2,
    },
    ReactorDestroyed {
        p: V2,
    },
    TurretDestroyed {
        p: V2,
    },
    DroneDestroyed {
        p: V2,
    },
    MineBlast {
        p: V2,
    },
    ShipLost {
        p: V2,
    },
    /// Klaxon pulse while the escape timer runs.
    AlarmPulse {
        p: V2,
    },
    ExitOpen {
        p: V2,
    },
    LevelComplete {
        p: V2,
    },
}

/// Where the ship ended up. Used for scoring and the results card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Left the cave without doing anything optional.
    Escaped,
    /// Left the cave with the payload.
    PodBeamed,
    /// Blasted the reactor and escaped the collapse.
    ReactorDestroyed,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Escaped => "ESCAPED",
            Outcome::PodBeamed => "PAYLOAD DELIVERED",
            Outcome::ReactorDestroyed => "REACTOR DESTROYED",
        }
    }
}

/// Why a run ended in failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Ship hit terrain, a projectile, a drone or a blast.
    ShipLost,
    /// The required payload was destroyed.
    PayloadLost,
    /// The reactor's collapse timer ran out.
    CaveCollapsed,
}

impl Failure {
    pub fn label(self) -> &'static str {
        match self {
            Failure::ShipLost => "SHIP LOST",
            Failure::PayloadLost => "PAYLOAD LOST",
            Failure::CaveCollapsed => "CAVE COLLAPSED",
        }
    }
}
