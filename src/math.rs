//! Small math helpers shared by the deterministic simulation and the renderer.
//!
//! The simulation uses plain `f32` (see `docs/design.md`): fixed timestep, fixed
//! operation order, no drag/clamping in the integrator. Everything here is
//! deterministic for a given binary.

use std::f32::consts::PI;
use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V2 {
    pub x: f32,
    pub y: f32,
}

impl V2 {
    pub const ZERO: V2 = V2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Unit vector pointing along `angle` (screen space, +y is down).
    pub fn from_angle(angle: f32) -> Self {
        Self::new(angle.cos(), angle.sin())
    }

    pub fn from_polar(len: f32, angle: f32) -> Self {
        Self::from_angle(angle) * len
    }

    pub fn len_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    pub fn len(self) -> f32 {
        self.len_sq().sqrt()
    }

    pub fn dist_sq(self, o: V2) -> f32 {
        (self - o).len_sq()
    }

    pub fn dist(self, o: V2) -> f32 {
        (self - o).len()
    }

    /// Normalised vector; `ZERO` for (near-)zero input so callers never get NaN.
    pub fn normalized(self) -> Self {
        let l = self.len();
        if l > 1e-6 { self / l } else { V2::ZERO }
    }

    pub fn with_len(self, len: f32) -> Self {
        self.normalized() * len
    }

    /// Left-hand perpendicular.
    pub fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }

    /// Rotates the vector by `angle`.
    pub fn rotated(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }

    pub fn dot(self, o: V2) -> f32 {
        self.x * o.x + self.y * o.y
    }

    pub fn cross(self, o: V2) -> f32 {
        self.x * o.y - self.y * o.x
    }

    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for V2 {
    type Output = V2;
    fn mul(self, k: f32) -> V2 {
        V2::new(self.x * k, self.y * k)
    }
}

impl Div<f32> for V2 {
    type Output = V2;
    fn div(self, k: f32) -> V2 {
        V2::new(self.x / k, self.y / k)
    }
}

impl Neg for V2 {
    type Output = V2;
    fn neg(self) -> V2 {
        V2::new(-self.x, -self.y)
    }
}

impl AddAssign for V2 {
    fn add_assign(&mut self, o: V2) {
        self.x += o.x;
        self.y += o.y;
    }
}

impl SubAssign for V2 {
    fn sub_assign(&mut self, o: V2) {
        self.x -= o.x;
        self.y -= o.y;
    }
}

impl MulAssign<f32> for V2 {
    fn mul_assign(&mut self, k: f32) {
        self.x *= k;
        self.y *= k;
    }
}

/// Wraps an angle into `(-PI, PI]`.
pub fn wrap_angle(a: f32) -> f32 {
    let mut a = a % (2.0 * PI);
    if a > PI {
        a -= 2.0 * PI;
    } else if a <= -PI {
        a += 2.0 * PI;
    }
    a
}

/// Signed shortest rotation from `from` to `to`, in `(-PI, PI]`.
pub fn angle_diff(from: f32, to: f32) -> f32 {
    wrap_angle(to - from)
}

/// Rotates `cur` towards `target` by at most `max_step` radians.
pub fn approach_angle(cur: f32, target: f32, max_step: f32) -> f32 {
    let d = angle_diff(cur, target);
    cur + d.clamp(-max_step, max_step)
}

pub fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Axis-aligned rectangle in world space; `w`/`h` are always non-negative.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Rectangle described by its centre and size.
    pub fn centered(c: V2, w: f32, h: f32) -> Self {
        Self::new(c.x - w * 0.5, c.y - h * 0.5, w, h)
    }

    pub fn from_corners(a: V2, b: V2) -> Self {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Self::new(x, y, (a.x - b.x).abs(), (a.y - b.y).abs())
    }

    pub fn left(&self) -> f32 {
        self.x
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn top(&self) -> f32 {
        self.y
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn center(&self) -> V2 {
        V2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn contains(&self, p: V2) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.left() <= o.right()
            && o.left() <= self.right()
            && self.top() <= o.bottom()
            && o.top() <= self.bottom()
    }

    /// Grows the rectangle by `m` on every side.
    pub fn inflate(&self, m: f32) -> Rect {
        Rect::new(self.x - m, self.y - m, self.w + 2.0 * m, self.h + 2.0 * m)
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }

    /// True when the segment `a`-`b` touches the rectangle (slab test).
    pub fn segment_hits(&self, a: V2, b: V2) -> bool {
        let d = b - a;
        let mut t0 = 0.0f32;
        let mut t1 = 1.0f32;
        let slabs = [
            (a.x, d.x, self.left(), self.right()),
            (a.y, d.y, self.top(), self.bottom()),
        ];
        for (origin, delta, lo, hi) in slabs {
            if delta.abs() < 1e-9 {
                if origin < lo || origin > hi {
                    return false;
                }
                continue;
            }
            let (mut ta, mut tb) = ((lo - origin) / delta, (hi - origin) / delta);
            if ta > tb {
                std::mem::swap(&mut ta, &mut tb);
            }
            t0 = t0.max(ta);
            t1 = t1.min(tb);
            if t0 > t1 {
                return false;
            }
        }
        true
    }
}

/// Squared distance from `p` to segment `a`-`b`.
pub fn dist_sq_to_segment(p: V2, a: V2, b: V2) -> f32 {
    let ab = b - a;
    let len_sq = ab.len_sq();
    if len_sq < 1e-9 {
        return p.dist_sq(a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    p.dist_sq(a + ab * t)
}

/// Deterministic PCG32 generator. Simulation never uses the OS entropy source.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
    inc: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Self {
            state: 0,
            inc: 0xda3e_39cb_94b9_5bdb,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    pub fn sym(&mut self, amount: f32) -> f32 {
        self.range(-amount, amount)
    }
}
