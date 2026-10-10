//! Play NES Tetris, reimplemented in readable Rust.
//!
//!     cargo run --release -p tetris-rs --bin tetris [path/to/tetris.nes]
//!
//! The ROM (Tetris (USA)) defaults to $TETRIS_ROM, then tetris_nes/tetris.nes. It's checked by
//! hash and read for the game's data and graphics; nothing from it is built into this program.

use std::path::PathBuf;
use std::sync::Arc;
use tetris_rs::{Rom, Runner};

struct Readable(Runner);

impl frontend::Game for Readable {
    fn frame(&mut self, buttons: u8) -> (Box<[u8; 256 * 240]>, Vec<f32>) {
        let out = self.0.next_frame(buttons);
        (out.pixels, out.samples)
    }
}

fn main() {
    let path = std::env::args_os()
        .nth(1)
        .or_else(|| std::env::var_os("TETRIS_ROM"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"));
    let file = std::fs::read(&path).unwrap_or_else(|e| panic!("can't read ROM {}: {e}", path.display()));
    let rom = Arc::new(Rom::new(&file).unwrap_or_else(|e| panic!("{}: {e}", path.display())));
    frontend::run("Tetris", |rate| Readable(Runner::new(rom, Some(rate))));
}
