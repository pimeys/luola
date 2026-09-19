//! The ship: attitude, thrust budget and the shield state machine.

use crate::math::{V2, angle_diff};
use crate::sim::body::Body;
use crate::sim::tuning;
use crate::sim::weapons::{self, Loadout, WeaponId};

/// Ship is fragile: one lethal contact, with a shield that eats exactly one hit
/// and then spends the timings documented in `docs/game_mechanics.md` §4.3.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shield {
    /// Up and absorbing.
    Charged,
    /// Just ate a hit: still absorbing for `SHIELD_ABSORB`, then it goes down.
    Absorbing {
        t: f32,
    },
    Down {
        t: f32,
    },
    /// Visibly recharging, but already protecting again.
    Recharging {
        t: f32,
    },
}

impl Shield {
    pub fn absorbing(&self) -> bool {
        matches!(
            self,
            Shield::Charged | Shield::Absorbing { .. } | Shield::Recharging { .. }
        )
    }

    /// Seconds until the shield is charged again, or 0 when it already is.
    pub fn time_to_charge(&self) -> f32 {
        match self {
            Shield::Charged => 0.0,
            Shield::Absorbing { t } => (tuning::SHIELD_ABSORB - t).max(0.0) + tuning::SHIELD_DOWN,
            Shield::Down { t } => (tuning::SHIELD_DOWN - t).max(0.0) + tuning::SHIELD_RECHARGE,
            Shield::Recharging { t } => (tuning::SHIELD_RECHARGE - t).max(0.0),
        }
    }

    /// Charge fraction for the HUD ring: 1 is fully up.
    pub fn charge(&self) -> f32 {
        1.0 - (self.time_to_charge() / (tuning::SHIELD_DOWN + tuning::SHIELD_RECHARGE)).min(1.0)
    }

    /// Advances the shield clock. Public because the timings are a contract
    /// the HUD and the tests both depend on.
    pub fn advance(&mut self, dt: f32, has_fuel: bool) {
        if !has_fuel {
            // The shield deactivates with a dry tank; it recovers when refuelled.
            *self = Shield::Down { t: 0.0 };
            return;
        }
        *self = match *self {
            Shield::Charged => Shield::Charged,
            Shield::Absorbing { t } if t + dt >= tuning::SHIELD_ABSORB => Shield::Down { t: 0.0 },
            Shield::Absorbing { t } => Shield::Absorbing { t: t + dt },
            Shield::Down { t } if t + dt >= tuning::SHIELD_DOWN => Shield::Recharging { t: 0.0 },
            Shield::Down { t } => Shield::Down { t: t + dt },
            Shield::Recharging { t } if t + dt >= tuning::SHIELD_RECHARGE => Shield::Charged,
            Shield::Recharging { t } => Shield::Recharging { t: t + dt },
        };
    }

    /// Eats a hit if the shield can. Returns true when the ship survives it.
    pub fn absorb(&mut self, has_fuel: bool) -> bool {
        if !has_fuel {
            return false;
        }
        match *self {
            Shield::Charged => {
                *self = Shield::Absorbing { t: 0.0 };
                true
            }
            // The absorb window is what makes rapid multi-hits survivable.
            Shield::Absorbing { t } => t < tuning::SHIELD_ABSORB,
            Shield::Down { .. } => false,
            Shield::Recharging { .. } => true,
        }
    }

    pub fn repair(&mut self) {
        *self = Shield::Charged;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ship {
    pub body: Body,
    /// Pose at the start of the tick; collision sweeps from here to `body`.
    pub prev: Body,
    pub fuel: f32,
    pub fuel_capacity: f32,
    pub shield: Shield,
    pub fire_cd: f32,
    pub alive: bool,
    /// Direction thrust was applied this tick; zero when the engine is cold.
    pub thrust_dir: V2,
    pub thrusting: bool,
    /// Index of the pod currently on the beam.
    pub attached: Option<usize>,
    /// The special weapon and its ammo: the half of the loadout a base changes.
    pub loadout: Loadout,
    /// Seconds left of the `Shield` weapon's bubble, which keeps the shield
    /// charged while it lasts.
    pub shield_field: f32,
    /// Where the harpoon bit into the rock, and how long it holds.
    pub tether: Option<(V2, f32)>,
    /// True while a granular wall holds the ship. The ship can still turn and
    /// fire; shooting the wall around it is how it gets out.
    pub grabbed: bool,
}

impl Ship {
    pub fn new(p: V2, angle: f32, fuel: f32) -> Self {
        let body = Body::new(p, angle);
        Self {
            body,
            prev: body,
            fuel,
            fuel_capacity: fuel,
            shield: Shield::Charged,
            fire_cd: 0.0,
            alive: true,
            thrust_dir: V2::ZERO,
            thrusting: false,
            attached: None,
            loadout: Loadout::new(WeaponId::Gun),
            shield_field: 0.0,
            tether: None,
            grabbed: false,
        }
    }

    /// Local hull vertices in polygon order around the V: nose, left wingtip,
    /// tail notch, right wingtip.
    pub const HULL: [V2; 4] = [
        tuning::HULL_APEX,
        tuning::HULL_WING_L,
        tuning::HULL_TAIL,
        tuning::HULL_WING_R,
    ];

    pub fn hull_at(body: &Body) -> [V2; 4] {
        [
            body.to_world(Self::HULL[0]),
            body.to_world(Self::HULL[1]),
            body.to_world(Self::HULL[2]),
            body.to_world(Self::HULL[3]),
        ]
    }

    pub fn hull(&self) -> [V2; 4] {
        Self::hull_at(&self.body)
    }

    pub fn prev_hull(&self) -> [V2; 4] {
        Self::hull_at(&self.prev)
    }

    pub fn has_fuel(&self) -> bool {
        self.fuel > 0.0
    }

    /// Thrust acceleration available this tick, zero with a dry tank.
    ///
    /// Scaled by [`Ship::agility`]: the reference load (full tank, full
    /// magazine) flies at the tuned `T = 2g`, and a ship that has burnt or
    /// dumped its load gets quicker up to the cap.
    pub fn thrust_available(&self, gravity: f32) -> f32 {
        if self.has_fuel() {
            gravity * tuning::THRUST_RATIO * self.agility()
        } else {
            0.0
        }
    }

    /// Mass of the ship and everything it is carrying: the hull, its fuel and
    /// its special magazine.
    pub fn mass(&self) -> f32 {
        tuning::SHIP_MASS
            + self.fuel * tuning::FUEL_MASS
            + self.loadout.ammo as f32 * tuning::AMMO_MASS
    }

    /// The mass the thrust model treats as the reference: a full tank and a
    /// full magazine, which is what a level's fuel budget was tuned around.
    pub fn mass_ref(&self) -> f32 {
        tuning::SHIP_MASS
            + self.fuel_capacity * tuning::FUEL_MASS
            + weapons::spec(self.loadout.special).ammo as f32 * tuning::AMMO_MASS
    }

    /// Handling multiplier from carried mass: exactly 1.0 at launch load, rising
    /// as the ship lightens, capped by `MASS_AGILITY_MAX`.
    pub fn agility(&self) -> f32 {
        (self.mass_ref() / self.mass().max(1e-3)).clamp(1.0, tuning::MASS_AGILITY_MAX)
    }

    pub fn burn(&mut self, dt: f32) {
        self.fuel = (self.fuel - tuning::FUEL_BURN * dt).max(0.0);
    }

    pub fn refuel(&mut self, amount: f32) -> f32 {
        let before = self.fuel;
        self.fuel = (self.fuel + amount).min(self.fuel_capacity);
        self.fuel - before
    }

    pub fn refuel_full(&mut self) -> f32 {
        let before = self.fuel;
        self.fuel = self.fuel_capacity;
        self.fuel - before
    }

    pub fn tick_timers(&mut self, dt: f32) {
        self.fire_cd = (self.fire_cd - dt).max(0.0);
        self.loadout.tick(dt);
        self.shield_field = (self.shield_field - dt).max(0.0);
        if let Some((_, t)) = self.tether.as_mut() {
            *t -= dt;
            if *t <= 0.0 {
                self.tether = None;
            }
        }
        // The shield weapon is a bubble that keeps topping the shield up, which
        // is exactly what makes it worth a shot against a wall of turrets.
        if self.shield_field > 0.0 {
            self.shield.repair();
        }
        self.shield
            .advance(dt, self.has_fuel() || !tuning::SHIELD_NEEDS_FUEL);
    }

    pub fn can_fire(&self) -> bool {
        self.fire_cd <= 0.0 && self.alive
    }

    /// Turns the hull towards `target` at `rate`, used by the modern scheme.
    pub fn turn_towards(&mut self, target: f32, rate: f32, dt: f32) {
        let step = rate * dt;
        let d = angle_diff(self.body.angle, target);
        self.body.angle = crate::math::wrap_angle(self.body.angle + d.clamp(-step, step));
    }

    /// Classic scheme: hold rotate keys to spin, thrust along the hull.
    pub fn set_spin(&mut self, direction: f32) {
        self.body.omega = direction * tuning::ROT_RATE;
    }
}
