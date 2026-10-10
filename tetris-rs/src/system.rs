//! Small hardware helpers: memory fills, mapper banking, OAM DMA, NMI control.

use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

/// PPUCTRL bits: NMI on vblank, and VRAM address increment of 32 (rather than 1).
const PPU_CTRL_NMI: u8 = 0x80;
const PPU_CTRL_INCREMENT_32: u8 = 0x04;
/// PPUSTATUS bit 7: vblank has started.
const PPU_STATUS_VBLANK: u8 = 0x80;
/// VRAM pages from $2000 up are nametables (and their attribute tables).
const FIRST_NAMETABLE_PAGE: u8 = 0x20;
/// A nametable is 1 KB: four pages, attribute bytes included.
const NAMETABLE_PAGES: u8 = 4;
/// The attribute table starts at $23C0 for the nametable at $2000: three pages above it.
const ATTRIBUTE_PAGE_OFFSET: u8 = 3;
const ATTRIBUTE_TABLE_LOW: u8 = 0xC0;
const ATTRIBUTE_BYTES: usize = 0x40;
/// The 2 KB of work RAM is mirrored through $0000-$1FFF, so a page selects its 2 KB copy by its
/// low three bits.
const RAM_MIRROR_PAGE_MASK: u8 = 0x07;

impl Game {
    /// `setMMC1Control`: write the MMC1 control register (mirroring, PRG/CHR mode).
    pub fn set_mmc1_control_value(&mut self, value: u8) {
        self.set_mmc1_control(value);
    }

    /// `changeCHRBank0`: select the 4 KB CHR bank at PPU $0000.
    pub fn change_chr_bank0(&mut self, bank: u8) {
        self.set_chr_bank0(bank);
    }

    /// `changeCHRBank1`: select the 4 KB CHR bank at PPU $1000.
    pub fn change_chr_bank1(&mut self, bank: u8) {
        self.set_chr_bank1(bank);
    }

    /// `changePRGBank`: select the switchable PRG bank.
    pub fn change_prg_bank(&mut self, bank: u8) {
        self.set_prg_bank(bank);
    }

    /// Write PPUCTRL and remember the value in `current_ppu_ctrl`, which the game keeps as the
    /// truth for NMI and pattern-table selection.
    fn write_ppu_ctrl(&mut self, value: u8) {
        self.ppu_ctrl(value);
        self.vars.current_ppu_ctrl = value;
    }

    /// `waitForVBlankAndEnableNmi`: wait for vblank, then turn NMI on.
    ///
    /// The original loops while PPUSTATUS bit 7 is set. Reading PPUSTATUS clears that flag, so
    /// the loop repeats at most once: a read that sees vblank clears it, and the next read exits.
    pub fn wait_for_vblank_and_enable_nmi(&mut self) {
        while self.ppu_status() & PPU_STATUS_VBLANK != 0 {}
        let value = self.vars.current_ppu_ctrl | PPU_CTRL_NMI;
        self.write_ppu_ctrl(value);
    }

    /// `disableNmi`: clear the NMI-enable bit in PPUCTRL.
    pub fn disable_nmi(&mut self) {
        let value = self.vars.current_ppu_ctrl & !PPU_CTRL_NMI;
        self.write_ppu_ctrl(value);
    }

    /// `copyCurrentScrollAndCtrlToPPU`: reset the scroll to the origin and restore PPUCTRL.
    /// The scroll is always zero here; the game's `ppu_scroll_x/y` are not used.
    pub fn copy_current_scroll_and_ctrl_to_ppu(&mut self) {
        self.ppu_scroll(0, 0);
        let value = self.vars.current_ppu_ctrl;
        self.ppu_ctrl(value);
    }

    /// `copyOamStagingToOam`: DMA the staged sprites from page $02 into OAM, from OAM address 0.
    pub fn copy_oam_staging_to_oam(&mut self) {
        self.oam_addr(0);
        self.oam_dma();
    }

    /// `memset_ppu_page_and_more`: fill VRAM. `page` is the high byte of the VRAM address.
    ///
    /// Below $2000 (pattern tables) it writes `count` pages of `fill`, with a count of 0 meaning
    /// 256 pages (the loop tests after its first pass). From $2000 up it fills four pages of
    /// nametable with `fill`, then writes `count` into the 64 attribute bytes that follow.
    pub fn memset_ppu_page_and_more(&mut self, page: u8, fill: u8, count: u8) {
        // Reading PPUSTATUS resets the VRAM address latch before the address is written.
        self.ppu_status();
        // Clear the increment-32 bit so VRAM writes step by one.
        let value = self.vars.current_ppu_ctrl & !PPU_CTRL_INCREMENT_32;
        self.write_ppu_ctrl(value);
        self.ppu_addr((page as u16) << 8);

        let is_nametable = page >= FIRST_NAMETABLE_PAGE;
        let pages = if is_nametable { NAMETABLE_PAGES } else { count };
        // Do-while loop: a count of 0 writes 256 pages.
        let pages = pages.wrapping_sub(1) as usize + 1;
        for _ in 0..pages * 256 {
            self.ppu_data(fill);
        }

        if is_nametable {
            // The original adds 2 to the page with the compare's carry still set, so the
            // attribute table is 3 pages above the nametable page.
            let attribute_page = page.wrapping_add(ATTRIBUTE_PAGE_OFFSET);
            self.ppu_addr(((attribute_page as u16) << 8) | ATTRIBUTE_TABLE_LOW as u16);
            for _ in 0..ATTRIBUTE_BYTES {
                self.ppu_data(count);
            }
        }
    }

    /// `memset_page`: fill RAM pages from `last` down to `first` (inclusive) with `value`.
    pub fn memset_page(&mut self, value: u8, first: u8, last: u8) {
        // Last - first + 1 pages, which is 256 when first is last + 1.
        let count = last.wrapping_sub(first) as usize + 1;
        self.vars.with_ram(|ram| {
            for i in 0..count {
                let page = last.wrapping_sub(i as u8) & RAM_MIRROR_PAGE_MASK;
                let start = page as usize * 256;
                ram[start..start + 256].fill(value);
            }
        });
    }
}

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "copyOamStagingToOam",
        addr: layout::COPY_OAM_STAGING_TO_OAM,
        run: |g, _| {
            g.copy_oam_staging_to_oam();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "copyCurrentScrollAndCtrlToPPU",
        addr: layout::COPY_CURRENT_SCROLL_AND_CTRL_TO_PPU,
        run: |g, _| {
            g.copy_current_scroll_and_ctrl_to_ppu();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "disableNmi",
        addr: layout::DISABLE_NMI,
        run: |g, _| {
            g.disable_nmi();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "waitForVBlankAndEnableNmi",
        addr: layout::WAIT_FOR_VBLANK_AND_ENABLE_NMI,
        run: |g, _| {
            g.wait_for_vblank_and_enable_nmi();
            Ret::Unit
        },
        scratch: &[],
    },
    // Routines taking their arguments in A, X and Y.
    Routine {
        name: "memset_page",
        addr: layout::MEMSET_PAGE,
        run: |g, r| {
            g.memset_page(r.a, r.x, r.y);
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "memset_ppu_page_and_more",
        addr: layout::MEMSET_PPU_PAGE_AND_MORE,
        run: |g, r| {
            g.memset_ppu_page_and_more(r.a, r.x, r.y);
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "setMMC1Control",
        addr: layout::SET_MMC1_CONTROL,
        run: |g, r| {
            g.set_mmc1_control_value(r.a);
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "changeCHRBank0",
        addr: layout::CHANGE_CHRBANK0,
        run: |g, r| {
            g.change_chr_bank0(r.a);
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "changeCHRBank1",
        addr: layout::CHANGE_CHRBANK1,
        run: |g, r| {
            g.change_chr_bank1(r.a);
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "changePRGBank",
        addr: layout::CHANGE_PRGBANK,
        run: |g, r| {
            g.change_prg_bank(r.a);
            Ret::Unit
        },
        scratch: &[],
    },
];
