//! Software synthesis: every luola sound is generated here, sample by sample.
//!
//! There are no sample assets. [`Mixer`] owns one continuous engine voice, one
//! continuous alarm voice and a fixed pool of one-shot voices. All voices add
//! into an interleaved output buffer, then a master stage applies the master
//! volume, a DC blocker and a soft clipper. Nothing here allocates, locks or
//! blocks, so the whole thing is safe to run on the audio callback thread.
//!
//! The same `Mixer` drives playback and offline rendering: [`Mixer::apply`] is
//! exactly what the live callback does with a queued command, so a command
//! sequence rendered to a file is the sequence you hear.

use super::{AudioCmd, SpecialVoice};

/// Simultaneous one-shot effects (shots, hits, explosions, pickups).
const MAX_VOICES: usize = 24;

/// Output trim on the engine noise mix, so full thrust sits under the weapons
/// rather than drowning them.
const ENGINE_GAIN: f32 = 0.35;

const PI: f32 = core::f32::consts::PI;
const TAU: f32 = core::f32::consts::TAU;

// ---------------------------------------------------------------------------
// small helpers
// ---------------------------------------------------------------------------

/// Clamp to `0..=1`, mapping NaN to 0 and infinities to the nearest end.
#[inline]
pub(crate) fn sanitize01(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else if v > 0.0 {
        1.0
    } else {
        0.0
    }
}

/// One-pole smoothing coefficient for time constant `tau` (seconds).
#[inline]
fn smooth_coeff(tau: f32, sr: f32) -> f32 {
    if tau <= 0.0 || !sr.is_finite() || sr <= 0.0 {
        1.0
    } else {
        (1.0 - (-1.0 / (tau * sr)).exp()).clamp(0.0, 1.0)
    }
}

/// Frequency must stay below Nyquist: clamp to a fifth of a half-decade below it.
#[inline]
fn clamp_freq(f: f32, sr: f32) -> f32 {
    let hi = sr * 0.45;
    if !f.is_finite() || f < 0.0 {
        0.0
    } else if f > hi {
        hi
    } else {
        f
    }
}

/// Normalised phase increment for `freq`, clamped below the polyBLEP limit.
#[inline]
fn phase_inc(freq: f32, sr: f32) -> f32 {
    (clamp_freq(freq, sr) / sr).clamp(0.0, 0.49)
}

/// Anti-aliasing correction for the discontinuity of a bandlimited step at `t`.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[inline]
fn saw(phase: f32, dt: f32) -> f32 {
    (2.0 * phase - 1.0) - poly_blep(phase, dt)
}

#[inline]
fn square(phase: f32, dt: f32) -> f32 {
    let s = if phase < 0.5 { 1.0 } else { -1.0 };
    let mut p2 = phase + 0.5;
    if p2 >= 1.0 {
        p2 -= 1.0;
    }
    s + poly_blep(phase, dt) - poly_blep(p2, dt)
}

#[inline]
fn sine(phase: f32) -> f32 {
    (phase * TAU).sin()
}

/// Add a stereo frame into an interleaved buffer of `ch` channels.
#[inline]
fn add_frame(out: &mut [f32], base: usize, l: f32, r: f32, ch: usize) {
    if ch == 1 {
        out[base] += (l + r) * 0.5;
    } else if base + 1 < out.len() {
        out[base] += l;
        out[base + 1] += r;
    }
}

/// Smooth saturator: a Padé approximation of `tanh`, hard-limited as a last
/// resort so the device never sees anything outside `-1..=1`.
#[inline]
fn soft_clip(x: f32) -> f32 {
    let x = if x.is_finite() {
        x.clamp(-3.0, 3.0)
    } else {
        0.0
    };
    let x2 = x * x;
    let y = x * (27.0 + x2) / (27.0 + 9.0 * x2);
    y.clamp(-1.0, 1.0)
}

/// xorshift32: cheap, deterministic, no allocation.
#[derive(Clone, Copy)]
struct Rng(u32);

impl Rng {
    const fn new(seed: u32) -> Self {
        Rng(seed | 1)
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// White noise in `[-1, 1)`.
    #[inline]
    fn noise(&mut self) -> f32 {
        ((self.next_u32() >> 8) as f32) * (1.0 / 8_388_608.0) - 1.0
    }
}

/// DC blocker, one per output channel.
#[derive(Clone, Copy)]
struct Dc {
    x1: f32,
    y1: f32,
}

impl Dc {
    const IDLE: Dc = Dc { x1: 0.0, y1: 0.0 };

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + 0.9995 * self.y1;
        self.x1 = x;
        self.y1 = if y.is_finite() { y } else { 0.0 };
        self.y1
    }
}

/// Topology-preserving-transform state variable low-pass (Zavalishin): stable
/// for any cutoff and any damping, unlike the naive Chamberlin form.
#[derive(Clone, Copy)]
struct Svf {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    z1: f32,
    z2: f32,
}

impl Svf {
    const IDLE: Svf = Svf {
        k: 1.0,
        a1: 1.0,
        a2: 0.0,
        a3: 0.0,
        z1: 0.0,
        z2: 0.0,
    };

    fn new(damping: f32, cutoff: f32, sr: f32) -> Self {
        let mut f = Svf::IDLE;
        f.k = if damping.is_finite() {
            damping.clamp(0.05, 4.0)
        } else {
            1.0
        };
        f.set(cutoff, sr);
        f
    }

    fn set(&mut self, cutoff: f32, sr: f32) {
        let fc = clamp_freq(cutoff, sr).max(5.0);
        let g = (PI * fc / sr).tan();
        if !g.is_finite() {
            self.a1 = 1.0;
            self.a2 = 0.0;
            self.a3 = 0.0;
            return;
        }
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        self.process_lp_bp(x).0
    }

    /// One filter pass returning both the low-pass (`v2`) and band-pass (`v1`)
    /// outputs, so a single run can feed a low band and a high band derived
    /// from the same input without advancing the state twice.
    #[inline]
    fn process_lp_bp(&mut self, x: f32) -> (f32, f32) {
        let v3 = x - self.z2;
        let v1 = self.a1 * self.z1 + self.a2 * v3;
        let v2 = self.z2 + self.a2 * self.z1 + self.a3 * v3;
        self.z1 = 2.0 * v1 - self.z1;
        self.z2 = 2.0 * v2 - self.z2;
        if !(v1.is_finite() && v2.is_finite() && self.z1.is_finite() && self.z2.is_finite()) {
            self.z1 = 0.0;
            self.z2 = 0.0;
            return (0.0, 0.0);
        }
        (v2, v1)
    }
}

/// Short attack ramp so a transient never starts on a step.
#[inline]
fn attack(t: f32, tau: f32) -> f32 {
    if tau <= 0.0 || t >= tau {
        1.0
    } else {
        (t / tau).clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// engine voice
// ---------------------------------------------------------------------------

/// Continuous thruster loop: exhaust noise, not an oscillator. Two independent
/// white-noise streams (one per channel) are split by state-variable filters
/// into the three bands a rocket actually has — a deep combustion rumble, a
/// broad mid roar and the shear-layer hiss above it — each with a level and a
/// cutoff that track `power`, over a sparse high-power crackle. There is no
/// tonal oscillator anywhere, so thrust reads as thrust rather than a synth
/// pad, and the two channels share no noise, so the image is genuinely wide.
#[derive(Clone, Copy)]
struct EngineVoice {
    on: bool,
    gate: f32,
    power: f32,
    target_power: f32,
    att: f32,
    rel: f32,
    pc: f32,
    ctr: u32,
    rng: [Rng; 2],
    rumble: [Svf; 2],
    roar: [Svf; 2],
    hiss: [Svf; 2],
    pre: [Svf; 2],
    pre2: [Svf; 2],
    crack: [f32; 2],
    drift: f32,
}

impl EngineVoice {
    fn new(sr: f32) -> Self {
        EngineVoice {
            on: false,
            gate: 0.0,
            power: 0.0,
            target_power: 0.0,
            att: smooth_coeff(0.030, sr),
            rel: smooth_coeff(0.160, sr),
            pc: smooth_coeff(0.070, sr),
            ctr: 0,
            rng: [Rng::new(0x1234_5679), Rng::new(0x9E37_79C3)],
            rumble: [Svf::new(0.65, 60.0, sr), Svf::new(0.65, 60.0, sr)],
            roar: [Svf::new(0.9, 500.0, sr), Svf::new(0.9, 500.0, sr)],
            hiss: [Svf::new(1.0, 1600.0, sr), Svf::new(1.0, 1600.0, sr)],
            pre: [Svf::new(1.2, 22.0, sr), Svf::new(1.2, 22.0, sr)],
            pre2: [Svf::new(1.2, 22.0, sr), Svf::new(1.2, 22.0, sr)],
            crack: [0.0, 0.0],
            drift: 0.0,
        }
    }

    fn set(&mut self, on: bool, power: f32) {
        self.on = on;
        self.target_power = sanitize01(power);
    }

    fn render(&mut self, out: &mut [f32], ch: usize, sr: f32) {
        if !self.on && self.gate < 1.0e-4 {
            return;
        }
        let frames = out.len() / ch.max(1);
        for i in 0..frames {
            let target = if self.on { 1.0 } else { 0.0 };
            let coeff = if self.on { self.att } else { self.rel };
            self.gate += (target - self.gate) * coeff;
            self.power += (self.target_power - self.power) * self.pc;
            if !(self.gate.is_finite() && self.power.is_finite()) {
                self.gate = 0.0;
                self.power = 0.0;
            }
            let p = self.power;

            // Real exhaust never sits still: re-roll a slow wobble on the band
            // edges about three times a second so the noise breathes instead
            // of sounding like a fixed filter patch.
            if self.ctr & 31 == 0 {
                self.drift = self.rng[0].noise();
                let wob = 1.0 + 0.09 * self.drift;
                for c in 0..2 {
                    let w = if c == 0 { wob } else { 2.0 - wob };
                    self.rumble[c].set((18.0 + 62.0 * p) * w, sr);
                    self.roar[c].set((190.0 + 900.0 * p) * w, sr);
                    self.hiss[c].set((1300.0 + 700.0 * p) * w, sr);
                }
            }
            self.ctr = self.ctr.wrapping_add(1);

            // Band levels: the rumble is always there, the roar and the hiss
            // grow with thrust, which is what makes power audible as well as
            // loud.
            let rumble_amp = 0.55 + 0.75 * p;
            let roar_amp = 0.42 + 0.85 * p;
            let hiss_amp = 0.10 + 0.90 * p;
            let crackle_amp = 0.20 * p * p;

            let mut l = 0.0;
            let mut r = 0.0;
            for c in 0..2 {
                // High-pass the raw noise first: a rocket has no sub-bass DC,
                // and every band below is a low-pass, which would otherwise
                // pass the rumble band's near-DC energy straight through. Two
                // cascaded stages give a 4th-order skirt, so the deep rumble
                // survives while the true DC that the output test bans does
                // not.
                let raw = self.rng[c].noise();
                let (pl, pb) = self.pre[c].process_lp_bp(raw);
                let mut n = raw - self.pre[c].k * pb - pl;
                let (p2l, p2b) = self.pre2[c].process_lp_bp(n);
                n -= self.pre2[c].k * p2b + p2l;

                let (rl, _) = self.rumble[c].process_lp_bp(n);
                let (ml, _) = self.roar[c].process_lp_bp(n);
                let (hl, hb) = self.hiss[c].process_lp_bp(n);
                let hp = n - self.hiss[c].k * hb - hl;

                // Crackle: sparse decaying impulses layered on the high band,
                // the pop of a hard-pushed motor.
                self.crack[c] *= 0.84;
                if p > 0.5 && (self.rng[c].next_u32() & 0x3ff) == 0 {
                    self.crack[c] = 1.0;
                }
                let crackle = self.crack[c] * crackle_amp * n;

                let s =
                    (rl * rumble_amp * 13.0 + ml * roar_amp * 2.0 + hp * hiss_amp * 0.42 + crackle)
                        * ENGINE_GAIN;
                if c == 0 {
                    l = s;
                } else {
                    r = s;
                }
            }
            let amp = self.gate;
            add_frame(out, i * ch, l * amp, r * amp, ch);
        }
    }
}

// ---------------------------------------------------------------------------
// alarm voice
// ---------------------------------------------------------------------------

/// Looping two-tone klaxon. The tone switch glides in pitch rather than
/// stepping, and the amplitude breathes slowly, so the loop has no seam.
#[derive(Clone, Copy)]
struct AlarmVoice {
    on: bool,
    gate: f32,
    freq: f32,
    tone: f32,
    am: f32,
    ph_saw: f32,
    ph_sq: f32,
    filt: Svf,
    att: f32,
    rel: f32,
    glide: f32,
}

impl AlarmVoice {
    fn new(sr: f32) -> Self {
        AlarmVoice {
            on: false,
            gate: 0.0,
            freq: 440.0,
            tone: 0.0,
            am: 0.0,
            ph_saw: 0.0,
            ph_sq: 0.25,
            filt: Svf::new(0.35, 2400.0, sr),
            att: smooth_coeff(0.020, sr),
            rel: smooth_coeff(0.070, sr),
            glide: smooth_coeff(0.030, sr),
        }
    }

    fn set(&mut self, on: bool) {
        self.on = on;
    }

    fn render(&mut self, out: &mut [f32], ch: usize, sr: f32) {
        if !self.on && self.gate < 1.0e-4 {
            return;
        }
        let frames = out.len() / ch.max(1);
        let inc = 1.0 / sr;
        for i in 0..frames {
            let target = if self.on { 1.0 } else { 0.0 };
            let coeff = if self.on { self.att } else { self.rel };
            self.gate += (target - self.gate) * coeff;
            if !self.gate.is_finite() {
                self.gate = 0.0;
            }

            self.tone += 1.4 * inc;
            if self.tone >= 1.0 {
                self.tone -= 1.0;
            }
            let want = if self.tone < 0.5 { 660.0 } else { 440.0 };
            self.freq += (want - self.freq) * self.glide;

            let dt = phase_inc(self.freq, sr);
            self.ph_saw += dt;
            if self.ph_saw >= 1.0 {
                self.ph_saw -= 1.0;
            }
            self.ph_sq += dt;
            if self.ph_sq >= 1.0 {
                self.ph_sq -= 1.0;
            }
            let raw = 0.6 * saw(self.ph_saw, dt) + 0.4 * square(self.ph_sq, dt);

            self.am += 0.5 * inc;
            if self.am >= 1.0 {
                self.am -= 1.0;
            }
            let breathe = 0.72 + 0.28 * sine(self.am);

            let body = self.filt.process(raw);
            let a = body * breathe * self.gate * 0.26;
            add_frame(out, i * ch, a, a, ch);
        }
    }
}

// ---------------------------------------------------------------------------
// one-shot voices
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Fire,
    Turret,
    Explosion,
    Shield,
    Pickup,
    Thud,
    Blip,
    Select,
    /// A special weapon leaving the tube: heavier and slower than the gun.
    Launch,
    /// A laser-like crack: the ion cannon and the energy weapons.
    Laser,
    /// Ice forming: `Freezer`.
    Freeze,
    /// The electric blast's discharge.
    Zap,
    /// The teleporter's arrival chime.
    Warp,
}

/// A single polyphonic effect slot. Fields are shared between effect recipes;
/// each recipe only touches the ones it needs.
#[derive(Clone, Copy)]
struct OneShot {
    kind: Kind,
    active: bool,
    dying: bool,
    rel: f32,
    rel_mul: f32,
    age: f32,
    dur: f32,
    inc: f32,
    p0: f32,
    p1: f32,
    p2: f32,
    ph: [f32; 5],
    ctr: u32,
    filt: Svf,
    filt2: Svf,
    rng: Rng,
}

impl OneShot {
    const IDLE: OneShot = OneShot {
        kind: Kind::Blip,
        active: false,
        dying: false,
        rel: 1.0,
        rel_mul: 0.999,
        age: 0.0,
        dur: 1.0,
        inc: 1.0 / 48_000.0,
        p0: 0.0,
        p1: 0.0,
        p2: 0.0,
        ph: [0.0; 5],
        ctr: 0,
        filt: Svf::IDLE,
        filt2: Svf::IDLE,
        rng: Rng::new(1),
    };

    /// Advance time; returns false once the slot is finished.
    #[inline]
    fn advance(&mut self) -> bool {
        self.age += self.inc;
        if self.dying {
            self.rel *= self.rel_mul;
            if self.rel < 1.0e-3 {
                self.rel = 0.0;
                self.active = false;
            }
        }
        if self.age >= self.dur {
            self.active = false;
        }
        self.active
    }

    fn render(&mut self, out: &mut [f32], ch: usize, sr: f32) {
        match self.kind {
            Kind::Fire => self.render_sweep(out, ch, sr, 900.0, 184.0, 5.0, 0.42, 0.34, 3400.0),
            Kind::Turret => self.render_sweep(out, ch, sr, 1500.0, 420.0, 5.5, 0.30, 0.22, 4600.0),
            Kind::Explosion => self.render_explosion(out, ch, sr),
            Kind::Shield => self.render_shield(out, ch, sr),
            Kind::Pickup => self.render_pickup(out, ch, sr),
            Kind::Thud => self.render_thud(out, ch, sr),
            Kind::Blip => self.render_pip(out, ch, sr, 900.0, 0.0, 0.16),
            Kind::Select => self.render_pip(out, ch, sr, 1300.0, 400.0, 0.17),
            Kind::Launch => self.render_sweep(out, ch, sr, 520.0, 90.0, 3.4, 0.55, 0.36, 2200.0),
            Kind::Laser => self.render_sweep(out, ch, sr, 2400.0, 700.0, 7.0, 0.34, 0.28, 5200.0),
            Kind::Freeze => self.render_pip(out, ch, sr, 2600.0, -1200.0, 0.26),
            Kind::Zap => self.render_sweep(out, ch, sr, 3200.0, 240.0, 11.0, 0.16, 0.32, 6400.0),
            Kind::Warp => self.render_pip(out, ch, sr, 420.0, 1900.0, 0.30),
        }
    }

    /// Gunshot shape: a blown-out square plus noise through a closing filter,
    /// both riding a downward exponential pitch sweep.
    #[allow(clippy::too_many_arguments)]
    fn render_sweep(
        &mut self,
        out: &mut [f32],
        ch: usize,
        sr: f32,
        f_hi: f32,
        f_lo: f32,
        rate: f32,
        tau: f32,
        amp: f32,
        cut_hi: f32,
    ) {
        let frames = out.len() / ch.max(1);
        for i in 0..frames {
            let u = if self.dur > 0.0 {
                self.age / self.dur
            } else {
                1.0
            };
            let freq = f_lo + (f_hi - f_lo) * (-rate * u).exp();
            let dt = phase_inc(freq, sr);
            self.ph[0] += dt;
            if self.ph[0] >= 1.0 {
                self.ph[0] -= 1.0;
            }
            let tone = square(self.ph[0], dt);
            let n = self.rng.noise();
            if self.ctr & 15 == 0 {
                self.filt.set(300.0 + cut_hi * (-3.0 * u).exp(), sr);
            }
            self.ctr = self.ctr.wrapping_add(1);
            let body = self.filt.process(n);
            let env = (-self.age / tau).exp() * attack(self.age, 0.0015);
            let a = (tone * 0.55 + body * 0.45) * env * amp * self.rel;
            add_frame(out, i * ch, a, a, ch);
            if !self.advance() {
                break;
            }
        }
    }

    /// Explosion: noise burst through a rapidly closing low-pass, a bright
    /// transient crack, and a sine sub-thump whose pitch drops as it dies.
    fn render_explosion(&mut self, out: &mut [f32], ch: usize, sr: f32) {
        let frames = out.len() / ch.max(1);
        let size = self.p0;
        let tau_noise = self.p1;
        let tau_sub = self.p2;
        let cut0 = 700.0 + 3200.0 * size;
        let sub0 = 52.0 - 18.0 * size;
        let amp = 0.42 + 0.42 * size;
        for i in 0..frames {
            let u = if self.dur > 0.0 {
                self.age / self.dur
            } else {
                1.0
            };
            if self.ctr & 15 == 0 {
                self.filt.set(cut0 * (-3.2 * u).exp() + 60.0, sr);
                self.filt2.set(3800.0 * (-2.0 * u).exp() + 300.0, sr);
            }
            self.ctr = self.ctr.wrapping_add(1);
            let n = self.rng.noise();
            let body = self.filt.process(n);
            let hiss = self.filt2.process(n);
            let crack = n * (-self.age / 0.006).exp() * 0.45;

            let df = phase_inc(sub0 * (1.0 + 0.45 * (-7.0 * u).exp()), sr);
            self.ph[0] += df;
            if self.ph[0] >= 1.0 {
                self.ph[0] -= 1.0;
            }
            let thump = sine(self.ph[0]) * (-self.age / tau_sub).exp();

            let env_n = (-self.age / tau_noise).exp() * attack(self.age, 0.004);
            let a = (body * 0.9 * env_n + hiss * 0.28 * env_n + crack * env_n + thump * 0.95)
                * amp
                * self.rel;
            add_frame(out, i * ch, a, a, ch);
            if !self.advance() {
                break;
            }
        }
    }

    /// Shield hit: five inharmonic partials ringing out at different rates,
    /// plus a very short spark, split slightly across the stereo field.
    fn render_shield(&mut self, out: &mut [f32], ch: usize, sr: f32) {
        const RATIO: [f32; 5] = [1.0, 2.37, 3.41, 4.83, 6.11];
        const LEVEL: [f32; 5] = [1.0, 0.62, 0.44, 0.30, 0.19];
        const DECAY: [f32; 5] = [1.0, 0.72, 0.52, 0.37, 0.25];
        let frames = out.len() / ch.max(1);
        let base = self.p0;
        let tau = self.p1;
        for i in 0..frames {
            let mut l = 0.0;
            let mut r = 0.0;
            for k in 0..5 {
                let dt = phase_inc(base * RATIO[k], sr);
                self.ph[k] += dt;
                if self.ph[k] >= 1.0 {
                    self.ph[k] -= 1.0;
                }
                let v = sine(self.ph[k]) * LEVEL[k] * (-self.age / (tau * DECAY[k])).exp();
                if k % 2 == 0 {
                    l += v;
                    r += v * 0.82;
                } else {
                    l += v * 0.82;
                    r += v;
                }
            }
            let spark = self.filt2.process(self.rng.noise()) * (-self.age / 0.005).exp() * 0.30;
            let env = attack(self.age, 0.0012) * self.rel * 0.5;
            add_frame(
                out,
                i * ch,
                (l * 0.32 + spark) * env,
                (r * 0.32 + spark) * env,
                ch,
            );
            if !self.advance() {
                break;
            }
        }
    }

    /// Pickup: a two-note rising arpeggio, each note shaped by a sin-squared
    /// bell so it starts and ends exactly at zero.
    fn render_pickup(&mut self, out: &mut [f32], ch: usize, sr: f32) {
        let frames = out.len() / ch.max(1);
        let step = (self.dur * 0.5).max(1.0e-3);
        for i in 0..frames {
            let second = self.age >= step;
            let local = if second { self.age - step } else { self.age };
            let x = (local / step).clamp(0.0, 1.0);
            let s = (PI * x).sin();
            let env = s * s;
            let freq = if second { 990.0 } else { 660.0 };
            let dt = phase_inc(freq, sr);
            self.ph[0] += dt;
            if self.ph[0] >= 1.0 {
                self.ph[0] -= 1.0;
            }
            let sig = 0.75 * sine(self.ph[0]) + 0.25 * square(self.ph[0], dt);
            let a = sig * env * 0.34 * self.rel;
            add_frame(out, i * ch, a, a, ch);
            if !self.advance() {
                break;
            }
        }
    }

    /// Thud: a short pitched-down sine plus a low-passed noise slap.
    fn render_thud(&mut self, out: &mut [f32], ch: usize, sr: f32) {
        let frames = out.len() / ch.max(1);
        let power = self.p0;
        let tau = self.p1;
        let f0 = 92.0 - 28.0 * power;
        let amp = 0.30 + 0.40 * power;
        for i in 0..frames {
            let u = if self.dur > 0.0 {
                self.age / self.dur
            } else {
                1.0
            };
            let dt = phase_inc(f0 * (1.0 + 0.5 * (-9.0 * u).exp()), sr);
            self.ph[0] += dt;
            if self.ph[0] >= 1.0 {
                self.ph[0] -= 1.0;
            }
            let tone = sine(self.ph[0]) * (-self.age / tau).exp();
            if self.ctr & 15 == 0 {
                self.filt.set(700.0, sr);
            }
            self.ctr = self.ctr.wrapping_add(1);
            let knock = self.filt.process(self.rng.noise()) * (-self.age / 0.022).exp() * 0.35;
            let a = (tone + knock) * amp * attack(self.age, 0.0015) * self.rel;
            add_frame(out, i * ch, a, a, ch);
            if !self.advance() {
                break;
            }
        }
    }

    /// Tiny UI pip: sine with an optional upward glide, sin-squared bell.
    fn render_pip(&mut self, out: &mut [f32], ch: usize, sr: f32, f0: f32, glide: f32, amp: f32) {
        let frames = out.len() / ch.max(1);
        for i in 0..frames {
            let u = if self.dur > 0.0 {
                self.age / self.dur
            } else {
                1.0
            };
            let dt = phase_inc(f0 + glide * u, sr);
            self.ph[0] += dt;
            if self.ph[0] >= 1.0 {
                self.ph[0] -= 1.0;
            }
            let s = (PI * u.clamp(0.0, 1.0)).sin();
            let env = s * s;
            let sig = 0.8 * sine(self.ph[0]) + 0.2 * square(self.ph[0], dt);
            let a = sig * env * amp * self.rel;
            add_frame(out, i * ch, a, a, ch);
            if !self.advance() {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// mixer
// ---------------------------------------------------------------------------

/// The whole synthesiser: two continuous voices plus a one-shot pool.
pub(crate) struct Mixer {
    sr: f32,
    engine: EngineVoice,
    alarm: AlarmVoice,
    shots: [OneShot; MAX_VOICES],
    dc: [Dc; 2],
    seed: u32,
}

impl Mixer {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let sr = if sample_rate >= 8_000 {
            sample_rate as f32
        } else {
            48_000.0
        };
        Mixer {
            sr,
            engine: EngineVoice::new(sr),
            alarm: AlarmVoice::new(sr),
            shots: [OneShot::IDLE; MAX_VOICES],
            dc: [Dc::IDLE; 2],
            seed: 0x9E37_79B9,
        }
    }

    /// Set the continuous engine loop state. Safe to call every block.
    pub(crate) fn set_engine(&mut self, on: bool, power: f32) {
        self.engine.set(on, power);
    }

    /// Set the continuous alarm loop state. Safe to call every block.
    pub(crate) fn set_alarm(&mut self, on: bool) {
        self.alarm.set(on);
    }

    /// Handle one command. The live callback and offline rendering both go
    /// through here, so the two paths cannot drift apart.
    pub(crate) fn apply(&mut self, cmd: AudioCmd) {
        match cmd {
            AudioCmd::Thrust { on, power } => self.set_engine(on, power),
            AudioCmd::Alarm { on } => self.set_alarm(on),
            AudioCmd::Fire => self.spawn_fire(),
            AudioCmd::TurretFire => self.spawn_turret(),
            AudioCmd::Explosion { size } => self.spawn_explosion(sanitize01(size)),
            AudioCmd::ShieldHit => self.spawn_shield(),
            AudioCmd::Pickup => self.spawn_pickup(),
            AudioCmd::Thud { power } => self.spawn_thud(sanitize01(power)),
            AudioCmd::UiBlip => self.spawn_pip(Kind::Blip, 0.06),
            AudioCmd::UiSelect => self.spawn_pip(Kind::Select, 0.09),
            AudioCmd::Special { voice } => self.spawn_special(voice),
            AudioCmd::Freeze => self.spawn_pip(Kind::Freeze, 0.26),
            AudioCmd::Zap => self.spawn_pip(Kind::Zap, 0.30),
            AudioCmd::Warp => self.spawn_pip(Kind::Warp, 0.30),
            AudioCmd::AllStop => self.stop_all(),
        }
    }

    /// Silence everything; one-shots are faded out over a few milliseconds.
    pub(crate) fn stop_all(&mut self) {
        self.engine.set(false, 0.0);
        self.alarm.set(false);
        self.kill_shots();
    }

    /// Fade out every running one-shot, leaving the continuous loops alone.
    ///
    /// The live callback uses this for a queued `AllStop`: the loop part of
    /// that command is applied through the state atomics the moment the game
    /// sends it, so re-applying it from the queue could cancel a `Thrust` the
    /// game issued afterwards.
    pub(crate) fn kill_shots(&mut self) {
        for s in self.shots.iter_mut() {
            if s.active {
                s.dying = true;
            }
        }
    }

    /// Reserve a slot for a new effect, stealing the most-finished one when the
    /// pool is full.
    fn spawn(&mut self, dur: f32) -> &mut OneShot {
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let seed = self.seed;
        let sr = self.sr;

        let mut slot = 0;
        let mut best = -1.0f32;
        for (i, s) in self.shots.iter().enumerate() {
            if !s.active {
                slot = i;
                break;
            }
            let progress = if s.dur > 0.0 { s.age / s.dur } else { 1.0 };
            if progress > best {
                best = progress;
                slot = i;
            }
        }

        let v = &mut self.shots[slot];
        *v = OneShot::IDLE;
        v.active = true;
        v.dur = if dur.is_finite() {
            dur.clamp(0.02, 12.0)
        } else {
            0.2
        };
        v.inc = 1.0 / sr;
        v.rel_mul = (-1.0 / (0.004 * sr)).exp();
        v.rng = Rng::new(seed);
        v
    }

    fn spawn_fire(&mut self) {
        let sr = self.sr;
        let v = self.spawn(0.26);
        v.kind = Kind::Fire;
        v.filt = Svf::new(0.9, 3600.0, sr);
    }

    fn spawn_turret(&mut self) {
        let sr = self.sr;
        let v = self.spawn(0.20);
        v.kind = Kind::Turret;
        v.filt = Svf::new(1.1, 4600.0, sr);
    }

    fn spawn_explosion(&mut self, size: f32) {
        let dur = 0.45 + 0.95 * size;
        let sr = self.sr;
        let v = self.spawn(dur);
        v.kind = Kind::Explosion;
        v.p0 = size;
        v.p1 = dur / 6.5;
        v.p2 = dur / 3.5;
        v.filt = Svf::new(0.9, 700.0 + 3200.0 * size, sr);
        v.filt2 = Svf::new(1.0, 6000.0, sr);
    }

    fn spawn_shield(&mut self) {
        let sr = self.sr;
        let v = self.spawn(0.50);
        v.kind = Kind::Shield;
        v.p0 = 780.0;
        v.p1 = 0.50 / 6.5;
        v.filt = Svf::new(1.0, 6000.0, sr);
        v.filt2 = Svf::new(1.0, 6000.0, sr);
    }

    fn spawn_pickup(&mut self) {
        let v = self.spawn(0.18);
        v.kind = Kind::Pickup;
    }

    fn spawn_thud(&mut self, power: f32) {
        let dur = 0.20 + 0.12 * power;
        let sr = self.sr;
        let v = self.spawn(dur);
        v.kind = Kind::Thud;
        v.p0 = power;
        v.p1 = dur / 6.5;
        v.filt = Svf::new(1.0, 700.0, sr);
    }

    fn spawn_pip(&mut self, kind: Kind, dur: f32) {
        let v = self.spawn(dur);
        v.kind = kind;
    }

    /// A special weapon's own voice: the roster is wide, so the look-up is by
    /// what the weapon *does* rather than 33 separate sounds.
    fn spawn_special(&mut self, voice: SpecialVoice) {
        let (kind, dur) = match voice {
            SpecialVoice::Launch => (Kind::Launch, 0.34),
            SpecialVoice::Laser => (Kind::Laser, 0.22),
            SpecialVoice::Blast => (Kind::Explosion, 0.40),
            SpecialVoice::Cold => (Kind::Freeze, 0.24),
            SpecialVoice::Field => (Kind::Warp, 0.28),
            SpecialVoice::Tool => (Kind::Thud, 0.22),
            SpecialVoice::Shot => (Kind::Fire, 0.18),
        };
        let v = self.spawn(dur);
        v.kind = kind;
    }

    /// Render `out` (interleaved, `channels` wide) with `master` gain applied.
    pub(crate) fn render(&mut self, out: &mut [f32], channels: usize, master: f32) {
        let ch = channels.max(1);
        out.fill(0.0);

        self.engine.render(out, ch, self.sr);
        self.alarm.render(out, ch, self.sr);
        for s in self.shots.iter_mut() {
            if s.active {
                s.render(out, ch, self.sr);
            }
        }

        let m = if master.is_finite() {
            master.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let frames = out.len() / ch;
        for i in 0..frames {
            let base = i * ch;
            let l = self.dc[0].process(out[base]);
            let r = if ch > 1 {
                self.dc[1].process(out[base + 1])
            } else {
                l
            };
            out[base] = soft_clip(l * m);
            if ch > 1 {
                out[base + 1] = soft_clip(r * m);
                for c in 2..ch {
                    out[base + c] = 0.0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_seq(rate: u32, seconds: f32) -> Vec<f32> {
        let mut mixer = Mixer::new(rate);
        let frames = (rate as f32 * seconds) as usize;
        let mut out = vec![0.0f32; frames * 2];
        mixer.apply(AudioCmd::Thrust {
            on: true,
            power: 0.4,
        });
        mixer.render(&mut out, 2, 1.0);
        out
    }

    #[test]
    fn stays_in_range_and_centred() {
        let out = render_seq(48_000, 0.5);
        let mut peak = 0.0f32;
        let mut sum = 0.0f64;
        for s in &out {
            assert!(s.is_finite(), "non-finite sample");
            peak = peak.max(s.abs());
            sum += *s as f64;
        }
        assert!(peak > 0.01, "engine should be audible, peak={peak}");
        assert!(peak <= 1.0, "peak out of range: {peak}");
        let dc = sum / out.len() as f64;
        assert!(dc.abs() < 1.0e-3, "dc offset {dc}");
    }

    #[test]
    fn silent_when_idle_and_after_stop() {
        let mut mixer = Mixer::new(48_000);
        let mut out = vec![0.0f32; 4800 * 2];
        mixer.render(&mut out, 2, 1.0);
        assert!(out.iter().all(|s| *s == 0.0));

        mixer.apply(AudioCmd::Explosion { size: 0.8 });
        mixer.apply(AudioCmd::AllStop);
        for _ in 0..20 {
            mixer.render(&mut out, 2, 1.0);
        }
        let tail = &out[out.len() / 2..];
        assert!(tail.iter().all(|s| s.abs() < 1.0e-3), "tail not silent");
    }

    #[test]
    fn every_effect_ends_silent() {
        let cmds = [
            AudioCmd::Fire,
            AudioCmd::TurretFire,
            AudioCmd::Explosion { size: 1.0 },
            AudioCmd::ShieldHit,
            AudioCmd::Pickup,
            AudioCmd::Thud { power: 1.0 },
            AudioCmd::UiBlip,
            AudioCmd::UiSelect,
        ];
        for cmd in cmds {
            let mut mixer = Mixer::new(48_000);
            mixer.apply(cmd);
            let mut out = vec![0.0f32; 48_000 * 8 / 5]; // 1.6 s mono
            mixer.render(&mut out, 1, 1.0);
            let peak = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
            assert!(peak > 0.005, "{cmd:?} inaudible (peak {peak})");
            let last = out[out.len() - 200..]
                .iter()
                .fold(0.0f32, |a, s| a.max(s.abs()));
            assert!(last < 0.01, "{cmd:?} does not end silent (peak {last})");
        }
    }

    #[test]
    fn extreme_inputs_do_not_blow_up() {
        let mut mixer = Mixer::new(44_100);
        for cmd in [
            AudioCmd::Thrust {
                on: true,
                power: f32::NAN,
            },
            AudioCmd::Thrust {
                on: true,
                power: 1.0e30,
            },
            AudioCmd::Thrust {
                on: true,
                power: -5.0,
            },
            AudioCmd::Explosion {
                size: f32::INFINITY,
            },
            AudioCmd::Explosion { size: -1.0 },
            AudioCmd::Thud { power: f32::NAN },
            AudioCmd::Alarm { on: true },
        ] {
            mixer.apply(cmd);
        }
        let mut out = vec![0.0f32; 4096 * 2];
        for _ in 0..200 {
            mixer.render(&mut out, 2, 1.0);
            for s in &out {
                assert!(s.is_finite() && s.abs() <= 1.0, "sample out of range: {s}");
            }
        }
    }

    #[test]
    fn engine_stop_is_not_a_click() {
        fn max_step(x: &[f32]) -> f32 {
            let mut m = 0.0f32;
            for w in x.windows(2) {
                m = m.max((w[1] - w[0]).abs());
            }
            m
        }
        let rate = 48_000usize;
        let mut mixer = Mixer::new(rate as u32);
        mixer.apply(AudioCmd::Thrust {
            on: true,
            power: 1.0,
        });
        let mut running = vec![0.0f32; rate];
        mixer.render(&mut running, 1, 1.0);
        let steady = max_step(&running[rate / 2..]);
        assert!(steady > 0.001, "engine too quiet to judge: {steady}");

        mixer.apply(AudioCmd::Thrust {
            on: false,
            power: 1.0,
        });
        let mut tail = vec![0.0f32; rate / 2];
        mixer.render(&mut tail, 1, 1.0);
        let stopping = max_step(&tail[..rate / 10]);
        assert!(
            stopping <= steady,
            "engine stop steps {stopping} above steady-state {steady}"
        );
    }

    #[test]
    fn mono_and_empty_buffers_are_safe() {
        let mut mixer = Mixer::new(48_000);
        mixer.apply(AudioCmd::Alarm { on: true });
        mixer.apply(AudioCmd::Fire);
        let mut empty: [f32; 0] = [];
        mixer.render(&mut empty, 2, 1.0);
        let mut stereo = vec![0.0f32; 512];
        mixer.render(&mut stereo, 2, 1.0);
        let mut mono = vec![0.0f32; 256];
        mixer.render(&mut mono, 1, 1.0);
        assert!(mono.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }
}
