//! The reference: the original ROM running in tetanes-core, stopped at each NMI entry.

use std::path::PathBuf;
use tetanes_core::{
    control_deck::{Config, ControlDeck, HeadlessMode},
    input::{JoypadBtn, Player},
    memory::RamState,
};

pub fn rom_path() -> PathBuf {
    std::env::var_os("TETRIS_ROM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"))
}

pub fn rom_bytes() -> Vec<u8> {
    std::fs::read(rom_path()).expect("ROM not found; set TETRIS_ROM or place tetris_nes/tetris.nes")
}

pub struct Reference {
    deck: ControlDeck,
    nmi_addr: u16,
}

impl Reference {
    pub fn new() -> Self {
        Self::with_video(false)
    }

    pub fn with_video(video: bool) -> Self {
        let headless = if video { HeadlessMode::NO_AUDIO } else { HeadlessMode::NO_AUDIO | HeadlessMode::NO_VIDEO };
        let cfg = Config::default().with_ram_state(RamState::AllZeros).with_headless_mode(headless);
        let mut deck = ControlDeck::with_config(cfg);
        let rom = rom_bytes();
        deck.load_rom("tetris.nes", &mut rom.as_slice()).expect("load ROM");
        let nmi_addr = u16::from_le_bytes([rom[16 + 0x7FFA], rom[16 + 0x7FFB]]);
        Self { deck, nmi_addr }
    }

    /// Hold `buttons` (bit order A, B, Select, Start, Up, Down, Left, Right, as the NES shifts
    /// them out) and run until the CPU next enters the NMI handler.
    pub fn run_to_nmi(&mut self, buttons: u8) {
        let pad = self.deck.joypad_mut(Player::One);
        for (bit, btn) in [
            JoypadBtn::A,
            JoypadBtn::B,
            JoypadBtn::Select,
            JoypadBtn::Start,
            JoypadBtn::Up,
            JoypadBtn::Down,
            JoypadBtn::Left,
            JoypadBtn::Right,
        ]
        .into_iter()
        .enumerate()
        {
            pad.set_button(btn, buttons & (0x80 >> bit) != 0);
        }
        // Step off the handler's first instruction if we're sitting on it.
        if self.deck.bus().cpu.pc == self.nmi_addr {
            self.deck.clock_instr().expect("cpu");
        }
        while self.deck.bus().cpu.pc != self.nmi_addr {
            self.deck.clock_instr().expect("cpu");
        }
    }

    pub fn wram(&self) -> &[u8; 2048] {
        self.deck.wram()
    }

    pub fn poke(&mut self, addr: u16, v: u8) {
        self.deck.wram_mut()[addr as usize & 0x7FF] = v;
    }

    /// The last finished frame as NES palette indices.
    pub fn pixels(&self) -> Vec<u8> {
        self.deck.frame_buffer_raw().iter().map(|&p| (p & 0x3F) as u8).collect()
    }

    pub fn sp(&self) -> u8 {
        self.deck.bus().cpu.sp
    }

    pub fn cycle(&self) -> u32 {
        self.deck.bus().cpu.cycle
    }
}
