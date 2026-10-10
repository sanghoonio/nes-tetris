//! Play NES Tetris, reimplemented in readable Rust.
//!
//!     cargo run --release -p tetris-rs --bin tetris [path/to/tetris.nes]
//!
//! Your ROM (Tetris (USA)) is built into this program (see `build.rs`), so it runs on its own,
//! e.g. from the app bundle (`macos/bundle.sh`). A ROM path given here is used instead.
//!
//! Settings, the last game's choices and the high scores are kept in `settings.toml`
//! (`settings.rs`), saved as they change.

mod settings;

use std::sync::Arc;
use settings::{LastGame, Settings, Table};
use tetris_rs::{Rom, Runner, Saved};

/// The ROM this program was built with.
const ROM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/tetris.nes"));

/// `gameMode` once a game (not the demo) is under way.
const MODE_PLAY: u8 = 4;
/// Where `initRam` leaves its marker once the high score table is set up.
const INIT_MAGIC: (usize, [u8; 5]) = (0x750, [0x12, 0x34, 0x56, 0x78, 0x9A]);

struct Readable {
    runner: Runner,
    settings: Settings,
}

impl Readable {
    /// Save whatever this frame changed: the high score table, or the choices a game started with.
    fn save_changes(&mut self, ram: &[u8; 0x800]) {
        let (at, magic) = INIT_MAGIC;
        if ram[at..at + 5] != magic {
            return; // still powering on
        }
        let saved = Saved::from_ram(ram);
        let mut next = self.settings.clone();
        if self.settings.save_high_scores {
            next.high_scores = saved.high_scores.as_ref().map(Table::from_game);
        }
        if ram[0xC0] == MODE_PLAY {
            next.last_game = LastGame::from_game(&saved.choices);
        }
        if next != self.settings {
            self.settings = next;
            self.settings.save();
        }
    }
}

impl frontend::Game for Readable {
    fn frame(&mut self, buttons: u8) -> (Box<[u8; 256 * 240]>, Vec<f32>) {
        let out = self.runner.next_frame(buttons);
        self.save_changes(&out.ram);
        (out.pixels, out.samples)
    }
}

fn main() {
    let file = match std::env::args_os().nth(1) {
        Some(path) => std::fs::read(&path).unwrap_or_else(|e| panic!("can't read ROM {}: {e}", path.display())),
        None => ROM.to_vec(),
    };
    let rom = Arc::new(Rom::new(&file).unwrap_or_else(|e| panic!("ROM: {e}")));
    let settings = Settings::load();
    // Write the file on first launch so it's there to edit.
    if !Settings::dir().join("settings.toml").exists() {
        settings.save();
    }
    let options = frontend::Options { bindings: settings.keys.clone(), volume: settings.volume };
    let saved = settings.saved();
    frontend::run("Tetris", options, |rate| Readable {
        runner: Runner::with_saved(rom, Some(rate), Some(saved)),
        settings,
    });
}
