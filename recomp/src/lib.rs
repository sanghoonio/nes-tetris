//! NES Tetris, statically recompiled from 6502 to Rust.
//!
//! The generated code (`generated/tetris.rs`, produced by `gen/gen.py` from the ROM) is the
//! game logic; `machine`, `cpu` and `ppu` are the hardware it runs on. The ROM is still needed at
//! run time for data tables and CHR graphics.

pub mod cpu;
pub mod interp;
pub mod machine;
pub mod ppu;
pub mod stepper;

#[path = "../generated/tetris.rs"]
#[allow(unused_variables, unused_mut, unreachable_code, non_snake_case, clippy::all)]
pub mod generated;
