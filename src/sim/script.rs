//! Deterministic scripted input sequences.
//!
//! Used by `--headless` smoke runs, by the test suite and by `--record` when you
//! want a replay without flying it yourself. Everything here is a pure function
//! of the tick number, so the same script always produces the same run.

use crate::math::V2;
use crate::sim::inputs::{
    BTN_FIRE, BTN_ROTATE_CCW, BTN_ROTATE_CW, BTN_SPECIAL, BTN_THRUST, InputFrame,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    /// No input at all: the ship falls and dies. Used to check failure paths.
    Idle,
    /// Rock the ship and burn fuel in bursts: exercises thrust, rotation, fuel.
    Wobble,
    /// Spin up, burn hard, fire the gun and the special on separate cadences.
    Dervish,
}

impl Script {
    pub fn name(self) -> &'static str {
        match self {
            Script::Idle => "idle",
            Script::Wobble => "wobble",
            Script::Dervish => "dervish",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "idle" => Some(Script::Idle),
            "wobble" => Some(Script::Wobble),
            "dervish" => Some(Script::Dervish),
            _ => None,
        }
    }

    pub fn frame(self, tick: u64) -> InputFrame {
        let t = tick as f32 / crate::sim::tuning::TICK_HZ as f32;
        match self {
            Script::Idle => InputFrame::default(),
            Script::Wobble => {
                let phase = (t * 0.9).sin();
                let mut f = InputFrame::default();
                f.set(BTN_ROTATE_CCW, phase > 0.0);
                f.set(BTN_ROTATE_CW, phase <= 0.0);
                f.set(BTN_THRUST, phase.abs() > 0.55);
                f
            }
            Script::Dervish => {
                let mut f = InputFrame::default();
                f.set(BTN_ROTATE_CCW, (t * 0.35).sin() > 0.0);
                f.set(BTN_ROTATE_CW, (t * 0.35).sin() <= 0.0);
                f.set(BTN_THRUST, (t * 0.7).sin() > -0.2);
                f.set(BTN_FIRE, (t * 3.0).fract() < 0.08);
                // The special fires on its own, slower cadence, so a headless
                // run of any weapon actually uses it.
                f.set(BTN_SPECIAL, (t * 1.3).fract() < 0.05);
                f.move_dir = V2::new((t * 1.3).sin(), (t * 1.7).cos()).normalized();
                f.aim = (t * 0.6).sin() * std::f32::consts::PI;
                f
            }
        }
    }
}
