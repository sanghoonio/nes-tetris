//! Play the statically recompiled NES Tetris.
//!
//!     cargo run --release -p recomp --bin tetris-recomp [path/to/tetris.nes]
//!
//! The ROM defaults to $TETRIS_ROM, then tetris_nes/tetris.nes.

use recomp::stepper::Stepper;
use std::path::PathBuf;

struct Recompiled(Stepper);

impl frontend::Game for Recompiled {
    fn frame(&mut self, buttons: u8) -> (Box<[u8; 256 * 240]>, Vec<f32>) {
        let snap = self.0.next_frame(buttons, true);
        (snap.pixels.expect("rendered"), snap.samples)
    }
}

fn main() {
    let rom_path = std::env::args_os()
        .nth(1)
        .or_else(|| std::env::var_os("TETRIS_ROM"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"));
    let rom = std::fs::read(&rom_path)
        .unwrap_or_else(|e| panic!("can't read ROM {}: {e}", rom_path.display()));
    frontend::run("Tetris (recompiled)", |rate| Recompiled(Stepper::with_audio(&rom, Some(rate))));
}
