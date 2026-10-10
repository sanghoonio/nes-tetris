//! The hardware the game talks to, behind named operations: the PPU (shared model in the `ppu`
//! crate), the MMC1 mapper's CHR banking, the joypads, and the APU.
//!
//! The original reaches these through memory-mapped registers; here each register access is a
//! method on `Game`, so game code reads as what it does ("set the CHR bank", "write a byte to
//! VRAM") while the device state stays exactly what the original leaves behind.

use crate::Game;

/// Where APU register writes go: the synthesiser when playing, a log in tests.
pub trait ApuSink: Send {
    fn write(&mut self, addr: u16, value: u8);

    /// The frame is over: the audio it produced, if this sink makes any.
    fn end_frame(&mut self) -> Vec<f32> {
        Vec::new()
    }
}

impl ApuSink for apu::Apu {
    fn write(&mut self, addr: u16, value: u8) {
        apu::Apu::write(self, addr, value);
    }
}

impl ApuSink for Vec<(u16, u8)> {
    fn write(&mut self, addr: u16, value: u8) {
        self.push((addr, value));
    }
}

/// MMC1 registers. The game sets them with five serial writes each, which leaves the shift
/// register empty; `shift`/`count` only matter for stray writes (`Game::mapper_write`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mmc1 {
    pub control: u8,
    pub chr0: u8,
    pub chr1: u8,
    pub prg: u8,
    pub shift: u8,
    pub count: u8,
}

impl Mmc1 {
    /// The two 4 KB CHR banks mapped at PPU $0000 and $1000.
    pub fn chr_banks(&self) -> [usize; 2] {
        let count = 4;
        if self.control & 0x10 != 0 {
            [self.chr0 as usize % count, self.chr1 as usize % count]
        } else {
            let b = (self.chr0 as usize & !1) % count;
            [b, b + 1]
        }
    }
}

/// Two standard pads. `held` is set by the frontend each frame (bit 7 = A ... bit 0 = Right).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Joypads {
    pub held: [u8; 2],
    pub shift: [u8; 2],
    pub strobe: bool,
}

impl Joypads {
    pub fn write_strobe(&mut self, v: u8) {
        self.strobe = v & 1 != 0;
        if self.strobe {
            self.shift = self.held;
        }
    }

    /// Read one bit from `port` ($4016 = 0, $4017 = 1), with the open-bus bit 6 set.
    pub fn read(&mut self, port: usize) -> u8 {
        if self.strobe {
            self.shift[port] = self.held[port];
        }
        let bit = self.shift[port] >> 7;
        self.shift[port] = self.shift[port] << 1 | 1;
        0x40 | bit
    }
}

impl Game {
    // ---- PPU ----

    pub fn ppu_ctrl(&mut self, v: u8) {
        self.ppu.write_reg(0, v);
    }

    pub fn ppu_mask(&mut self, v: u8) {
        self.ppu.write_reg(1, v);
    }

    /// Read PPUSTATUS (clears the vblank flag and the address latch).
    pub fn ppu_status(&mut self) -> u8 {
        self.ppu.read_reg(2)
    }

    pub fn oam_addr(&mut self, v: u8) {
        self.ppu.write_reg(3, v);
    }

    /// Point the PPU's VRAM address at `addr` (two PPUADDR writes, high byte first).
    pub fn ppu_addr(&mut self, addr: u16) {
        self.ppu.write_reg(6, (addr >> 8) as u8);
        self.ppu.write_reg(6, addr as u8);
    }

    /// Write one byte to VRAM at the current address, which then advances by 1 or 32.
    pub fn ppu_data(&mut self, v: u8) {
        self.ppu.write_reg(7, v);
    }

    pub fn ppu_scroll(&mut self, x: u8, y: u8) {
        self.ppu.write_reg(5, x);
        self.ppu.write_reg(5, y);
    }

    /// OAM DMA from `oam_staging`.
    pub fn oam_dma(&mut self) {
        for &b in &self.vars.oam_staging {
            self.ppu.oam_dma_byte(b);
        }
    }

    // ---- MMC1 ----

    fn sync_chr(&mut self) {
        self.ppu.chr_banks = self.mmc1.chr_banks();
        self.ppu.mirroring = self.mmc1.control & 3;
    }

    pub fn set_mmc1_control(&mut self, v: u8) {
        self.mmc1.control = v & 0x1F;
        self.sync_chr();
    }

    pub fn set_chr_bank0(&mut self, v: u8) {
        self.mmc1.chr0 = v & 0x1F;
        self.sync_chr();
    }

    pub fn set_chr_bank1(&mut self, v: u8) {
        self.mmc1.chr1 = v & 0x1F;
        self.sync_chr();
    }

    pub fn set_prg_bank(&mut self, v: u8) {
        self.mmc1.prg = v & 0x1F;
    }

    /// A CPU write to $8000-$FFFF: one serial bit into the MMC1 (bit 7 resets it).
    pub fn mapper_write(&mut self, addr: u16, v: u8) {
        let m = &mut self.mmc1;
        if v & 0x80 != 0 {
            m.shift = 0;
            m.count = 0;
            m.control |= 0x0C;
        } else {
            m.shift |= (v & 1) << m.count;
            m.count += 1;
            if m.count == 5 {
                let r = m.shift;
                match addr {
                    0x8000..=0x9FFF => m.control = r,
                    0xA000..=0xBFFF => m.chr0 = r,
                    0xC000..=0xDFFF => m.chr1 = r,
                    _ => m.prg = r,
                }
                m.shift = 0;
                m.count = 0;
            }
        }
        self.sync_chr();
    }

    // ---- APU ----

    pub fn apu_write(&mut self, addr: u16, v: u8) {
        self.apu.write(addr, v);
    }
}
