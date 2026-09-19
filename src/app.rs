//! Window, input bindings, game states and the main loop.
//!
//! The loop is a fixed-timestep accumulator: input is sampled once per frame,
//! the simulation runs in whole 1/120 s steps, and rendering happens once per
//! frame. Replays record the *input frames*, never wall-clock deltas, so a
//! replay is exact regardless of frame rate.

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::audio::{AudioCmd, AudioEngine};
use crate::headless::SimRunner;
use crate::math::V2;
use crate::render::camera::Camera;
use crate::render::fb::Framebuffer;
use crate::render::fx::Fx;
use crate::render::hud;
use crate::render::palette as pal;
use crate::render::scene;
use crate::sim::inputs::{
    BTN_BEAM, BTN_DUMP, BTN_FIRE, BTN_ROTATE_CCW, BTN_ROTATE_CW, BTN_SPECIAL, BTN_THRUST,
    BTN_WEAPON_NEXT, BTN_WEAPON_PREV, InputFrame, Scheme,
};
use crate::sim::level::Level;
use crate::sim::replay::Replay;
use crate::sim::tuning::{DT, MAX_STEPS_PER_FRAME};
use crate::sim::weapons::WeaponId;
use crate::sim::world::{RunState, World};

/// Target internal resolution: the framebuffer is the window divided by an
/// integer scale, so pixels stay square and the vector look stays crisp. 320
/// rows keeps the ship and the cave furniture readable at 720p and above while
/// still showing a good slice of a multi-screen cave.
const TARGET_HEIGHT: u32 = 320;
const MIN_INTERNAL_WIDTH: i32 = 320;
const MAX_INTERNAL_WIDTH: i32 = 960;

#[derive(Clone, Debug)]
pub struct AppConfig {
    /// Campaign in play order.
    pub levels: Vec<PathBuf>,
    pub start_index: usize,
    pub scheme: Scheme,
    pub seed: u64,
    /// Special weapon to mount at launch, overriding the level's own choice.
    pub weapon: Option<WeaponId>,
    /// Save a replay of every run to this path.
    pub record: Option<PathBuf>,
    /// Play back a recorded input log instead of reading the keyboard.
    pub replay: Option<Replay>,
    pub mute: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            levels: campaign_paths(),
            start_index: 0,
            scheme: Scheme::Classic,
            seed: 0x10aa_1e17,
            weapon: None,
            record: None,
            replay: None,
            mute: false,
        }
    }
}

/// The shipped campaign, resolved relative to `LUOLA_LEVELS` or `./levels`.
pub fn campaign_paths() -> Vec<PathBuf> {
    let dir = std::env::var("LUOLA_LEVELS").unwrap_or_else(|_| "levels".to_string());
    ["01_first_descent", "02_payload", "03_reactor_run"]
        .iter()
        .map(|name| Path::new(&dir).join(format!("{name}.toml")))
        .collect()
}

pub fn run(config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut game = Game::new(config)?;
    event_loop.run_app(&mut game)?;
    Ok(())
}

/// What a key press asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    None,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Title,
    Briefing,
    Playing,
    Paused,
    Help,
    CampaignComplete,
}

impl Screen {
    fn simulates(self) -> bool {
        matches!(self, Screen::Playing)
    }
}

pub struct Game {
    config: AppConfig,
    screen: Screen,
    help_return: Screen,
    level_index: usize,
    level: Arc<Level>,
    runner: SimRunner,
    fx: Fx,
    camera: Camera,
    audio: AudioEngine,

    window: Option<Arc<Window>>,
    context: Option<softbuffer::Context<Arc<Window>>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    fb: Framebuffer,
    /// Integer upscale factor and letterbox offset used when presenting.
    scale: u32,
    offset: (i32, i32),

    keys: HashSet<KeyCode>,
    mouse_buttons: HashSet<MouseButton>,
    /// Cursor position in framebuffer pixels.
    mouse: V2,

    accumulator: f32,
    last_frame: Instant,
    hint_timer: f32,

    playback: Option<Replay>,
    playback_frame: usize,
    record: Option<Replay>,
    record_path: Option<PathBuf>,

    ended_for: f32,
    total_score: i32,
    total_time: f32,
    present_error_reported: bool,
}

impl Game {
    pub fn new(config: AppConfig) -> Result<Self, String> {
        let path = config
            .levels
            .get(config.start_index)
            .cloned()
            .ok_or_else(|| "no levels configured".to_string())?;
        let level = Arc::new(Level::load(&path).map_err(|e| format!("cannot load level: {e}"))?);
        let scheme = config
            .replay
            .as_ref()
            .map(|r| r.scheme)
            .unwrap_or(config.scheme);
        let seed = config
            .replay
            .as_ref()
            .map(|r| r.seed)
            .unwrap_or(config.seed);
        let audio = if config.mute {
            AudioEngine::with_volume(0.0)
        } else {
            AudioEngine::new()
        };
        let record_path = config.record.clone();
        let record = record_path
            .as_ref()
            .map(|_| Replay::new(level.source.clone(), level.name.clone(), scheme, seed));
        Ok(Self {
            screen: Screen::Title,
            help_return: Screen::Title,
            level_index: config.start_index,
            runner: SimRunner::with_weapon(level.clone(), scheme, seed, config.weapon),
            level,
            fx: Fx::new(),
            camera: Camera::new(960.0, 540.0),
            audio,
            window: None,
            context: None,
            surface: None,
            fb: Framebuffer::new(MIN_INTERNAL_WIDTH, 200),
            scale: 1,
            offset: (0, 0),
            keys: HashSet::new(),
            mouse_buttons: HashSet::new(),
            mouse: V2::ZERO,
            accumulator: 0.0,
            last_frame: Instant::now(),
            hint_timer: 0.0,
            playback: config.replay.clone(),
            playback_frame: 0,
            record,
            record_path,
            ended_for: 0.0,
            total_score: 0,
            total_time: 0.0,
            present_error_reported: false,
            config,
        })
    }

    fn audio_cmd(&self, cmd: AudioCmd) {
        self.audio.send(cmd);
    }

    fn active_scheme(&self) -> Scheme {
        self.playback
            .as_ref()
            .map(|r| r.scheme)
            .unwrap_or(self.config.scheme)
    }

    fn load_level(&mut self, index: usize) -> Result<(), String> {
        let path = self
            .config
            .levels
            .get(index)
            .cloned()
            .ok_or_else(|| format!("no level at index {index}"))?;
        let level = Arc::new(Level::load(&path).map_err(|e| format!("{e}"))?);
        self.level = level;
        self.level_index = index;
        self.restart_level();
        Ok(())
    }

    fn restart_level(&mut self) {
        let scheme = self.active_scheme();
        let seed = self
            .playback
            .as_ref()
            .map(|r| r.seed)
            .unwrap_or(self.config.seed);
        self.runner = SimRunner::with_weapon(self.level.clone(), scheme, seed, self.config.weapon);
        self.playback_frame = 0;
        // Recording only makes sense for keyboard runs.
        self.record = if self.playback.is_some() {
            None
        } else {
            self.record_path.as_ref().map(|_| {
                let mut replay = Replay::new(
                    self.level.source.clone(),
                    self.level.name.clone(),
                    scheme,
                    seed,
                );
                replay.weapon = Some(self.runner.world.ship.loadout.special);
                replay
            })
        };
        self.fx.clear();
        self.audio_cmd(AudioCmd::AllStop);
        self.hint_timer = 0.0;
        self.ended_for = 0.0;
        self.accumulator = 0.0;
        self.camera
            .snap(self.runner.world.ship.body.p, self.level.bounds);
    }

    // ------------------------------------------------------------ input ----

    fn key_down(&self, codes: &[KeyCode]) -> bool {
        codes.iter().any(|c| self.keys.contains(c))
    }

    fn build_frame(&self) -> InputFrame {
        let mut f = InputFrame::default();
        // Wings changes weapon with the turn buttons, but only at a base, and
        // so do we: parked on a pad, the rotation keys step the roster instead
        // of spinning the hull. The simulation refuses to cycle anywhere else,
        // so this is a convenience, not the rule.
        let docked = self.runner.world.docked_on_pad();
        match self.config.scheme {
            Scheme::Classic => {
                let ccw = self.key_down(&[KeyCode::KeyA, KeyCode::ArrowLeft]);
                let cw = self.key_down(&[KeyCode::KeyD, KeyCode::ArrowRight]);
                f.set(BTN_ROTATE_CCW, ccw && !docked);
                f.set(BTN_ROTATE_CW, cw && !docked);
                f.set(BTN_WEAPON_PREV, ccw && docked);
                f.set(BTN_WEAPON_NEXT, cw && docked);
                f.set(
                    BTN_THRUST,
                    self.key_down(&[
                        KeyCode::KeyW,
                        KeyCode::ArrowUp,
                        KeyCode::ShiftLeft,
                        KeyCode::ShiftRight,
                    ]),
                );
                f.set(BTN_FIRE, self.key_down(&[KeyCode::Space]));
                f.set(BTN_SPECIAL, self.key_down(&[KeyCode::KeyF]));
                f.set(BTN_DUMP, self.key_down(&[KeyCode::KeyX]));
                f.set(BTN_BEAM, self.key_down(&[KeyCode::KeyE]));
            }
            Scheme::Modern => {
                let mut dir = V2::ZERO;
                if self.key_down(&[KeyCode::KeyA, KeyCode::ArrowLeft]) {
                    dir.x -= 1.0;
                }
                if self.key_down(&[KeyCode::KeyD, KeyCode::ArrowRight]) {
                    dir.x += 1.0;
                }
                if self.key_down(&[KeyCode::KeyW, KeyCode::ArrowUp]) {
                    dir.y -= 1.0;
                }
                if self.key_down(&[KeyCode::KeyS, KeyCode::ArrowDown]) {
                    dir.y += 1.0;
                }
                f.move_dir = dir.normalized();
                f.set(BTN_THRUST, dir.len_sq() > 0.0);
                f.set(
                    BTN_FIRE,
                    self.mouse_buttons.contains(&MouseButton::Left)
                        || self.key_down(&[KeyCode::Space]),
                );
                f.set(
                    BTN_BEAM,
                    self.mouse_buttons.contains(&MouseButton::Right)
                        || self.key_down(&[KeyCode::KeyE]),
                );
                f.set(BTN_SPECIAL, self.key_down(&[KeyCode::KeyF]));
                f.set(BTN_DUMP, self.key_down(&[KeyCode::KeyX]));
                if docked {
                    f.set(
                        BTN_WEAPON_PREV,
                        self.key_down(&[KeyCode::KeyA, KeyCode::ArrowLeft]),
                    );
                    f.set(
                        BTN_WEAPON_NEXT,
                        self.key_down(&[KeyCode::KeyD, KeyCode::ArrowRight]),
                    );
                }
                let (ox, oy) = self.offset;
                let world = V2::new(
                    (self.mouse.x - ox as f32) * self.scale as f32 + self.camera.offset().x,
                    (self.mouse.y - oy as f32) * self.scale as f32 + self.camera.offset().y,
                );
                f.aim = (world - self.runner.world.ship.body.p).angle();
            }
        }
        f
    }

    fn toggle_scheme(&mut self) {
        self.config.scheme = match self.config.scheme {
            Scheme::Classic => Scheme::Modern,
            Scheme::Modern => Scheme::Classic,
        };
        if self.playback.is_none() {
            self.runner.world.scheme = self.config.scheme;
        }
        self.audio_cmd(AudioCmd::UiSelect);
    }

    fn toggle_recording(&mut self) {
        if let Some(mut rec) = self.record.take() {
            self.runner.finish_replay();
            rec.checksum = Some(self.runner.world.checksum());
            rec.result = self.runner.replay.result.clone();
            let path = self.record_path.clone().unwrap_or_else(default_record_path);
            match rec.save(&path) {
                Ok(()) => {
                    eprintln!("replay saved: {}", path.display());
                    self.fx.show_banner(
                        "REPLAY SAVED",
                        &path.display().to_string(),
                        pal::HUD_ACCENT,
                        2.5,
                    );
                }
                Err(e) => eprintln!("cannot save replay: {e}"),
            }
        } else {
            let path = self
                .config
                .record
                .clone()
                .unwrap_or_else(default_record_path);
            self.record_path = Some(path.clone());
            self.record = Some(Replay::new(
                self.level.source.clone(),
                self.level.name.clone(),
                self.config.scheme,
                self.config.seed,
            ));
            self.fx.show_banner(
                "RECORDING",
                &path.display().to_string(),
                pal::HUD_WARNING,
                2.0,
            );
        }
        self.audio_cmd(AudioCmd::UiBlip);
    }

    /// Key handling is separable from winit so the whole game flow can be driven
    /// without a window; the only shell-specific decision is "should we quit".
    pub fn on_key(&mut self, code: KeyCode, pressed: bool) -> KeyAction {
        if pressed {
            self.keys.insert(code);
        } else {
            self.keys.remove(&code);
        }
        if !pressed {
            return KeyAction::None;
        }
        let confirm = matches!(code, KeyCode::Enter | KeyCode::NumpadEnter);
        match self.screen {
            Screen::Title => match code {
                _ if confirm => self.screen = Screen::Briefing,
                KeyCode::KeyH | KeyCode::F2 => {
                    self.help_return = Screen::Title;
                    self.screen = Screen::Help;
                }
                KeyCode::KeyC => self.toggle_scheme(),
                KeyCode::Escape => return KeyAction::Exit,
                _ => {}
            },
            Screen::Briefing => match code {
                _ if confirm => {
                    self.restart_level();
                    self.screen = Screen::Playing;
                }
                KeyCode::Escape => self.screen = Screen::Title,
                KeyCode::KeyH | KeyCode::F2 => {
                    self.help_return = Screen::Briefing;
                    self.screen = Screen::Help;
                }
                KeyCode::KeyC => self.toggle_scheme(),
                _ => {}
            },
            Screen::Playing => match code {
                KeyCode::KeyP | KeyCode::Escape => {
                    self.screen = Screen::Paused;
                    self.audio_cmd(AudioCmd::AllStop);
                }
                KeyCode::KeyR => self.restart_level(),
                KeyCode::KeyC => self.toggle_scheme(),
                KeyCode::KeyH | KeyCode::F2 => {
                    self.help_return = Screen::Playing;
                    self.screen = Screen::Help;
                }
                KeyCode::F1 => self.toggle_recording(),
                _ if confirm && self.runner.world.state.is_over() => self.advance_or_menu(),
                _ => {}
            },
            Screen::Paused => match code {
                KeyCode::KeyP | KeyCode::Escape => self.screen = Screen::Playing,
                KeyCode::KeyR => {
                    self.restart_level();
                    self.screen = Screen::Playing;
                }
                KeyCode::KeyM => self.screen = Screen::Title,
                KeyCode::KeyC => self.toggle_scheme(),
                KeyCode::KeyH | KeyCode::F2 => {
                    self.help_return = Screen::Paused;
                    self.screen = Screen::Help;
                }
                _ => {}
            },
            Screen::Help => {
                if confirm || code == KeyCode::Escape {
                    self.screen = self.help_return;
                }
            }
            Screen::CampaignComplete => {
                if confirm || code == KeyCode::Escape {
                    self.total_score = 0;
                    self.total_time = 0.0;
                    let _ = self.load_level(0);
                    self.screen = Screen::Title;
                }
            }
        }
        KeyAction::None
    }

    /// Current screen, for automation and tests.
    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn level_index(&self) -> usize {
        self.level_index
    }

    pub fn world(&self) -> &World {
        &self.runner.world
    }

    /// Simulates `ticks` fixed steps without waiting for frames. Used by
    /// automation; the interactive loop drives `update` instead.
    pub fn tick_simulation(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.step_once();
        }
    }

    /// Renders the current screen into `fb`.
    pub fn render(&mut self, fb: &mut Framebuffer) {
        self.draw(fb);
    }

    fn advance_or_menu(&mut self) {
        match self.runner.world.state {
            RunState::Complete { .. } => {
                self.total_score += self.runner.world.score.total();
                self.total_time += self.runner.world.elapsed();
                let next = self.level_index + 1;
                if next < self.config.levels.len() {
                    if let Err(e) = self.load_level(next) {
                        eprintln!("{e}");
                        self.screen = Screen::Title;
                        return;
                    }
                    self.screen = Screen::Briefing;
                } else {
                    self.screen = Screen::CampaignComplete;
                }
                self.audio_cmd(AudioCmd::UiSelect);
            }
            _ => {
                self.restart_level();
                self.screen = Screen::Playing;
            }
        }
    }

    // -------------------------------------------------------------- loop ---

    fn step_once(&mut self) {
        let frame = match &self.playback {
            Some(replay) => {
                let f = replay
                    .frames
                    .get(self.playback_frame)
                    .copied()
                    .unwrap_or_default();
                self.playback_frame += 1;
                f
            }
            None => self.build_frame(),
        };
        self.runner.push(frame);
        self.fx.consume(&self.runner.world);
        for cmd in self.fx.take_audio() {
            self.audio_cmd(cmd);
        }
        self.hint_timer += DT;
    }

    fn update(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;

        if self.screen.simulates() {
            self.accumulator += elapsed;
            let mut steps = 0;
            while self.accumulator >= DT && steps < MAX_STEPS_PER_FRAME {
                self.accumulator -= DT;
                steps += 1;
                self.step_once();
            }
            if steps == MAX_STEPS_PER_FRAME {
                self.accumulator = 0.0;
            }
        }

        let ship = self.runner.world.ship.body.p;
        let vel = self.runner.world.ship.body.v;
        let bounds = self.level.bounds;
        let rate = if self.screen.simulates() { 3.2 } else { 1.4 };
        self.camera.follow(ship, vel, bounds, rate);
        self.camera.tick();
        self.fx.update(&self.runner.world, &mut self.camera);

        if self.runner.world.state.is_over() && self.screen.simulates() {
            self.ended_for += elapsed;
            if self.record.is_some() {
                self.toggle_recording();
            }
        }
    }

    // ------------------------------------------------------------ window ---

    fn layout(&mut self, width: u32, height: u32) {
        let scale = (height / TARGET_HEIGHT).max(1);
        let fb_w = ((width / scale) as i32).clamp(MIN_INTERNAL_WIDTH, MAX_INTERNAL_WIDTH);
        let fb_h = (height / scale).max(180) as i32;
        self.scale = scale;
        if self.fb.width() != fb_w || self.fb.height() != fb_h {
            self.fb = Framebuffer::new(fb_w, fb_h);
        }
        self.camera.resize(fb_w as f32, fb_h as f32);
        self.camera
            .snap(self.runner.world.ship.body.p, self.level.bounds);
        self.offset = (
            (width as i32 - fb_w * scale as i32) / 2,
            (height as i32 - fb_h * scale as i32) / 2,
        );
        if let (Some(surface), Some(window)) = (self.surface.as_mut(), self.window.as_ref()) {
            let size = window.inner_size();
            if let (Some(w), Some(h)) = (
                NonZeroU32::new(size.width.max(1)),
                NonZeroU32::new(size.height.max(1)),
            ) {
                let _ = surface.resize(w, h);
            }
        }
    }

    fn present(&mut self) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        let mut buffer = match surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(e) => {
                if !self.present_error_reported {
                    self.present_error_reported = true;
                    eprintln!("luola: cannot map the window buffer: {e}");
                }
                return;
            }
        };
        blit(
            &self.fb,
            &mut buffer,
            size.width as i32,
            size.height as i32,
            self.scale as i32,
            self.offset,
            pal::SPACE,
        );
        if let Err(e) = buffer.present()
            && !self.present_error_reported
        {
            self.present_error_reported = true;
            eprintln!("luola: present failed: {e}");
        }
        let _ = (w, h);
    }
}

impl ApplicationHandler for Game {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Luolalentely")
            .with_inner_size(LogicalSize::new(1280.0, 720.0));
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("cannot create window: {e}");
                event_loop.exit();
                return;
            }
        };
        match softbuffer::Context::new(window.clone())
            .and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone()).map(|s| (ctx, s)))
        {
            Ok((ctx, surface)) => {
                self.context = Some(ctx);
                self.surface = Some(surface);
                self.window = Some(window.clone());
                let size = window.inner_size();
                self.layout(size.width.max(1), size.height.max(1));
                window.request_redraw();
            }
            Err(e) => {
                eprintln!("cannot create drawing surface: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.audio_cmd(AudioCmd::AllStop);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.layout(size.width.max(1), size.height.max(1));
            }
            WindowEvent::RedrawRequested => {
                self.update();
                let mut fb = std::mem::replace(&mut self.fb, Framebuffer::new(1, 1));
                self.draw(&mut fb);
                self.fb = fb;
                self.present();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let pressed = event.state == ElementState::Pressed;
                    let repeated = event.repeat && pressed;
                    if !repeated && self.on_key(code, pressed) == KeyAction::Exit {
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed {
                    self.mouse_buttons.insert(button);
                } else {
                    self.mouse_buttons.remove(&button);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse = V2::new(position.x as f32, position.y as f32);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

/// Upscales the internal framebuffer into a window-sized buffer at an integer
/// scale, letterboxed in `background`. Pure so the presentation path can be
/// tested without a window.
pub fn blit(
    fb: &Framebuffer,
    dst: &mut [u32],
    dst_w: i32,
    dst_h: i32,
    scale: i32,
    offset: (i32, i32),
    background: u32,
) {
    for px in dst.iter_mut() {
        *px = background;
    }
    if scale < 1 || dst_w <= 0 || dst_h <= 0 {
        return;
    }
    let (ox, oy) = offset;
    let fb_w = fb.width();
    let fb_h = fb.height();
    let src = fb.pixels();
    let stride = dst_w as usize;
    for fy in 0..fb_h {
        for fx in 0..fb_w {
            let c = src[(fy * fb_w + fx) as usize];
            let x0 = ox + fx * scale;
            let y0 = oy + fy * scale;
            for dy in 0..scale {
                let y = y0 + dy;
                if y < 0 || y >= dst_h {
                    continue;
                }
                let row = (y as usize) * stride;
                let mut x = x0.max(0);
                let end = (x0 + scale).min(dst_w);
                while x < end {
                    dst[row + x as usize] = c;
                    x += 1;
                }
            }
        }
    }
}

fn default_record_path() -> PathBuf {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    PathBuf::from(format!("recordings/luola-{secs}.rep"))
}

impl Game {
    fn draw(&mut self, fb: &mut Framebuffer) {
        match self.screen {
            Screen::Title => {
                hud::draw_title(fb, self.runner.world.elapsed(), self.config.levels.len())
            }
            Screen::Help => hud::draw_help(fb),
            Screen::CampaignComplete => {
                hud::draw_campaign_complete(fb, self.total_score, self.total_time)
            }
            Screen::Briefing => hud::draw_briefing(
                fb,
                &self.runner.world,
                self.level_index,
                self.config.levels.len(),
            ),
            Screen::Playing | Screen::Paused => self.draw_flight(fb),
        }
    }

    fn draw_flight(&mut self, fb: &mut Framebuffer) {
        let world: &World = &self.runner.world;
        let camera = &self.camera;
        scene::draw_world(fb, world, camera);
        self.fx.particles.draw(fb, camera);
        hud::draw_vignette(fb);
        hud::draw_radar(fb, world, camera);
        hud::draw_indicators(fb, world, camera);
        let hint = if self.level_index == 0 {
            (1.0 - (self.hint_timer / 24.0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        hud::draw_status(fb, world, hint, self.config.scheme);
        hud::draw_objective_distance(fb, world);
        if self.playback.is_some() {
            hud::text_centered(
                fb,
                fb.width() / 2,
                fb.height() - 40,
                "REPLAY",
                1,
                pal::HUD_WARNING,
            );
        }
        if let Some(banner) = self.fx.banner() {
            hud::draw_banner(fb, banner);
        }
        if self.screen == Screen::Paused {
            hud::draw_pause(fb, world, self.config.scheme, self.hint_timer);
        } else if world.state.is_over() && self.ended_for > 0.7 {
            let failure = match world.state {
                RunState::ShipLost { .. } => Some("THE HULL IS PART OF THE CAVE NOW".to_string()),
                RunState::Failed { reason, .. } => {
                    Some(format!("{} - PRESS R TO TRY AGAIN", reason.label()))
                }
                _ => None,
            };
            let next = self.level_index + 1 < self.config.levels.len();
            hud::draw_results(fb, world, failure.as_deref(), next);
        }
    }
}
