//! The user's ROM: checked to be NES Tetris (USA), and read for data tables and graphics.
//!
//! Nothing from the ROM is in the source; the `tetris` binary embeds the ROM it was built with
//! (`build.rs`). Tables are read at the addresses in `layout.rs`.

use crate::layout;

/// SHA-1 of PRG + CHR (the file minus its 16-byte header) of Tetris (USA).
const BODY_SHA1: &str = "fd9079cb5e8479eb06d93c2ae5175bfce871746a";
const PRG_SIZE: usize = 0x8000;
const CHR_SIZE: usize = 0x4000;

pub struct Rom {
    prg: Vec<u8>,
    pub chr: Vec<u8>,
}

#[derive(Debug)]
pub enum RomError {
    NotInes,
    WrongSize(usize),
    WrongRom(String),
}

impl std::fmt::Display for RomError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            RomError::NotInes => write!(f, "not an iNES file"),
            RomError::WrongSize(n) => write!(f, "{n} bytes; expected a 48 KB Tetris ROM"),
            RomError::WrongRom(h) => write!(f, "not Tetris (USA): PRG+CHR SHA-1 is {h}"),
        }
    }
}

impl Rom {
    pub fn new(file: &[u8]) -> Result<Self, RomError> {
        if file.len() < 4 || &file[..4] != b"NES\x1A" {
            return Err(RomError::NotInes);
        }
        if file.len() != 16 + PRG_SIZE + CHR_SIZE {
            return Err(RomError::WrongSize(file.len()));
        }
        let body = &file[16..];
        let hash = sha1_smol::Sha1::from(body).digest().to_string();
        if hash != BODY_SHA1 {
            return Err(RomError::WrongRom(hash));
        }
        Ok(Self { prg: body[..PRG_SIZE].to_vec(), chr: body[PRG_SIZE..].to_vec() })
    }

    /// The byte at CPU address `addr` ($8000-$FFFF).
    pub fn byte(&self, addr: u16) -> u8 {
        self.prg[addr as usize - 0x8000]
    }

    /// `len` bytes starting at CPU address `addr`.
    pub fn bytes(&self, addr: u16, len: usize) -> &[u8] {
        &self.prg[addr as usize - 0x8000..][..len]
    }

    /// The little-endian word at `addr`.
    pub fn word(&self, addr: u16) -> u16 {
        u16::from_le_bytes([self.byte(addr), self.byte(addr + 1)])
    }

    /// Entry `i` of a byte table.
    pub fn table(&self, table: u16, i: usize) -> u8 {
        self.byte(table + i as u16)
    }

    /// Frames per gravity step at `level` (levels 29+ use 1, as the game does without a lookup).
    pub fn frames_per_drop(&self, level: u8) -> u8 {
        if level >= 29 { 1 } else { self.table(layout::FRAMES_PER_DROP_TABLE, level as usize) }
    }
}
