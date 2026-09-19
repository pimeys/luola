//! Audio: a self-contained, asset-free synthesiser for the game's sound.
//!
//! The game thread never blocks on audio. [`AudioEngine::send`] writes either a
//! lock-free atomic (for the two looping voices) or a slot in a small fixed
//! ring buffer (for discrete effects), and a single cpal output callback pulls
//! from the mixer. No allocation happens on either side, and a machine without
//! a usable output device simply gets an inert engine instead of an error.

mod synth;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use cpal::SampleFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use synth::Mixer;

/// Number of pending discrete events the audio thread may lag behind by.
/// Looping voices do not use this queue at all.
const QUEUE_CAP: usize = 512;

/// Which voice a special weapon gets. The roster is 33 weapons wide, so they are
/// grouped by what they do rather than given 33 separate sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialVoice {
    /// A projectile leaving the tube: rockets, bombs, torpedoes.
    Launch,
    /// An energy shot: ion cannon, electric blast.
    Laser,
    /// Something that goes off: nukes, explosives.
    Blast,
    /// Cold and nets.
    Cold,
    /// The ship's own fields: shield, teleport.
    Field,
    /// Tools: dirt, water, wells.
    Tool,
    /// A plain forward weapon.
    Shot,
}

/// What the game asks the audio layer to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AudioCmd {
    /// The special weapon fired, in its family's voice.
    Special {
        voice: SpecialVoice,
    },
    /// `Freezer`.
    Freeze,
    /// The electric blast.
    Zap,
    /// The teleporter.
    Warp,
    /// Continuous engine loop; `on=false` fades the running engine out.
    Thrust {
        on: bool,
        power: f32,
    }, // power 0..1, scales pitch/loudness
    Fire,       // player cannon
    TurretFire, // enemy turret shot, different pitch
    Explosion {
        size: f32,
    }, // 0..1: bigger = longer, lower, louder
    ShieldHit,
    Pickup,
    Thud {
        power: f32,
    }, // payload bumping / landing
    Alarm {
        on: bool,
    }, // looping klaxon while a reactor is critical
    UiBlip,
    UiSelect,
    /// Silence all loops (pause, death, level end).
    AllStop,
}

impl AudioCmd {
    /// The voice for a weapon: chosen from the spec, so the sound follows the
    /// behaviour and never has to be listed per weapon.
    pub fn special(weapon: crate::sim::weapons::WeaponId) -> AudioCmd {
        use crate::sim::weapons::{Effect, Kind, WeaponId};
        let spec = crate::sim::weapons::spec(weapon);
        let voice = match (weapon, spec.kind, spec.effect) {
            (_, _, Effect::Shield | Effect::Blink | Effect::Emp) => SpecialVoice::Field,
            (_, _, Effect::Freeze | Effect::Net) => SpecialVoice::Cold,
            (WeaponId::Nuke, ..) => SpecialVoice::Blast,
            (WeaponId::IonCannon, ..) => SpecialVoice::Laser,
            (_, Kind::Shell, _) => SpecialVoice::Launch,
            (_, Kind::Place, _) => SpecialVoice::Tool,
            (WeaponId::Dirtball | WeaponId::Watercannon | WeaponId::Gravitor, ..) => {
                SpecialVoice::Tool
            }
            (WeaponId::Missile | WeaponId::Torpedo | WeaponId::Rockets | WeaponId::Bats, ..) => {
                SpecialVoice::Launch
            }
            _ => SpecialVoice::Shot,
        };
        AudioCmd::Special { voice }
    }
}

/// Fixed-capacity ring of pending events. Never grows, never allocates, never
/// panics: a full queue drops the newest event unless it is an `AllStop`, which
/// is important enough to displace the oldest one instead.
struct Queue {
    buf: [AudioCmd; QUEUE_CAP],
    head: usize,
    len: usize,
}

impl Queue {
    const fn new() -> Self {
        Queue {
            buf: [AudioCmd::AllStop; QUEUE_CAP],
            head: 0,
            len: 0,
        }
    }

    fn push(&mut self, cmd: AudioCmd) {
        if self.len == QUEUE_CAP {
            if matches!(cmd, AudioCmd::AllStop) {
                self.head = (self.head + 1) % QUEUE_CAP;
                self.len -= 1;
            } else {
                return;
            }
        }
        self.buf[(self.head + self.len) % QUEUE_CAP] = cmd;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<AudioCmd> {
        if self.len == 0 {
            return None;
        }
        let cmd = self.buf[self.head];
        self.head = (self.head + 1) % QUEUE_CAP;
        self.len -= 1;
        Some(cmd)
    }
}

/// State shared between the game thread and the audio callback.
struct Shared {
    queue: Mutex<Queue>,
    volume: AtomicU32,
    engine_on: AtomicBool,
    engine_power: AtomicU32,
    alarm_on: AtomicBool,
}

impl Shared {
    fn new(volume: f32) -> Self {
        Shared {
            queue: Mutex::new(Queue::new()),
            volume: AtomicU32::new(volume.to_bits()),
            engine_on: AtomicBool::new(false),
            engine_power: AtomicU32::new(0.0f32.to_bits()),
            alarm_on: AtomicBool::new(false),
        }
    }

    #[inline]
    fn power(&self) -> f32 {
        synth::sanitize01(f32::from_bits(self.engine_power.load(Ordering::Relaxed)))
    }
}

/// A poisoned lock can only mean someone panicked while holding it; we never
/// do, and losing audio is strictly better than panicking here.
#[inline]
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The audio device and its mixer. Dropping this stops all sound.
pub struct AudioEngine {
    shared: Option<Arc<Shared>>,
    /// Kept alive for as long as the engine lives; dropping it ends playback.
    _stream: Option<cpal::Stream>,
    sample_rate: u32,
    device_name: Option<String>,
}

impl AudioEngine {
    /// Never fails, never panics. With no output device the engine is inert
    /// (`enabled() == false`) and `send` is a cheap no-op. Prints at most one
    /// short notice to stderr when disabled.
    pub fn new() -> AudioEngine {
        AudioEngine::with_volume(1.0)
    }

    /// Same but with an explicit master volume 0..1 (clamped).
    pub fn with_volume(volume: f32) -> AudioEngine {
        let volume = synth::sanitize01(volume);
        if audio_disabled_by_env() {
            eprintln!("luola: audio disabled (LUOLA_NO_AUDIO)");
            return AudioEngine::disabled();
        }
        match open_device(volume) {
            Some((shared, stream, sample_rate, name)) => AudioEngine {
                shared: Some(shared),
                _stream: Some(stream),
                sample_rate,
                device_name: Some(name),
            },
            None => {
                eprintln!("luola: audio disabled (no usable output device)");
                AudioEngine::disabled()
            }
        }
    }

    fn disabled() -> AudioEngine {
        AudioEngine {
            shared: None,
            _stream: None,
            sample_rate: 0,
            device_name: None,
        }
    }

    pub fn enabled(&self) -> bool {
        self.shared.is_some()
    }

    pub fn device_name(&self) -> Option<String> {
        self.device_name.clone()
    }

    /// Output sample rate, or 0 when disabled.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Non-blocking; called from the game thread every simulation event.
    pub fn send(&self, cmd: AudioCmd) {
        let Some(shared) = self.shared.as_ref() else {
            return;
        };
        match cmd {
            // Looping voices are plain atomics: no queue slot, no lock, and a
            // repeated command costs the same as a first one.
            AudioCmd::Thrust { on, power } => {
                shared.engine_on.store(on, Ordering::Relaxed);
                shared
                    .engine_power
                    .store(synth::sanitize01(power).to_bits(), Ordering::Relaxed);
            }
            AudioCmd::Alarm { on } => shared.alarm_on.store(on, Ordering::Relaxed),
            AudioCmd::AllStop => {
                shared.engine_on.store(false, Ordering::Relaxed);
                shared.alarm_on.store(false, Ordering::Relaxed);
                shared
                    .engine_power
                    .store(0.0f32.to_bits(), Ordering::Relaxed);
                lock(&shared.queue).push(cmd);
            }
            _ => lock(&shared.queue).push(cmd),
        }
    }

    pub fn set_volume(&self, volume: f32) {
        if let Some(shared) = self.shared.as_ref() {
            shared
                .volume
                .store(synth::sanitize01(volume).to_bits(), Ordering::Relaxed);
        }
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        AudioEngine::new()
    }
}

/// `LUOLA_NO_AUDIO=1` (any non-empty value) skips device opening entirely.
fn audio_disabled_by_env() -> bool {
    match std::env::var("LUOLA_NO_AUDIO") {
        Ok(v) => !v.is_empty(),
        Err(_) => false,
    }
}

/// Pick an f32 output configuration, preferring stereo at 48 kHz.
fn pick_config(device: &cpal::Device) -> Option<cpal::StreamConfig> {
    let mut best: Option<(i32, cpal::StreamConfig)> = None;
    if let Ok(ranges) = device.supported_output_configs() {
        for range in ranges {
            if range.sample_format() != SampleFormat::F32 {
                continue;
            }
            let channels = range.channels();
            if channels == 0 {
                continue;
            }
            let score = match channels {
                2 => 4,
                1 => 1,
                _ => 2,
            } + if range.contains_rate(48_000) { 2 } else { 0 };
            if best.as_ref().is_some_and(|(s, _)| *s >= score) {
                continue;
            }
            let rate = 48_000u32
                .max(range.min_sample_rate())
                .min(range.max_sample_rate());
            if let Some(cfg) = range.try_with_sample_rate(rate) {
                best = Some((score, cfg.config()));
            }
        }
    }
    if let Some((_, cfg)) = best {
        return Some(cfg);
    }
    // No enumerated f32 range: fall back to the device default if it is f32.
    match device.default_output_config() {
        Ok(def) if def.sample_format() == SampleFormat::F32 && def.channels() > 0 => {
            Some(def.config())
        }
        _ => None,
    }
}

/// Open the default output device and start a stream. `None` on any failure —
/// the caller degrades to a disabled engine, never a panic.
fn open_device(volume: f32) -> Option<(Arc<Shared>, cpal::Stream, u32, String)> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let name = device.to_string();
    let config = pick_config(&device)?;

    let channels = config.channels.max(1) as usize;
    let sample_rate = config.sample_rate;
    let shared = Arc::new(Shared::new(volume));
    let mut mixer = Mixer::new(sample_rate);

    let cb_shared = Arc::clone(&shared);
    let stream = device
        .build_output_stream::<f32, _, _>(
            config,
            move |data: &mut [f32], _info| {
                // Looping voices first: the atomics always reflect the newest
                // game state, so the loops track the game without queueing.
                mixer.set_engine(
                    cb_shared.engine_on.load(Ordering::Relaxed),
                    cb_shared.power(),
                );
                mixer.set_alarm(cb_shared.alarm_on.load(Ordering::Relaxed));

                // Discrete events, in order. The lock is held for the handful
                // of microseconds it takes to fold them into the mixer.
                //
                // `AllStop` only silences the effects here: its loop half was
                // already applied above (and at send time), so re-applying it
                // could not cancel a `Thrust` issued after the stop.
                let mut queue = lock(&cb_shared.queue);
                while let Some(cmd) = queue.pop() {
                    if matches!(cmd, AudioCmd::AllStop) {
                        mixer.kill_shots();
                    } else {
                        mixer.apply(cmd);
                    }
                }
                drop(queue);

                let volume = f32::from_bits(cb_shared.volume.load(Ordering::Relaxed));
                mixer.render(data, channels, volume);
            },
            |_err| {},
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some((shared, stream, sample_rate, name))
}
