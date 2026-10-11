//! A game replayed from an input log, as checksums of its RAM every 60 frames: natively, and as a
//! WebAssembly module with plain exports (`alloc`, `replay`) that `tests/wasm.rs` runs under Node.
//! Matching checksums mean a browser player's game and a desktop player's replica of it agree.

use std::sync::Arc;
use tetris_rs::options::Rules;
use tetris_rs::{GameSetup, Pieces, Rom, Runner, Seed};

pub const EVERY: usize = 60;

/// Checksums (FNV-1a of RAM) after every 60th frame of a direct game at `level`, from `seed`, with
/// identical or original pieces, fed `inputs`.
pub fn checksums(rom_file: &[u8], inputs: &[u8], seed: u32, level: u8, identical: bool) -> Vec<u64> {
    let rom = Arc::new(Rom::new(rom_file).expect("Tetris (USA)"));
    let setup = GameSetup {
        b_type: false,
        level,
        height: 0,
        music: 0,
        seed: Seed::from_u32(seed),
        pieces: if identical { Pieces::Identical } else { Pieces::Original },
        rules: Rules::ORIGINAL,
        next_piece: true,
    };
    let mut game = Runner::start_game(rom, None, setup);
    let mut out = vec![];
    for (i, &b) in inputs.iter().enumerate() {
        let frame = game.next_frame(b);
        if (i + 1) % EVERY == 0 {
            out.push(frame.ram.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)));
        }
    }
    out
}

/// Room for `len` bytes, for the host to fill.
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    Box::leak(vec![0u8; len].into_boxed_slice()).as_mut_ptr()
}

/// `checksums` over buffers from `alloc`; returns a pointer to them (`inputs_len / 60` of them).
///
/// # Safety
/// The pointers and lengths must come from `alloc`.
#[no_mangle]
pub unsafe extern "C" fn replay(rom: *const u8, rom_len: usize, inputs: *const u8, inputs_len: usize, seed: u32, level: u32, identical: u32) -> *const u64 {
    let (rom, inputs) = unsafe { (std::slice::from_raw_parts(rom, rom_len), std::slice::from_raw_parts(inputs, inputs_len)) };
    Box::leak(checksums(rom, inputs, seed, level as u8, identical != 0).into_boxed_slice()).as_ptr()
}
