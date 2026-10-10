//! NES Tetris (USA), reimplemented in readable Rust.
//!
//! The game's logic is rewritten routine by routine from the annotated disassembly, keeping the
//! original's variables (`vars`) and hardware (`hw`) so that every routine can be checked against
//! the static recompile on real game states. Data tables and graphics are read from the user's
//! ROM (`rom`); nothing from it is embedded.

pub mod hw;
pub mod layout;
pub mod rom;
pub mod shadow;
pub mod vars;

mod controls;
mod ending;
mod high_scores;
mod input;
mod menus;
mod play;
mod player;
mod render;
mod rng;
mod sound;
mod spawn;
mod sprites;
mod system;
mod vram;

use std::sync::Arc;

pub use hw::{ApuSink, Joypads, Mmc1};
pub use rom::Rom;
pub use vars::Vars;

/// Joypad bits as the game sees them (first shifted out = A).
pub mod buttons {
    pub const A: u8 = 0x80;
    pub const B: u8 = 0x40;
    pub const SELECT: u8 = 0x20;
    pub const START: u8 = 0x10;
    pub const UP: u8 = 0x08;
    pub const DOWN: u8 = 0x04;
    pub const LEFT: u8 = 0x02;
    pub const RIGHT: u8 = 0x01;
    pub const DPAD: u8 = UP | DOWN | LEFT | RIGHT;
}

/// The whole machine as the game sees it.
pub struct Game {
    pub rom: Arc<Rom>,
    pub vars: Vars,
    pub ppu: ppu::Ppu,
    pub mmc1: Mmc1,
    pub joypads: Joypads,
    pub apu: Box<dyn ApuSink>,
}
