//! Play the statically recompiled NES Tetris.
//!
//!     cargo run --release -p recomp --bin tetris-recomp [path/to/tetris.nes]
//!
//! The ROM defaults to $TETRIS_ROM, then tetris_nes/tetris.nes.

use recomp::stepper::Stepper;
use std::path::PathBuf;

struct Recompiled(Stepper);

impl frontend::Game for Recompiled {
    fn frame(&mut self, input: &frontend::Input) -> (Box<[u8; 256 * 240]>, Vec<f32>) {
        let snap = self.0.next_frame(buttons(input), true);
        (snap.pixels.expect("rendered"), snap.samples)
    }
}

/// The keys as an NES controller: arrows, X = A, Z = B, Enter = Start, Right Shift = Select;
/// gamepads in the usual emulator layout.
fn buttons(input: &frontend::Input) -> u8 {
    use frontend::*;
    let map = [
        ("ArrowUp", BTN_UP),
        ("ArrowDown", BTN_DOWN),
        ("ArrowLeft", BTN_LEFT),
        ("ArrowRight", BTN_RIGHT),
        ("KeyX", BTN_A),
        ("KeyZ", BTN_B),
        ("Enter", BTN_START),
        ("ShiftRight", BTN_SELECT),
        ("PadUp", BTN_UP),
        ("PadDown", BTN_DOWN),
        ("PadLeft", BTN_LEFT),
        ("PadRight", BTN_RIGHT),
        ("PadEast", BTN_A),
        ("PadSouth", BTN_B),
        ("PadStart", BTN_START),
        ("PadSelect", BTN_SELECT),
    ];
    input.held.iter().filter_map(|k| map.iter().find(|(n, _)| n == k)).fold(0, |b, &(_, bit)| b | bit)
}

fn main() {
    let rom_path = std::env::args_os()
        .nth(1)
        .or_else(|| std::env::var_os("TETRIS_ROM"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"));
    let rom = std::fs::read(&rom_path)
        .unwrap_or_else(|e| panic!("can't read ROM {}: {e}", rom_path.display()));
    frontend::run("Tetris (recompiled)", frontend::Options::default(), |rate| Recompiled(Stepper::with_audio(&rom, Some(rate))));
}
