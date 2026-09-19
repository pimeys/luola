//! Input-log replays.
//!
//! A replay is the level, the control scheme, the seed and one `InputFrame` per
//! simulation tick. Since the integration is fixed-order `f32` at a fixed
//! timestep, replaying the same log reproduces the same run bit for bit, and the
//! stored checksum proves it (`docs/game_mechanics.md` §9, §12).
//!
//! The format is a small line-oriented text file: inputs are stored as raw bit
//! patterns so `f32` values round-trip exactly.

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use crate::math::V2;
use crate::sim::inputs::{InputFrame, Scheme};
use crate::sim::weapons::WeaponId;

/// Bumped to 2 when the weapon system landed: a replay now carries the special
/// the ship launched with, and an input frame carries the weapon buttons.
pub const MAGIC: &str = "# luola-replay 2";

#[derive(Clone, Debug)]
pub struct Replay {
    pub source: Option<PathBuf>,
    pub level: Option<PathBuf>,
    pub level_name: String,
    pub scheme: Scheme,
    pub seed: u64,
    /// The special weapon the run started with. A run that swapped weapons at a
    /// base reproduces that through its input frames; only the launch loadout
    /// has to be recorded, because it comes from the level and the command line
    /// rather than from the pilot.
    pub weapon: Option<WeaponId>,
    pub frames: Vec<InputFrame>,
    /// Checksum of the world after the last frame, when the recorder knows it.
    pub checksum: Option<u64>,
    /// Short result string (`escaped 42.1s score=8123`).
    pub result: Option<String>,
}

impl Replay {
    pub fn new(
        level: Option<PathBuf>,
        level_name: impl Into<String>,
        scheme: Scheme,
        seed: u64,
    ) -> Self {
        Self {
            source: None,
            level,
            level_name: level_name.into(),
            scheme,
            seed,
            frames: Vec::new(),
            weapon: None,
            checksum: None,
            result: None,
        }
    }

    pub fn push(&mut self, frame: InputFrame) {
        self.frames.push(frame);
    }

    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(64 + self.frames.len() * 30);
        let _ = writeln!(out, "{MAGIC}");
        let _ = writeln!(
            out,
            "level {}",
            self.level
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        );
        let _ = writeln!(out, "name {}", self.level_name);
        let _ = writeln!(out, "scheme {}", self.scheme.name());
        let _ = writeln!(out, "seed {}", self.seed);
        let _ = writeln!(
            out,
            "weapon {}",
            self.weapon
                .map(|w| crate::sim::weapons::spec(w).name)
                .unwrap_or("-")
        );
        let _ = writeln!(out, "frames {}", self.frames.len());
        if let Some(c) = self.checksum {
            let _ = writeln!(out, "checksum {c:#018x}");
        }
        if let Some(r) = &self.result {
            let _ = writeln!(out, "result {r}");
        }
        for f in &self.frames {
            let _ = writeln!(
                out,
                "{:04x} {:08x} {:08x} {:08x}",
                f.buttons,
                f.move_dir.x.to_bits(),
                f.move_dir.y.to_bits(),
                f.aim.to_bits()
            );
        }
        out
    }

    pub fn parse(text: &str) -> Result<Replay, String> {
        let mut lines = text.lines();
        match lines.next() {
            Some(l) if l.trim() == MAGIC => {}
            Some(l) => return Err(format!("not a luola replay: first line is {l:?}")),
            None => return Err("empty replay".into()),
        }
        let mut replay = Replay::new(None, "", Scheme::Classic, 0);
        let mut declared_frames: Option<usize> = None;

        for (n, line) in lines.enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap_or_default();
            match key {
                "level" => {
                    let v = parts.collect::<Vec<_>>().join(" ");
                    replay.level = if v.is_empty() { None } else { Some(v.into()) };
                }
                "name" => {
                    replay.level_name = parts.collect::<Vec<_>>().join(" ");
                }
                "scheme" => {
                    let v = parts.next().unwrap_or_default();
                    replay.scheme =
                        Scheme::parse(v).ok_or_else(|| format!("unknown scheme {v:?}"))?;
                }
                "weapon" => {
                    // Weapon names contain spaces (`electric blast`, `grenade
                    // launcher`), so the whole rest of the line is the name.
                    let v = parts.collect::<Vec<_>>().join(" ");
                    replay.weapon = if v.is_empty() || v == "-" {
                        None
                    } else {
                        Some(
                            crate::sim::weapons::parse(&v)
                                .ok_or_else(|| format!("unknown weapon {v:?} on line {}", n + 2))?,
                        )
                    };
                }
                "seed" => {
                    replay.seed = parts
                        .next()
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| format!("bad seed on line {}", n + 2))?;
                }
                "frames" => {
                    declared_frames = Some(
                        parts
                            .next()
                            .and_then(|v| v.parse().ok())
                            .ok_or_else(|| format!("bad frame count on line {}", n + 2))?,
                    );
                }
                "checksum" => {
                    let v = parts.next().unwrap_or_default();
                    let v = v.trim_start_matches("0x");
                    replay.checksum = Some(
                        u64::from_str_radix(v, 16)
                            .map_err(|_| format!("bad checksum {v:?} on line {}", n + 2))?,
                    );
                }
                "result" => {
                    replay.result = Some(parts.collect::<Vec<_>>().join(" "));
                }
                _ => {
                    // A frame line: buttons + three bit patterns.
                    let buttons = u16::from_str_radix(key, 16)
                        .map_err(|_| format!("bad frame line {}: {line:?}", n + 2))?;
                    let mx = parse_bits(parts.next(), n + 2)?;
                    let my = parse_bits(parts.next(), n + 2)?;
                    let aim = parse_bits(parts.next(), n + 2)?;
                    replay.frames.push(InputFrame {
                        buttons,
                        move_dir: V2::new(f32::from_bits(mx), f32::from_bits(my)),
                        aim: f32::from_bits(aim),
                    });
                }
            }
        }

        if let Some(n) = declared_frames
            && n != replay.frames.len()
        {
            return Err(format!(
                "replay declares {n} frames but contains {}",
                replay.frames.len()
            ));
        }
        Ok(replay)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_text())
    }

    pub fn load(path: &Path) -> io::Result<Replay> {
        let text = std::fs::read_to_string(path)?;
        let mut replay =
            Replay::parse(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        replay.source = Some(path.to_path_buf());
        Ok(replay)
    }
}

fn parse_bits(part: Option<&str>, line: usize) -> Result<u32, String> {
    let v = part.ok_or_else(|| format!("missing field on line {line}"))?;
    u32::from_str_radix(v.trim_start_matches("0x"), 16)
        .map_err(|_| format!("bad bit pattern {v:?} on line {line}"))
}
