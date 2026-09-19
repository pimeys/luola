//! Rendering layer: software rasterizer, camera, particles, HUD.
//!
//! There is no GPU, no shader and no texture: a small framebuffer is drawn with
//! vector primitives and blitted to the window at an integer scale.

pub mod camera;
pub mod fb;
pub mod font;
pub mod fx;
pub mod hud;
pub mod palette;
pub mod particles;
pub mod scene;
