//! Camera: free scrolling over the cave, with a velocity lead and screen shake.
//!
//! Screen shake and lead are cosmetic, but they are deterministic functions of
//! the tick so replays and screenshots stay reproducible.

use crate::math::{Rect, V2, clampf, lerp};
use crate::sim::tuning::{CAMERA_LEAD, DT};

pub struct Camera {
    /// Top-left corner of the view in world space.
    pub pos: V2,
    /// Viewport size in internal pixels.
    pub size: V2,
    /// Extra offset applied while shaking.
    pub shake: V2,
    shake_amp: f32,
    shake_phase: u32,
}

impl Camera {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            pos: V2::ZERO,
            size: V2::new(width, height),
            shake: V2::ZERO,
            shake_amp: 0.0,
            shake_phase: 0,
        }
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.size = V2::new(width, height);
    }

    /// World-space rectangle currently visible.
    pub fn view(&self) -> Rect {
        Rect::new(self.pos.x, self.pos.y, self.size.x, self.size.y)
    }

    /// Snaps the camera so `target` is centred, clamped to the level bounds.
    pub fn snap(&mut self, target: V2, bounds: Rect) {
        self.pos = clamp_to_bounds(target - self.size * 0.5, self.size, bounds);
    }

    /// Smooth follow with a lead proportional to velocity, clamped to bounds.
    pub fn follow(&mut self, target: V2, velocity: V2, bounds: Rect, rate: f32) {
        let goal = clamp_to_bounds(
            target + velocity * CAMERA_LEAD - self.size * 0.5,
            self.size,
            bounds,
        );
        let t = 1.0 - (-rate * DT).exp();
        self.pos = V2::new(lerp(self.pos.x, goal.x, t), lerp(self.pos.y, goal.y, t));
    }

    pub fn kick(&mut self, amount: f32) {
        self.shake_amp = (self.shake_amp + amount).min(26.0);
    }

    /// Decays the shake and evaluates its offset.
    pub fn tick(&mut self) {
        if self.shake_amp <= 0.01 {
            self.shake_amp = 0.0;
            self.shake = V2::ZERO;
            return;
        }
        self.shake_amp *= 0.90;
        self.shake_phase = self.shake_phase.wrapping_add(1);
        let t = self.shake_phase as f32;
        self.shake = V2::new(
            (t * 12.9898).sin() * self.shake_amp,
            (t * 7.233).cos() * self.shake_amp * 0.7,
        );
    }

    pub fn offset(&self) -> V2 {
        self.pos - self.shake
    }

    pub fn world_to_screen(&self, p: V2) -> (f32, f32) {
        let o = self.offset();
        (p.x - o.x, p.y - o.y)
    }

    pub fn screen_to_world(&self, x: f32, y: f32) -> V2 {
        let o = self.offset();
        V2::new(x + o.x, y + o.y)
    }

    /// True when the point (inflated by `margin`) can be drawn this frame.
    pub fn visible(&self, p: V2, margin: f32) -> bool {
        self.view().inflate(margin).contains(p)
    }
}

fn clamp_to_bounds(pos: V2, size: V2, bounds: Rect) -> V2 {
    let x = if bounds.w <= size.x {
        bounds.x + (bounds.w - size.x) * 0.5
    } else {
        clampf(pos.x, bounds.x, bounds.right() - size.x)
    };
    let y = if bounds.h <= size.y {
        bounds.y + (bounds.h - size.y) * 0.5
    } else {
        clampf(pos.y, bounds.y, bounds.bottom() - size.y)
    };
    V2::new(x, y)
}
