//! Window, keyboard and audio runner shared by the NES Tetris ports.
//!
//! A port implements [`Game`]: given the buttons held this frame, produce the frame (NES palette
//! indices) and the audio generated during it. [`run`] owns the window and drives the game at the
//! NES's 60.0988 Hz.
//!
//! Keys come from [`Bindings`]; the defaults are arrows = D-pad, X = A (rotate clockwise),
//! Z = B (rotate anticlockwise), Enter = Start, Right Shift = Select, Escape = Back. Quit with
//! Cmd+Q or by closing the window.
//!
//! Gamepads work too, in the usual emulator layout: d-pad or left stick, east face button = A,
//! south = B, Start, Select (Back/View/Share).

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

/// The key for each NES button, and for Back (the settings screen's way out).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Bindings {
    pub up: KeyCode,
    pub down: KeyCode,
    pub left: KeyCode,
    pub right: KeyCode,
    pub a: KeyCode,
    pub b: KeyCode,
    pub start: KeyCode,
    pub select: KeyCode,
    pub back: KeyCode,
}

impl Default for Bindings {
    fn default() -> Self {
        Bindings {
            up: KeyCode::ArrowUp,
            down: KeyCode::ArrowDown,
            left: KeyCode::ArrowLeft,
            right: KeyCode::ArrowRight,
            a: KeyCode::KeyX,
            b: KeyCode::KeyZ,
            start: KeyCode::Enter,
            select: KeyCode::ShiftRight,
            back: KeyCode::Escape,
        }
    }
}

impl Bindings {
    /// The joypad bit `key` is bound to, if any.
    fn button(&self, key: KeyCode) -> Option<u8> {
        [
            (self.up, BTN_UP),
            (self.down, BTN_DOWN),
            (self.left, BTN_LEFT),
            (self.right, BTN_RIGHT),
            (self.a, BTN_A),
            (self.b, BTN_B),
            (self.start, BTN_START),
            (self.select, BTN_SELECT),
        ]
        .into_iter()
        .find(|&(k, _)| k == key)
        .map(|(_, bit)| bit)
    }
}

/// How the window plays the game.
#[derive(Clone, Debug)]
pub struct Options {
    pub bindings: Bindings,
    /// Output volume, 0.0..=1.0.
    pub volume: f32,
    pub fullscreen: bool,
    /// Scale by whole numbers only, so every NES pixel is the same size (sharper, with a wider
    /// border).
    pub integer_scale: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { bindings: Bindings::default(), volume: 1.0, fullscreen: false, integer_scale: false }
    }
}

pub trait Game {
    /// Run one frame holding `buttons`; `key` is a key pressed since the last frame (for
    /// rebinding). Return the finished frame and its audio (mono, [-1, 1]).
    fn frame(&mut self, buttons: u8, key: Option<KeyCode>) -> (Box<[u8; WIDTH * HEIGHT]>, Vec<f32>);

    /// Checked after each frame: close the window.
    fn quit(&self) -> bool {
        false
    }

    /// Checked after each frame: new bindings or volume to use from now on.
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
        buttons: 0,
        next_frame: Instant::now(),
        frame: Box::new([0x0F; WIDTH * HEIGHT]),
        audio,
        bindings: options.bindings,
        key: None,
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
    buttons: u8,
    next_frame: Instant,
    frame: Box<[u8; WIDTH * HEIGHT]>,
    audio: Audio,
    bindings: Bindings,
    /// A key pressed since the last frame.
    key: Option<KeyCode>,
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
            .with_inner_size(winit::dpi::LogicalSize::new(WIDTH as u32 * SCALE, HEIGHT as u32 * SCALE));
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
                if event.state == ElementState::Pressed && !event.repeat {
                    self.key.get_or_insert(code);
                }
                let Some(bit) = self.bindings.button(code) else { return };
                match event.state {
                    ElementState::Pressed => self.buttons |= bit,
                    ElementState::Released => self.buttons &= !bit,
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
                let buttons = self.buttons | self.pad_buttons();
                let (frame, samples) = self.game.frame(buttons, self.key.take());
                self.frame = frame;
                self.audio.push(&samples);
                self.next_frame += FRAME_TIME;
                if let Some(options) = self.game.take_options() {
                    self.bindings = options.bindings;
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
    fn apply_fullscreen(&self) {
        if let Some(w) = &self.window {
            w.set_fullscreen(self.fullscreen.then_some(winit::window::Fullscreen::Borderless(None)));
        }
    }

    /// Buttons held on any connected gamepad.
    fn pad_buttons(&mut self) -> u8 {
        use gilrs::{Axis, Button};
        let Some(pads) = &mut self.pads else { return 0 };
        while pads.next_event().is_some() {}
        let mut held = 0;
        for (_, pad) in pads.gamepads() {
            let stick = |axis, sign: f32| pad.value(axis) * sign > 0.5;
            for (on, bit) in [
                (pad.is_pressed(Button::DPadUp) || stick(Axis::LeftStickY, 1.0), BTN_UP),
                (pad.is_pressed(Button::DPadDown) || stick(Axis::LeftStickY, -1.0), BTN_DOWN),
                (pad.is_pressed(Button::DPadLeft) || stick(Axis::LeftStickX, -1.0), BTN_LEFT),
                (pad.is_pressed(Button::DPadRight) || stick(Axis::LeftStickX, 1.0), BTN_RIGHT),
                (pad.is_pressed(Button::East), BTN_A),
                (pad.is_pressed(Button::South), BTN_B),
                (pad.is_pressed(Button::Start), BTN_START),
                (pad.is_pressed(Button::Select), BTN_SELECT),
            ] {
                if on {
                    held |= bit;
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
        let mut scale = (w as f32 / WIDTH as f32).min(h as f32 / HEIGHT as f32);
        if self.integer_scale && scale >= 1.0 {
            scale = scale.floor();
        }
        let (ow, oh) = ((WIDTH as f32 * scale) as usize, (HEIGHT as f32 * scale) as usize);
        let (x0, y0) = ((w - ow) / 2, (h - oh) / 2);
        buf.fill(0);
        for y in 0..oh {
            let sy = (y as f32 / scale) as usize;
            let src = &self.frame[sy.min(HEIGHT - 1) * WIDTH..][..WIDTH];
            let dst = &mut buf[(y0 + y) * w + x0..][..ow];
            for (x, px) in dst.iter_mut().enumerate() {
                let sx = ((x as f32 / scale) as usize).min(WIDTH - 1);
                *px = PALETTE[(src[sx] & 0x3F) as usize];
            }
        }
        buf.present().expect("present");
    }
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
                let mut q = q.lock().unwrap();
                for frame in data.chunks_mut(channels) {
                    let s = q.pop_front().unwrap_or(0.0);
                    frame.fill(s);
                }
            },
            |e| eprintln!("audio: {e}"),
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
