//! Bulk VRAM uploads: the game's screens, palettes and patches are stored in ROM as a little
//! command stream that `copyToPpu` plays into the PPU.
//!
//! Each command starts with a byte:
//! - `$00..=$7F` (except `$4C`/`$60`): the high byte of a VRAM address. Then the low byte, then a
//!   control byte: bit 7 = write downwards (+32 per byte) rather than across, bit 6 = repeat one
//!   data byte, bits 0-5 = count (0 means 64). Then the data.
//! - `$4C` ("jmp"): continue at the address in the next two bytes, coming back afterwards.
//! - `$60` ("rts"): go back to where the last `$4C` was.
//! - `$80..=$FF`: end.

use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

const CMD_JUMP: u8 = 0x4C;
const CMD_RETURN: u8 = 0x60;
const PALETTE_PAGE: u8 = 0x3F;
const CTRL_INCREMENT_32: u8 = 0x04;

impl Game {
    /// `bulkCopyToPpu`: play the command stream at `data` (given inline after the call in the
    /// original).
    pub fn bulk_copy_to_ppu(&mut self, data: u16) {
        self.copy_to_ppu(data);
    }

    /// `copyToPpu`.
    pub fn copy_to_ppu(&mut self, mut ptr: u16) {
        // The original keeps the "jmp" return addresses on the CPU stack.
        let mut returns = Vec::new();
        loop {
            self.ppu_status();
            let cmd = self.rom.byte(ptr);
            match cmd {
                0x80..=0xFF => break,
                CMD_RETURN => {
                    let back = returns.pop().expect("copyToPpu: return without jump");
                    ptr = back + 3;
                }
                CMD_JUMP => {
                    returns.push(ptr);
                    ptr = self.rom.word(ptr + 1);
                }
                high => {
                    let used = self.copy_run(ptr, high);
                    ptr += used;
                }
            }
        }
        self.set_scratch_pointer(ptr);
    }

    /// One address-control-data command at `ptr`; returns its length in bytes.
    fn copy_run(&mut self, ptr: u16, high: u8) -> u16 {
        self.ppu_addr(u16::from_be_bytes([high, self.rom.byte(ptr + 1)]));
        let control = self.rom.byte(ptr + 2);
        let ctrl = if control & 0x80 != 0 {
            self.vars.current_ppu_ctrl | CTRL_INCREMENT_32
        } else {
            self.vars.current_ppu_ctrl & !CTRL_INCREMENT_32
        };
        self.ppu_ctrl(ctrl);
        self.vars.current_ppu_ctrl = ctrl;

        let repeat = control & 0x40 != 0;
        // A count of 0 means 64 for runs; a repeated fill with count 0 runs 256 times (the
        // original's X register counts down from 0).
        let count = match (control & 0x3F, repeat) {
            (0, false) => 64,
            (0, true) => 256,
            (n, _) => n as u16,
        };
        let mut offset = 2;
        if repeat {
            offset += 1;
        }
        for _ in 0..count {
            if !repeat {
                offset += 1;
            }
            let v = self.rom.byte(ptr + offset);
            self.ppu_data(v);
        }
        if high == PALETTE_PAGE {
            // Point the PPU away from the palette ($3F00, then $0000) so the backdrop shows.
            self.ppu_addr(0x3F00);
            self.ppu_addr(0x0000);
        }
        // The next command starts after the last byte read (8-bit offset, as the original's Y).
        (offset as u8 as u16) + 1
    }

    /// The original leaves its data pointer in tmp1/tmp2.
    fn set_scratch_pointer(&mut self, ptr: u16) {
        self.vars.tmp1 = ptr as u8;
        self.vars.tmp2 = (ptr >> 8) as u8;
    }
}

pub const SHADOW: &[Routine] = &[Routine {
    name: "bulkCopyToPpu",
    addr: layout::BULK_COPY_TO_PPU,
    run: |g, r| {
        g.bulk_copy_to_ppu(r.inline_word);
        Ret::Unit
    },
    scratch: &[],
}];
