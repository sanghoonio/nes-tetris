//! The app for the web page: `client::App` (the game, its menus, settings, VERSUS) behind a
//! small JavaScript interface. The page supplies the ROM file, the keys by name, the time and
//! the server; it gets back each frame's picture as RGBA, its sound, and the settings to keep
//! whenever they change (JSON).

use std::sync::Arc;

use client::{App, Input, Settings};
use tetris_rs::options::{key_label, ACTIONS};
use tetris_rs::Rom;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Engine {
    app: App,
    rgba: Vec<u8>,
    width: usize,
    height: usize,
    samples: Vec<f32>,
}

#[wasm_bindgen]
impl Engine {
    /// The game from power-on with the player's ROM file, the settings kept from last time (JSON,
    /// or nothing for the defaults), the sound's sample rate, the room server (`wss://...`), and a
    /// random seed. Fails if the file isn't Tetris (USA).
    #[wasm_bindgen(constructor)]
    pub fn new(rom: &[u8], settings: Option<String>, sample_rate: u32, server: String, seed: f64) -> Result<Engine, JsError> {
        let rom = Rom::new(rom).map_err(|e| JsError::new(&e.to_string()))?;
        let settings: Settings = settings.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let mut app = App::new(Arc::new(rom), Some(sample_rate), settings, seed.to_bits());
        app.set_versus(Some(server), None);
        Ok(Engine { app, rgba: vec![], width: 256, height: 240, samples: vec![] })
    }

    /// One frame: the keys held and newly pressed (by `KeyboardEvent.code`, and `PadSouth`-style
    /// names for gamepads), whether the page has the keyboard, and the time in milliseconds.
    pub fn frame(&mut self, held: Vec<String>, pressed: Vec<String>, unfocused: bool, now: f64) {
        let (pixels, samples) = self.app.frame(&Input { held, pressed, unfocused }, now as u64);
        self.height = self.app.height();
        self.width = pixels.len() / self.height;
        self.rgba.clear();
        self.rgba.reserve(pixels.len() * 4);
        for p in pixels {
            let c = ppu::PALETTE[(p & 0x3F) as usize];
            self.rgba.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]);
        }
        self.samples.extend(samples);
    }

    /// Whether a game is running that losing focus would pause, and hasn't yet (`App::pausable`).
    pub fn pausable(&self) -> bool {
        self.app.pausable()
    }

    /// The last frame's picture: RGBA, `width()` by `height()` (taller during a VERSUS match).
    pub fn rgba(&self) -> Vec<u8> {
        self.rgba.clone()
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// The sound since last asked.
    pub fn samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.samples)
    }

    /// The settings to keep, if they changed since last asked (JSON).
    pub fn changed_settings(&mut self) -> Option<String> {
        self.app.take_dirty().then(|| serde_json::to_string(self.app.settings()).expect("settings serialise"))
    }

    /// Whether the settings screen changed the volume, fullscreen or scaling since last asked.
    pub fn display_changed(&mut self) -> bool {
        self.app.take_config_changed()
    }

    pub fn volume(&self) -> f32 {
        self.app.settings().volume
    }

    pub fn fullscreen(&self) -> bool {
        self.app.settings().fullscreen
    }

    pub fn integer_scale(&self) -> bool {
        self.app.settings().integer_scale
    }

    /// Whether in a VERSUS room, where the game can't stop (the others play on).
    pub fn online(&self) -> bool {
        self.app.online()
    }

    /// EXIT was chosen.
    pub fn quit(&self) -> bool {
        self.app.quit()
    }

    /// The game's keys as set, for the page's help: `[action, key]` pairs (JSON).
    pub fn controls(&self) -> String {
        let keys = self.app.settings().config().keys;
        let pairs: Vec<[String; 2]> = ACTIONS.iter().zip(keys).map(|(a, k)| [a.to_string(), key_label(&k)]).collect();
        serde_json::to_string(&pairs).expect("controls serialise")
    }
}
