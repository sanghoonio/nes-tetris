//! Play NES Tetris, reimplemented in readable Rust.
//!
//!     cargo run --release -p tetris-rs --bin tetris [path/to/tetris.nes]
//!
//! Your ROM (Tetris (USA)) is built into this program (see `build.rs`), so it runs on its own,
//! e.g. from the app bundle (`macos/bundle.sh`). A ROM path given here is used instead.

use std::sync::Arc;
use tetris_rs::{Rom, Runner};

/// The ROM this program was built with.
const ROM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/tetris.nes"));

struct Readable(Runner);

impl frontend::Game for Readable {
    fn frame(&mut self, buttons: u8) -> (Box<[u8; 256 * 240]>, Vec<f32>) {
        let out = self.0.next_frame(buttons);
        (out.pixels, out.samples)
    }
}

fn main() {
    let file = match std::env::args_os().nth(1) {
        Some(path) => std::fs::read(&path).unwrap_or_else(|e| panic!("can't read ROM {}: {e}", path.display())),
        None => ROM.to_vec(),
    };
    let rom = Arc::new(Rom::new(&file).unwrap_or_else(|e| panic!("ROM: {e}")));
    frontend::run("Tetris", |rate| Readable(Runner::new(rom, Some(rate))));
}
