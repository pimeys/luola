//! Rigid body: one integrator for ship, payload, drones, mines and debris.

use crate::math::{V2, wrap_angle};

/// Semi-implicit Euler state, matching `docs/game_mechanics.md` §4.1.
///
/// There is deliberately no drag and no speed clamp anywhere in this type.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Body {
    pub p: V2,
    pub v: V2,
    pub angle: f32,
    pub omega: f32,
}

impl Body {
    pub fn new(p: V2, angle: f32) -> Self {
        Self {
            p,
            v: V2::ZERO,
            angle,
            omega: 0.0,
        }
    }

    /// `v += a*dt; p += v*dt; angle += omega*dt`.
    pub fn integrate(&mut self, accel: V2, dt: f32) {
        self.v += accel * dt;
        self.p += self.v * dt;
        self.angle = wrap_angle(self.angle + self.omega * dt);
    }

    /// Local offset rotated into world space.
    pub fn to_world(&self, local: V2) -> V2 {
        self.p + local.rotated(self.angle)
    }

    pub fn forward(&self) -> V2 {
        V2::from_angle(self.angle)
    }
}
