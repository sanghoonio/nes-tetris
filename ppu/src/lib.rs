//! PPU registers, VRAM, frame timing and a scanline renderer.
//!
//! Timing tracks dots within the frame so vblank (and NMI) land on the right CPU cycle. Each
//! visible line is drawn from the state at the moment the PPU passes it, which catches the game
//! switching screens partway through a frame; within a line, rendering isn't cycle-accurate.

pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 240;

const DOTS_PER_LINE: u32 = 341;
const VBLANK_SET: u32 = 241 * DOTS_PER_LINE + 1;
const VBLANK_CLEAR: u32 = 261 * DOTS_PER_LINE + 1;
const FRAME_DOTS: u32 = 262 * DOTS_PER_LINE;
/// A visible line is drawn when the PPU reaches this dot of it.
const LINE_RENDER_DOT: u32 = 256;
const RENDER_END: u32 = (HEIGHT as u32 - 1) * DOTS_PER_LINE + LINE_RENDER_DOT + 1;

#[derive(Clone)]
pub struct Ppu {
    pub chr: Vec<u8>,
    pub vram: [u8; 0x800],
    pub palette: [u8; 32],
    pub oam: [u8; 256],
    pub ctrl: u8,
    pub mask: u8,
    pub vblank: bool,
    pub oam_addr: u8,
    /// Loopy registers: current and temporary VRAM address, fine X, write toggle.
    pub v: u16,
    pub t: u16,
    pub fine_x: u8,
    pub w: bool,
    read_buffer: u8,
    /// Last value driven on the PPU's data bus; low bits of PPUSTATUS read back as this.
    open_bus: u8,

    /// Dot within the current frame, and frame parity for the odd-frame skip.
    pub dot: u32,
    pub odd_frame: bool,
    pub frame: u64,

    /// Mapper state the PPU sees, kept in sync by the machine: MMC1 mirroring mode and the two
    /// 4 KB CHR banks mapped at $0000 and $1000.
    pub mirroring: u8,
    pub chr_banks: [usize; 2],
    /// The frame being drawn, as NES palette indices; complete once vblank starts.
    pub frame_buf: Box<[u8; WIDTH * HEIGHT]>,
}

impl Ppu {
    pub fn new(chr: Vec<u8>) -> Self {
        Self {
            chr,
            vram: [0; 0x800],
            palette: [0; 32],
            oam: [0; 256],
            ctrl: 0,
            mask: 0,
            vblank: false,
            oam_addr: 0,
            v: 0,
            t: 0,
            fine_x: 0,
            w: false,
            read_buffer: 0,
            open_bus: 0,
            dot: 0,
            odd_frame: false,
            frame: 0,
            mirroring: 0,
            chr_banks: [0, 1],
            frame_buf: Box::new([0; WIDTH * HEIGHT]),
        }
    }

    fn rendering_enabled(&self) -> bool {
        self.mask & 0x18 != 0
    }

    /// Advance by `dots`. If NMI should be raised, returns how many dots ago the vblank edge was
    /// (the CPU only sees an edge that comes before an instruction's last cycle).
    #[inline(always)]
    pub fn advance(&mut self, dots: u32) -> Option<u32> {
        let before = self.dot;
        self.dot += dots;
        // Draw each visible line once the PPU has passed its last pixel.
        if before < RENDER_END {
            let first = (before + DOTS_PER_LINE - LINE_RENDER_DOT) / DOTS_PER_LINE;
            let last = (self.dot.min(RENDER_END) + DOTS_PER_LINE - LINE_RENDER_DOT) / DOTS_PER_LINE;
            for y in first..last {
                self.render_line(y as usize);
            }
        }
        let mut nmi = None;
        if before < VBLANK_SET && self.dot >= VBLANK_SET {
            self.vblank = true;
            if self.ctrl & 0x80 != 0 {
                nmi = Some(self.dot - VBLANK_SET);
            }
        }
        if before < VBLANK_CLEAR && self.dot >= VBLANK_CLEAR {
            self.vblank = false;
        }
        // The pre-render line is one dot short on odd frames while rendering.
        let len = if self.odd_frame && self.rendering_enabled() { FRAME_DOTS - 1 } else { FRAME_DOTS };
        if self.dot >= len {
            self.dot -= len;
            self.odd_frame = !self.odd_frame;
            self.frame += 1;
            // A long instruction (OAM DMA) can carry us across a frame boundary into vblank.
            if self.dot >= VBLANK_SET {
                self.vblank = true;
                if self.ctrl & 0x80 != 0 {
                    nmi = Some(self.dot - VBLANK_SET);
                }
            }
        }
        nmi
    }

    pub fn read_reg(&mut self, reg: u16) -> u8 {
        match reg {
            2 => {
                let v = (self.vblank as u8) << 7 | (self.open_bus & 0x1F);
                self.vblank = false;
                self.w = false;
                self.open_bus = v;
                v
            }
            4 => {
                let v = self.oam[self.oam_addr as usize];
                self.open_bus = v;
                v
            }
            7 => {
                let addr = self.v & 0x3FFF;
                let v = if addr >= 0x3F00 {
                    // Palette reads are immediate; the buffer gets the nametable byte underneath.
                    self.read_buffer = self.vram_read(addr - 0x1000);
                    self.palette[palette_index(addr)] | (self.open_bus & 0xC0)
                } else {
                    let buffered = self.read_buffer;
                    self.read_buffer = self.vram_read(addr);
                    buffered
                };
                self.increment_v();
                self.open_bus = v;
                v
            }
            _ => self.open_bus,
        }
    }

    /// Returns true if this write raises NMI (enabling it during vblank).
    pub fn write_reg(&mut self, reg: u16, v: u8) -> bool {
        self.open_bus = v;
        match reg {
            0 => {
                let was_enabled = self.ctrl & 0x80 != 0;
                self.ctrl = v;
                self.t = (self.t & !0x0C00) | ((v as u16 & 3) << 10);
                return !was_enabled && v & 0x80 != 0 && self.vblank;
            }
            1 => self.mask = v,
            3 => self.oam_addr = v,
            4 => {
                self.oam[self.oam_addr as usize] = v;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            5 => {
                if !self.w {
                    self.t = (self.t & !0x001F) | (v as u16 >> 3);
                    self.fine_x = v & 7;
                } else {
                    self.t = (self.t & !0x73E0) | ((v as u16 & 7) << 12) | ((v as u16 & 0xF8) << 2);
                }
                self.w = !self.w;
            }
            6 => {
                if !self.w {
                    self.t = (self.t & 0x00FF) | ((v as u16 & 0x3F) << 8);
                } else {
                    self.t = (self.t & 0xFF00) | v as u16;
                    self.v = self.t;
                }
                self.w = !self.w;
            }
            7 => {
                let addr = self.v & 0x3FFF;
                self.vram_write(addr, v);
                self.increment_v();
            }
            _ => {}
        }
        false
    }

    pub fn oam_dma_byte(&mut self, b: u8) {
        self.oam[self.oam_addr as usize] = b;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    fn increment_v(&mut self) {
        let step = if self.ctrl & 0x04 != 0 { 32 } else { 1 };
        self.v = self.v.wrapping_add(step) & 0x7FFF;
    }

    fn vram_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.chr_byte(addr),
            0x2000..=0x3EFF => self.vram[nametable_index(addr, self.mirroring)],
            _ => self.palette[palette_index(addr)],
        }
    }

    fn vram_write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => {} // CHR ROM
            0x2000..=0x3EFF => self.vram[nametable_index(addr, self.mirroring)] = v,
            _ => self.palette[palette_index(addr)] = v & 0x3F,
        }
    }

    fn chr_byte(&self, addr: u16) -> u8 {
        let bank = self.chr_banks[(addr >> 12) as usize & 1];
        self.chr[bank * 0x1000 + (addr as usize & 0xFFF)]
    }

    /// Draw the whole frame from the current state, for callers without dot timing.
    pub fn render_frame(&mut self) {
        for y in 0..HEIGHT {
            self.render_line(y);
        }
    }

    /// Draw scanline `y` from the current state into `self.frame_buf`.
    fn render_line(&mut self, y: usize) {
        let mut row = [0u8; WIDTH];
        let mut backdrop = self.palette[0];
        // With rendering off, a VRAM address inside palette memory shows that entry instead of
        // the backdrop (visible while the game uploads a palette).
        if !self.rendering_enabled() && self.v & 0x3F00 == 0x3F00 {
            backdrop = self.palette[palette_index(self.v)];
        }
        let mut bg_opaque = [false; WIDTH];
        if self.mask & 0x08 == 0 {
            row.fill(backdrop);
        } else {
            let bg_table = if self.ctrl & 0x10 != 0 { 0x1000 } else { 0 };
            let scroll_x = ((self.t & 0x1F) << 3 | self.fine_x as u16) as usize
                + if self.t & 0x0400 != 0 { 256 } else { 0 };
            let scroll_y = (((self.t >> 5) & 0x1F) << 3 | (self.t >> 12) & 7) as usize
                + if self.t & 0x0800 != 0 { 240 } else { 0 };
            let wy = (y + scroll_y) % 480;
            for x in 0..WIDTH {
                let wx = (x + scroll_x) % 512;
                let nt = 0x2000 + (wy / 240) as u16 * 0x800 + (wx / 256) as u16 * 0x400;
                let (tx, ty) = ((wx % 256) / 8, (wy % 240) / 8);
                let tile = self.vram[nametable_index(nt + (ty * 32 + tx) as u16, self.mirroring)];
                let attr = self.vram[nametable_index(nt + 0x3C0 + ((ty / 4) * 8 + tx / 4) as u16, self.mirroring)];
                let shift = ((ty & 2) << 1) | (tx & 2);
                let pal = (attr >> shift) & 3;
                let px = self.tile_pixel(bg_table + tile as u16 * 16, wx % 8, wy % 8);
                if px == 0 || (x < 8 && self.mask & 0x02 == 0) {
                    row[x] = backdrop;
                } else {
                    bg_opaque[x] = true;
                    row[x] = self.palette[(pal * 4 + px) as usize];
                }
            }
        }
        if self.mask & 0x10 != 0 {
            let tall = self.ctrl & 0x20 != 0;
            let h = if tall { 16 } else { 8 };
            // Sprites are drawn one line below their OAM Y. Lower OAM index wins, so draw from
            // the back; only the first eight on a line are shown.
            let on_line: Vec<usize> = (0..64)
                .filter(|&s| {
                    let top = self.oam[s * 4] as usize + 1;
                    y >= top && y < top + h
                })
                .take(8)
                .collect();
            for &s in on_line.iter().rev() {
                let [sy, tile, attr, sx] =
                    [self.oam[s * 4], self.oam[s * 4 + 1], self.oam[s * 4 + 2], self.oam[s * 4 + 3]];
                let row_in = y - (sy as usize + 1);
                let r = if attr & 0x80 != 0 { h - 1 - row_in } else { row_in };
                let base = if tall {
                    ((tile as u16 & 1) * 0x1000) + ((tile as u16 & 0xFE) + (r / 8) as u16) * 16
                } else {
                    (if self.ctrl & 0x08 != 0 { 0x1000 } else { 0 }) + tile as u16 * 16
                };
                for col in 0..8 {
                    let x = sx as usize + col;
                    if x >= WIDTH || (x < 8 && self.mask & 0x04 == 0) {
                        continue;
                    }
                    let c = if attr & 0x40 != 0 { 7 - col } else { col };
                    let px = self.tile_pixel(base, c, r % 8);
                    if px == 0 || (attr & 0x20 != 0 && bg_opaque[x]) {
                        continue;
                    }
                    row[x] = self.palette[(0x10 + (attr & 3) * 4 + px) as usize];
                }
            }
        }
        self.frame_buf[y * WIDTH..(y + 1) * WIDTH].copy_from_slice(&row);
    }

    fn tile_pixel(&self, tile_addr: u16, x: usize, y: usize) -> u8 {
        let lo = self.chr_byte(tile_addr + y as u16);
        let hi = self.chr_byte(tile_addr + y as u16 + 8);
        ((lo >> (7 - x)) & 1) | (((hi >> (7 - x)) & 1) << 1)
    }
}

fn palette_index(addr: u16) -> usize {
    let i = addr as usize & 0x1F;
    if i >= 0x10 && i & 3 == 0 { i - 0x10 } else { i }
}

/// MMC1 mirroring: 0 = one-screen low, 1 = one-screen high, 2 = vertical, 3 = horizontal.
fn nametable_index(addr: u16, mirroring: u8) -> usize {
    let a = (addr as usize - 0x2000) & 0xFFF;
    let table = a / 0x400;
    let offset = a % 0x400;
    let page = match mirroring {
        0 => 0,
        1 => 1,
        2 => table & 1,
        _ => table >> 1,
    };
    page * 0x400 + offset
}
