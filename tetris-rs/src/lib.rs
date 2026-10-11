//! NES Tetris (USA), reimplemented in readable Rust.
//!
//! The game's logic is rewritten routine by routine from the annotated disassembly, keeping the
//! original's variables (`vars`) and hardware (`hw`) so that every routine can be checked against
//! the static recompile on real game states. Data tables and graphics are read from the user's
//! ROM (`rom`); the source holds nothing from it.

// A wait that isn't awaited would silently skip its frame.
#![deny(unused_must_use)]

pub mod frame;
pub mod hw;
pub mod layout;
pub mod rom;
mod rom_data;
mod rom_extract;
pub mod shadow;
pub mod vars;

mod controls;
mod endgame;
mod flow;
mod ending;
mod high_scores;
mod input;
mod menus;
mod play;
mod player;
mod render;
mod rng;
pub mod options;
pub mod pc_input;
mod pc_menus;
#[cfg(feature = "versus")]
mod pc_versus;
#[cfg(feature = "versus")]
pub mod versus;
pub mod saved;
mod sound;
mod spawn;
mod sprites;
mod system;
mod vram;

use std::sync::Arc;

pub use frame::{FrameOut, Runner};
pub use hw::{ApuSink, Joypads, Mmc1};
pub use rom::Rom;
pub use saved::Saved;
pub use options::{Config, GameSetup, Options, Pieces, Seed};
pub use pc_input::Keys;
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
    /// The frame exchange with the frontend, when running under `frame::Runner`.
    pub(crate) io: Option<std::rc::Rc<frame::FrameIo>>,
    /// Saved state to restore at power-on (`initRam`); none for the original game.
    pub saved: Option<Saved>,
    /// The app's additions; `Options::original()` for the game as it was.
    pub options: Options,
    /// The game `Runner::start_game` went straight into; none when powered on to the menus.
    pub setup: Option<GameSetup>,
    /// The second seed `Pieces::Identical` deals from.
    pub(crate) piece_seed: u16,
    pub(crate) ui: pc_menus::Ui,
}
