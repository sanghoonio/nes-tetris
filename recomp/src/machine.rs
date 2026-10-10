//! CPU registers plus the NES bus: RAM, PPU registers and timing, MMC1, joypads, APU latches.
//!
//! Timing is per instruction, not per bus cycle: an instruction's cycles are added after it runs.
//! That's enough for Tetris, which waits for NMI every frame and never races the PPU mid-frame.

use crate::ppu::Ppu;

pub const PRG_SIZE: usize = 0x8000;
pub const CHR_SIZE: usize = 0x4000;

/// Joypad bit order as the NES shifts it out (first read = A).
pub const BTN_A: u8 = 0x80;
pub const BTN_B: u8 = 0x40;
pub const BTN_SELECT: u8 = 0x20;
pub const BTN_START: u8 = 0x10;
pub const BTN_UP: u8 = 0x08;
pub const BTN_DOWN: u8 = 0x04;
pub const BTN_LEFT: u8 = 0x02;
pub const BTN_RIGHT: u8 = 0x01;

pub struct Machine {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub c: bool,
    pub z: bool,
    pub i: bool,
    pub d: bool,
    pub v: bool,
    pub n: bool,

    pub ram: [u8; 0x800],
    pub prg: Vec<u8>,
    pub prg_ram: [u8; 0x2000],
    pub ppu: Ppu,
    pub apu_regs: [u8; 0x18],

    /// MMC1 shift register and registers.
    pub mmc1_shift: u8,
    pub mmc1_count: u8,
    pub mmc1_control: u8,
    pub mmc1_chr0: u8,
    pub mmc1_chr1: u8,
    pub mmc1_prg: u8,

    /// Buttons held on each pad, in [`BTN_A`].. order.
    pub buttons: [u8; 2],
    joy_shift: [u8; 2],
    joy_strobe: bool,

    pub cycles: u64,
    pub extra_cycles: u32,
    pub nmi_pending: bool,
    /// The NMI edge came during the last cycle of an instruction: let one more instruction run.
    nmi_delay: bool,
    /// Where the most recent `rts`/`rti` returned to.
    pub ret_to: u16,
    /// Called on every NMI entry (before the handler runs), for tracing and frame hooks.
    pub on_nmi: Option<Box<dyn FnMut(&mut Machine) + Send>>,
    /// Addresses the fallback interpreter had to run, for reporting.
    pub fallback_log: Vec<u16>,
}

impl Machine {
    /// Build a machine from an iNES / NES 2.0 file. RAM starts all zero.
    pub fn new(rom: &[u8]) -> Self {
        let mut m = Self::power_on(rom);
        m.sync_ppu_mapping();
        m
    }

    fn power_on(rom: &[u8]) -> Self {
        assert!(rom.len() >= 16 + PRG_SIZE + CHR_SIZE && &rom[..4] == b"NES\x1A", "not a Tetris ROM");
        let prg = rom[16..16 + PRG_SIZE].to_vec();
        let chr = rom[16 + PRG_SIZE..16 + PRG_SIZE + CHR_SIZE].to_vec();
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            c: false,
            z: false,
            i: true,
            d: false,
            v: false,
            n: false,
            ram: [0; 0x800],
            prg,
            prg_ram: [0; 0x2000],
            ppu: Ppu::new(chr),
            apu_regs: [0; 0x18],
            mmc1_shift: 0,
            mmc1_count: 0,
            mmc1_control: 0x0C,
            mmc1_chr0: 0,
            mmc1_chr1: 0,
            mmc1_prg: 0,
            buttons: [0; 2],
            joy_shift: [0; 2],
            joy_strobe: false,
            cycles: 0,
            extra_cycles: 0,
            nmi_pending: false,
            nmi_delay: false,
            ret_to: 0,
            on_nmi: None,
            fallback_log: Vec::new(),
        }
    }

    pub fn vector(&self, addr: u16) -> u16 {
        u16::from_le_bytes([self.prg[addr as usize - 0x8000], self.prg[addr as usize - 0x7FFF]])
    }

    /// Advance the clock; the PPU may raise NMI.
    #[inline(always)]
    pub fn tick(&mut self, cycles: u32) {
        self.cycles += cycles as u64;
        if let Some(dots_ago) = self.ppu.advance(cycles * 3) {
            self.nmi_pending = true;
            self.nmi_delay = dots_ago < 3;

        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[addr as usize & 0x7FF],
            0x2000..=0x3FFF => {
                self.ppu.read_reg(addr & 7)
            }
            0x4016 | 0x4017 => {
                let pad = (addr - 0x4016) as usize;
                if self.joy_strobe {
                    self.joy_shift[pad] = self.buttons[pad];
                }
                let bit = self.joy_shift[pad] >> 7;
                // Standard pads shift in 1s after the eight buttons.
                self.joy_shift[pad] = self.joy_shift[pad] << 1 | 1;
                0x40 | bit
            }
            0x4000..=0x5FFF => 0x40,
            0x6000..=0x7FFF => self.prg_ram[addr as usize - 0x6000],
            0x8000..=0xFFFF => self.prg[addr as usize - 0x8000],
        }
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[addr as usize & 0x7FF] = v,
            0x2000..=0x3FFF => {
                if self.ppu.write_reg(addr & 7, v) {
                    self.nmi_pending = true;
                }
            }
            0x4014 => {
                let base = (v as u16) << 8;
                for i in 0..256u16 {
                    let b = self.read(base + i);
                    self.ppu.oam_dma_byte(b);
                }
                let odd = self.cycles & 1 == 1;
                self.tick(513 + odd as u32);
            }
            0x4016 => {
                self.joy_strobe = v & 1 != 0;
                if self.joy_strobe {
                    self.joy_shift = self.buttons;
                }
            }
            0x4000..=0x4017 => self.apu_regs[addr as usize - 0x4000] = v,
            0x4018..=0x5FFF => {}
            0x6000..=0x7FFF => self.prg_ram[addr as usize - 0x6000] = v,
            0x8000..=0xFFFF => self.mmc1_write(addr, v),
        }
    }

    /// The unmodified-value write of a read-modify-write instruction. Tetris only does RMW on RAM
    /// and on ROM (to reset MMC1, where both writes reset it identically), so it's a no-op.
    #[inline(always)]
    pub fn write_dummy(&mut self, _addr: u16, _v: u8) {}

    fn mmc1_write(&mut self, addr: u16, v: u8) {
        if v & 0x80 != 0 {
            self.mmc1_shift = 0;
            self.mmc1_count = 0;
            self.mmc1_control |= 0x0C;
            self.sync_ppu_mapping();
            return;
        }
        self.mmc1_shift |= (v & 1) << self.mmc1_count;
        self.mmc1_count += 1;
        if self.mmc1_count == 5 {
            let r = self.mmc1_shift;
            match addr {
                0x8000..=0x9FFF => self.mmc1_control = r,
                0xA000..=0xBFFF => self.mmc1_chr0 = r,
                0xC000..=0xDFFF => self.mmc1_chr1 = r,
                _ => self.mmc1_prg = r,
            }
            self.mmc1_shift = 0;
            self.mmc1_count = 0;
            self.sync_ppu_mapping();
        }
    }

    fn sync_ppu_mapping(&mut self) {
        self.ppu.mirroring = self.mmc1_control & 3;
        self.ppu.chr_banks = self.chr_banks();
    }

    /// The two 4 KB CHR banks currently mapped at PPU $0000 and $1000.
    pub fn chr_banks(&self) -> [usize; 2] {
        let count = 4; // 16 KB of CHR = four 4 KB banks
        if self.mmc1_control & 0x10 != 0 {
            [self.mmc1_chr0 as usize % count, self.mmc1_chr1 as usize % count]
        } else {
            let b = (self.mmc1_chr0 as usize & !1) % count;
            [b, b + 1]
        }
    }

    /// After a call returns: false if the stack is where it was before the `jsr` (carry on at
    /// [`Machine::ret_to`]); true if the callee also popped our own return address, as the
    /// jump-table helper does, in which case the caller must return too.
    #[inline(always)]
    pub fn unwound(&self, sp0: u8) -> bool {
        if self.sp == sp0 {
            return false;
        }
        let popped = self.sp.wrapping_sub(sp0);
        assert!(popped < 0x80, "callee left {} bytes on the stack", popped.wrapping_neg());
        true
    }

    /// Enter the NMI handler if one is pending. `pc` is where execution resumes afterwards.
    #[inline(always)]
    pub fn poll(&mut self, pc: u16) {
        if self.nmi_pending {
            if self.nmi_delay {
                self.nmi_delay = false;
            } else {
                self.take_nmi(pc);
            }
        }
    }

    #[cold]
    fn take_nmi(&mut self, pc: u16) {
        self.nmi_pending = false;
        self.push((pc >> 8) as u8);
        self.push(pc as u8);
        self.push(self.status() | crate::cpu::FLAG_U);
        self.i = true;
        self.tick(7);
        if let Some(mut hook) = self.on_nmi.take() {
            hook(self);
            self.on_nmi = Some(hook);
        }
        let handler = self.vector(0xFFFA);
        crate::generated::call(self, handler);
        debug_assert_eq!(self.ret_to, pc, "NMI returned somewhere else");
    }

    /// Power on: run the reset vector. Returns only if the game does (it doesn't).
    pub fn run(&mut self) {
        self.tick(7);
        let reset = self.vector(0xFFFC);
        crate::generated::call(self, reset);
    }
}
