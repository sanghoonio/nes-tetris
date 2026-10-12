//! Window, keyboard and audio runner shared by the NES Tetris ports.
//!
//! A port implements [`Game`]: given the buttons held this frame, produce the frame (NES palette
//! indices) and the audio generated during it. [`run`] owns the window and drives the game at the
//! NES's 60.0988 Hz.
//!
//! The window reports the keys held and pressed each frame by name ([`Input`]), gamepads
//! included; what they mean is the game's business. Quit with Cmd+Q (macOS) or by closing the window
//! (or when the game asks).

use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
pub use winit::keyboard::KeyCode;
use winit::window::{Window, WindowId};

pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 240;
const SCALE: u32 = 3;
const FRAME_TIME: Duration = Duration::from_nanos(16_639_267); // 1 / 60.0988 Hz

/// Joypad bits, first-shifted-out first (the layout the game reads).
pub const BTN_A: u8 = 0x80;
pub const BTN_B: u8 = 0x40;
pub const BTN_SELECT: u8 = 0x20;
pub const BTN_START: u8 = 0x10;
pub const BTN_UP: u8 = 0x08;
pub const BTN_DOWN: u8 = 0x04;
pub const BTN_LEFT: u8 = 0x02;
pub const BTN_RIGHT: u8 = 0x01;

/// The keys this frame, by name: winit `KeyCode` names (`KeyX`, `ArrowUp`, `Enter`) and
/// `Pad*` for gamepad buttons (`PadSouth`, `PadEast`, `PadNorth`, `PadWest`, `PadStart`,
/// `PadSelect`, and `PadUp`/`PadDown`/`PadLeft`/`PadRight` for the d-pad or left stick).
#[derive(Clone, Debug, Default)]
pub struct Input {
    pub held: Vec<String>,
    /// Pressed since the last frame, in order.
    pub pressed: Vec<String>,
    /// The window isn't focused (the player switched to another window).
    pub unfocused: bool,
}

/// How the window plays the game.
#[derive(Clone, Debug)]
pub struct Options {
    /// Output volume, 0.0..=1.0.
    pub volume: f32,
    pub fullscreen: bool,
    /// Scale by whole numbers only, so every NES pixel is the same size (sharper, with a wider
    /// border).
    pub integer_scale: bool,
    /// The picture's width in pixels at the start (it's always `HEIGHT` high): `WIDTH` for one
    /// NES screen, more for several side by side. A game can change it by returning a picture of
    /// another width; the window then resizes to keep its scale.
    pub width: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { volume: 1.0, fullscreen: false, integer_scale: false, width: WIDTH }
    }
}

pub trait Game {
    /// Run one frame with `input`. Return the finished picture (NES palette indices, any width ×
    /// `HEIGHT`, row by row) and its audio (mono, [-1, 1]).
    fn frame(&mut self, input: &Input) -> (Vec<u8>, Vec<f32>);

    /// Checked after each frame: close the window.
    fn quit(&self) -> bool {
        false
    }

    /// Checked after each frame: new volume or display settings to use from now on.
    fn take_options(&mut self) -> Option<Options> {
        None
    }
}

/// The 2C02 palette as 0x00RRGGBB (the common nesdev palette, as also used by tetanes-core).
#[rustfmt::skip]
pub const PALETTE: [u32; 64] = [
    0x545454, 0x001E74, 0x081090, 0x300088, 0x440064, 0x5C0030, 0x540400, 0x3C1800,
    0x202A00, 0x083A00, 0x004000, 0x003C00, 0x00323C, 0x000000, 0x000000, 0x000000,
    0x989698, 0x084CC4, 0x3032EC, 0x5C1EE4, 0x8814B0, 0xA01464, 0x982220, 0x783C00,
    0x545A00, 0x287200, 0x087C00, 0x007628, 0x006678, 0x000000, 0x000000, 0x000000,
    0xECEEEC, 0x4C9AEC, 0x787CEC, 0xB062EC, 0xE454EC, 0xEC58B4, 0xEC6A64, 0xD48820,
    0xA0AA00, 0x74C400, 0x4CD020, 0x38CC6C, 0x38B4CC, 0x3C3C3C, 0x000000, 0x000000,
    0xECEEEC, 0xA8CCEC, 0xBCBCEC, 0xD4B2EC, 0xECAEEC, 0xECAED4, 0xECB4B0, 0xE4C490,
    0xCCD278, 0xB4DE78, 0xA8E290, 0x98E2B4, 0xA0D6E4, 0xA0A2A0, 0x000000, 0x000000,
];

/// Open the window and run. `make_game` is given the audio output's sample rate.
pub fn run<G: Game + 'static>(title: &str, options: Options, make_game: impl FnOnce(u32) -> G) {
    let mut audio = Audio::start();
    audio.volume = options.volume.clamp(0.0, 1.0);
    let game = make_game(audio.sample_rate);
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App {
        title: title.to_string(),
        window: None,
        surface: None,
        game,
        held: Vec::new(),
        pressed: Vec::new(),
        pad_held: Vec::new(),
        focused: true,
        next_frame: Instant::now(),
        frame: vec![0x0F; options.width * HEIGHT],
        width: options.width,
        audio,
        fullscreen: options.fullscreen,
        integer_scale: options.integer_scale,
        pads: gilrs::Gilrs::new().map_err(|e| eprintln!("gamepads unavailable: {e}")).ok(),
    };
    event_loop.run_app(&mut app).expect("event loop");
}

struct App<G> {
    title: String,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    game: G,
    /// Keys held, and pressed since the last frame.
    held: Vec<KeyCode>,
    pressed: Vec<String>,
    /// Gamepad buttons held last frame, by name.
    pad_held: Vec<String>,
    focused: bool,
    next_frame: Instant,
    frame: Vec<u8>,
    width: usize,
    audio: Audio,
    fullscreen: bool,
    integer_scale: bool,
    pads: Option<gilrs::Gilrs>,
}

impl<G: Game> ApplicationHandler for App<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(self.width as u32 * SCALE, HEIGHT as u32 * SCALE));
        let window = Rc::new(event_loop.create_window(attrs).expect("window"));
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&context, window.clone()).expect("surface"));
        self.window = Some(window);
        self.apply_fullscreen();
        self.next_frame = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                match event.state {
                    ElementState::Pressed if !event.repeat => {
                        self.pressed.push(key_name(code));
                        if !self.held.contains(&code) {
                            self.held.push(code);
                        }
                    }
                    ElementState::Pressed => {}
                    ElementState::Released => self.held.retain(|&k| k != code),
                }
            }
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                // Releases while away never arrive.
                if !focused {
                    self.held.clear();
                }
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_frame {
            // Catch up at most a few frames after a stall rather than fast-forwarding.
            if now - self.next_frame > FRAME_TIME * 4 {
                self.next_frame = now;
            }
            while self.next_frame <= now {
                let input = self.input();
                let (frame, samples) = self.game.frame(&input);
                let width = frame.len() / HEIGHT;
                self.frame = frame;
                if width != self.width {
                    self.width = width;
                    self.fit_window();
                }
                self.audio.push(&samples);
                self.next_frame += FRAME_TIME;
                if let Some(options) = self.game.take_options() {
                    self.audio.volume = options.volume.clamp(0.0, 1.0);
                    self.integer_scale = options.integer_scale;
                    if options.fullscreen != self.fullscreen {
                        self.fullscreen = options.fullscreen;
                        self.apply_fullscreen();
                    }
                }
                if self.game.quit() {
                    event_loop.exit();
                    return;
                }
            }
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

impl<G> App<G> {
    /// After the picture's width changed: resize the window to keep its height (so its scale),
    /// narrowing everything if that would be wider than the display. Fullscreen or maximised
    /// windows stay as they are; the picture fits inside.
    fn fit_window(&self) {
        let Some(w) = &self.window else { return };
        if self.fullscreen || w.is_maximized() {
            return;
        }
        let size = w.inner_size();
        let (mut width, mut height) = (self.width as f64 * size.height as f64 / HEIGHT as f64, size.height as f64);
        if let Some(monitor) = w.current_monitor() {
            let room = monitor.size().width as f64 * 0.95;
            if width > room {
                height *= room / width;
                width = room;
            }
        }
        let _ = w.request_inner_size(winit::dpi::PhysicalSize::new(width as u32, height as u32));
    }

    fn apply_fullscreen(&self) {
        if let Some(w) = &self.window {
            w.set_fullscreen(self.fullscreen.then_some(winit::window::Fullscreen::Borderless(None)));
        }
    }

    /// This frame's keys: the keyboard's, plus gamepad buttons by name.
    fn input(&mut self) -> Input {
        let pads = self.pad_names();
        let mut pressed = std::mem::take(&mut self.pressed);
        pressed.extend(pads.iter().filter(|p| !self.pad_held.contains(p)).cloned());
        let mut held: Vec<String> = self.held.iter().map(|&k| key_name(k)).collect();
        held.extend(pads.iter().cloned());
        self.pad_held = pads;
        Input { held, pressed, unfocused: !self.focused }
    }

    /// Buttons held on any connected gamepad.
    fn pad_names(&mut self) -> Vec<String> {
        use gilrs::{Axis, Button};
        let Some(pads) = &mut self.pads else { return vec![] };
        while pads.next_event().is_some() {}
        let mut held = vec![];
        for (_, pad) in pads.gamepads() {
            let stick = |axis, sign: f32| pad.value(axis) * sign > 0.5;
            for (on, name) in [
                (pad.is_pressed(Button::DPadUp) || stick(Axis::LeftStickY, 1.0), "PadUp"),
                (pad.is_pressed(Button::DPadDown) || stick(Axis::LeftStickY, -1.0), "PadDown"),
                (pad.is_pressed(Button::DPadLeft) || stick(Axis::LeftStickX, -1.0), "PadLeft"),
                (pad.is_pressed(Button::DPadRight) || stick(Axis::LeftStickX, 1.0), "PadRight"),
                (pad.is_pressed(Button::South), "PadSouth"),
                (pad.is_pressed(Button::East), "PadEast"),
                (pad.is_pressed(Button::North), "PadNorth"),
                (pad.is_pressed(Button::West), "PadWest"),
                (pad.is_pressed(Button::Start), "PadStart"),
                (pad.is_pressed(Button::Select), "PadSelect"),
            ] {
                if on && !held.iter().any(|h| h == name) {
                    held.push(name.to_string());
                }
            }
        }
        held
    }

    fn draw(&mut self) {
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else { return };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        surface.resize(w, h).expect("resize");
        let mut buf = surface.buffer_mut().expect("buffer");
        let (w, h) = (size.width as usize, size.height as usize);
        // Nearest-neighbour scale to fit the window (whole numbers only with `integer_scale`),
        // letterboxed to keep the aspect.
        let width = self.width;
        let mut scale = (w as f32 / width as f32).min(h as f32 / HEIGHT as f32);
        if self.integer_scale && scale >= 1.0 {
            scale = scale.floor();
        }
        let (ow, oh) = ((width as f32 * scale) as usize, (HEIGHT as f32 * scale) as usize);
        let (x0, y0) = ((w - ow) / 2, (h - oh) / 2);
        buf.fill(0);
        for y in 0..oh {
            let sy = (y as f32 / scale) as usize;
            let src = &self.frame[sy.min(HEIGHT - 1) * width..][..width];
            let dst = &mut buf[(y0 + y) * w + x0..][..ow];
            for (x, px) in dst.iter_mut().enumerate() {
                let sx = ((x as f32 / scale) as usize).min(width - 1);
                *px = PALETTE[(src[sx] & 0x3F) as usize];
            }
        }
        buf.present().expect("present");
    }
}

/// A key's name: its winit `KeyCode` variant (`KeyX`, `ArrowUp`, `ShiftRight`).
pub fn key_name(code: KeyCode) -> String {
    format!("{code:?}")
}

/// Audio output fed from a small queue; underruns play silence.
struct Audio {
    sample_rate: u32,
    volume: f32,
    queue: Arc<Mutex<VecDeque<f32>>>,
    _stream: Option<cpal::Stream>,
}

impl Audio {
    fn start() -> Self {
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            eprintln!("no audio output device; running silent");
            return Self { sample_rate: 44100, volume: 1.0, queue, _stream: None };
        };
        let config: cpal::StreamConfig = match device.default_output_config() {
            Ok(c) => c.into(),
            Err(e) => {
                eprintln!("audio config: {e}; running silent");
                return Self { sample_rate: 44100, volume: 1.0, queue, _stream: None };
            }
        };
        let channels = config.channels as usize;
        let sample_rate = config.sample_rate as u32;
        let q = queue.clone();
        let stream = device.build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // Never wait on the game's thread here: if it's pushing, this buffer is silent
                // (and the samples play next time) rather than late.
                let Ok(mut q) = q.try_lock() else {
                    data.fill(0.0);
                    return;
                };
                for frame in data.chunks_mut(channels) {
                    let s = q.pop_front().unwrap_or(0.0);
                    frame.fill(s);
                }
            },
            {
                // The device reports each glitch (a buffer it couldn't fill in time); say so once.
                let mut glitched = false;
                move |e: cpal::Error| {
                    if e.kind() != cpal::ErrorKind::Xrun {
                        eprintln!("audio: {e}");
                    } else if !std::mem::replace(&mut glitched, true) {
                        eprintln!("audio: the output fell behind (a glitch); later ones aren't reported");
                    }
                }
            },
            None,
        );
        match stream {
            Ok(s) => {
                if let Err(e) = s.play() {
                    eprintln!("audio play: {e}");
                }
                Self { sample_rate, volume: 1.0, queue, _stream: Some(s) }
            }
            Err(e) => {
                eprintln!("audio stream: {e}; running silent");
                Self { sample_rate, volume: 1.0, queue, _stream: None }
            }
        }
    }

    fn push(&self, samples: &[f32]) {
        let mut q = self.queue.lock().unwrap();
        q.extend(samples.iter().map(|s| s * self.volume));
        // Keep latency under ~100 ms if the game runs ahead of the device.
        let max = self.sample_rate as usize / 10;
        if q.len() > max {
            let excess = q.len() - max;
            q.drain(..excess);
        }
    }
}
