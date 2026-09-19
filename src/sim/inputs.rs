//! Virtual input actions.
//!
//! Game code never reads keys directly: a scheme (classic / modern) maps devices
//! to these actions, and the simulation only ever sees an [`InputFrame`]. That
//! keeps replays exact and makes control schemes a binding table rather than a
//! branch inside the physics (see `docs/game_mechanics.md` §12).

use crate::math::V2;

pub const BTN_ROTATE_CCW: u16 = 1 << 0;
pub const BTN_ROTATE_CW: u16 = 1 << 1;
pub const BTN_THRUST: u16 = 1 << 2;
pub const BTN_FIRE: u16 = 1 << 3;
pub const BTN_BEAM: u16 = 1 << 4;
/// Fires the ship's special weapon (Wings' selectable one).
pub const BTN_SPECIAL: u16 = 1 << 5;
/// Steps the special weapon up or down the level's list. Wings does this with
/// the turn buttons while docked at a base, and so do we.
pub const BTN_WEAPON_PREV: u16 = 1 << 6;
pub const BTN_WEAPON_NEXT: u16 = 1 << 7;
/// Jettison the special weapon's magazine to fly lighter (AUTS/Turboraketti's
/// stripped-for-speed choice). The gun is never dumped.
pub const BTN_DUMP: u16 = 1 << 8;

/// Which control scheme produced a frame. Stored in replays because classic and
/// modern frames are interpreted differently by the ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Classic,
    Modern,
}

impl Scheme {
    pub fn name(self) -> &'static str {
        match self {
            Scheme::Classic => "classic",
            Scheme::Modern => "modern",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "classic" => Some(Scheme::Classic),
            "modern" => Some(Scheme::Modern),
            _ => None,
        }
    }
}

/// One simulation tick of player intent.
///
/// `move_dir` and `aim` only carry meaning for the modern scheme; `buttons`
/// carries meaning for both.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct InputFrame {
    pub buttons: u16,
    pub move_dir: V2,
    pub aim: f32,
}

impl InputFrame {
    pub fn has(&self, button: u16) -> bool {
        self.buttons & button != 0
    }

    pub fn set(&mut self, button: u16, on: bool) {
        if on {
            self.buttons |= button;
        } else {
            self.buttons &= !button;
        }
    }

    pub fn with(mut self, button: u16, on: bool) -> Self {
        self.set(button, on);
        self
    }
}
