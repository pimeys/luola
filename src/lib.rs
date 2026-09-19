//! Luolalentely — a cave flyer.
//!
//! Layout:
//! - [`sim`]: deterministic fixed-timestep simulation (no rendering, no I/O).
//! - [`render`]: software rasterizer, particles, camera, HUD.
//! - [`audio`]: cpal-synthesized engine, weapons and klaxons.
//! - [`app`]: winit shell, control bindings, game states.
//! - [`headless`]: run the simulation without a window, for replays and tests.

pub mod app;
pub mod audio;
pub mod headless;
pub mod image;
pub mod math;
pub mod render;
pub mod sim;
