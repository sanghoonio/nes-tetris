//! Drawing the game screen during vblank: playfield rows, the line-clear animation, the lines,
//! level, score and statistics panels, the level's palette, and the Tetris flash.

use crate::shadow::{Ret, Routine};
use crate::vars::Player;
use crate::{layout, Game};

/// `outOfDateRenderFlags` bits: which panels need redrawing.
const RENDER_LINES: u8 = 0x01;
const RENDER_LEVEL: u8 = 0x02;
const RENDER_SCORE: u8 = 0x04;
const RENDER_STATS: u8 = 0x40;
/// Play state during the line-clear animation.
const PLAY_STATE_LINE_CLEAR_ANIMATION: u8 = 4;
const SFX_TETRIS_FLASH: u8 = 0x09;
/// Blank tile drawn over cleared rows, column pair by column pair.
const TILE_BLANK: u8 = 0xFF;
/// Nametable addresses of the panels.
const LINES_ONE_PLAYER: u16 = 0x2073;
const LINES_PLAYER1: u16 = 0x2068;
const LINES_PLAYER2: u16 = 0x207A;
const LEVEL: u16 = 0x22BA;
const SCORE: u16 = 0x2118;
/// Background colour 3 of palette 3, which flashes white during a Tetris.
const FLASH_COLOR: u16 = 0x3F0E;
const WHITE: u8 = 0x30;

/// `vramRow` is parked here once the whole playfield has been redrawn.
const VRAM_ROW_DONE: u8 = 0x20;

/// Which playfield a routine is working on: the page of `playfieldAddr` decides.
const PAGE_PLAYER1: u16 = 0x04;
const PAGE_PLAYER2: u16 = 0x05;

impl Game {
    /// `render_mode_play_and_demo`: each frame of play, redraw up to four playfield rows (or
    /// step the line-clear animation), any out-of-date panels, and the Tetris flash.
    pub fn render_mode_play_and_demo(&mut self) {
        self.render_playfield(1);
        if self.vars.number_of_players == 2 {
            self.render_playfield(2);
        }

        let flags = self.vars.out_of_date_render_flags;
        if flags & RENDER_LINES != 0 {
            if self.vars.number_of_players == 2 {
                self.draw_lines(LINES_PLAYER1, self.vars.player1.lines.0);
                self.draw_lines(LINES_PLAYER2, self.vars.player2.lines.0);
            } else {
                self.draw_lines(LINES_ONE_PLAYER, self.vars.player1.lines.0);
            }
            self.vars.out_of_date_render_flags &= !RENDER_LINES;
        }
        let one_player = self.vars.number_of_players != 2;
        if one_player && self.vars.out_of_date_render_flags & RENDER_LEVEL != 0 {
            let shown = self.rom.table(layout::LEVEL_DISPLAY_TABLE, self.vars.player1.level_number as usize);
            self.ppu_addr(LEVEL);
            self.two_digs_to_ppu(shown);
            self.update_palette_for_level();
            self.vars.out_of_date_render_flags &= !RENDER_LEVEL;
        }
        if one_player && self.vars.out_of_date_render_flags & RENDER_SCORE != 0 {
            let score = self.vars.player1.score.0;
            self.ppu_addr(SCORE);
            for digits in score.iter().rev() {
                self.two_digs_to_ppu(*digits);
            }
            self.vars.out_of_date_render_flags &= !RENDER_SCORE;
        }
        if one_player && self.vars.out_of_date_render_flags & RENDER_STATS != 0 {
            for piece in 0..7u8 {
                self.vars.tmp_current_piece = piece;
                let i = piece as u16 * 2;
                // Big-endian VRAM addresses, one per piece type.
                let addr = u16::from_be_bytes([
                    self.rom.byte(layout::PIECE_TO_PPU_STAT_ADDR + i),
                    self.rom.byte(layout::PIECE_TO_PPU_STAT_ADDR + i + 1),
                ]);
                self.ppu_addr(addr);
                self.ppu_data(self.vars.stats_by_type[i as usize + 1]);
                self.two_digs_to_ppu(self.vars.stats_by_type[i as usize]);
            }
            self.vars.tmp_current_piece = 7;
            self.vars.out_of_date_render_flags &= !RENDER_STATS;
        }

        // During a Tetris the background flashes white every 4th frame, with a sound every 8th.
        let mut flash = 0;
        let frame = self.vars.frame_counter.0 as u8;
        if self.vars.active.completed_lines == 4 && frame & 0x03 == 0 {
            flash = WHITE;
            if frame & 0x07 == 0 {
                self.vars.sfx_init[1] = SFX_TETRIS_FLASH;
            }
        }
        self.ppu_addr(FLASH_COLOR);
        self.ppu_data(flash);
        self.vars.ppu_scroll_x = 0;
        self.vars.ppu_scroll_y = 0;
        self.ppu_scroll(0, 0);
    }

    /// One player's playfield: the line-clear animation while it runs, otherwise four rows of
    /// the incremental redraw. The work happens on `active`, loaded from and saved back to the
    /// player's copy.
    fn render_playfield(&mut self, player: u8) {
        let page = if player == 1 { PAGE_PLAYER1 } else { PAGE_PLAYER2 };
        self.vars.playfield_addr.0 = (page << 8) | (self.vars.playfield_addr.0 & 0xFF);
        let copy = if player == 1 { self.vars.player1.clone() } else { self.vars.player2.clone() };
        let saved = if copy.play_state == PLAY_STATE_LINE_CLEAR_ANIMATION {
            let a = &mut self.vars.active;
            a.row_y = copy.row_y;
            a.completed_row = copy.completed_row;
            a.play_state = copy.play_state;
            self.update_line_clearing_animation();
            Player { row_y: self.vars.active.row_y, play_state: self.vars.active.play_state, vram_row: 0, ..copy }
        } else {
            self.vars.active.vram_row = copy.vram_row;
            for _ in 0..4 {
                self.copy_playfield_row_to_vram();
            }
            Player { vram_row: self.vars.active.vram_row, ..copy }
        };
        if player == 1 {
            self.vars.player1 = saved;
        } else {
            self.vars.player2 = saved;
        }
    }

    /// Lines counter: hundreds digit, then the BCD tens and ones.
    fn draw_lines(&mut self, addr: u16, lines: [u8; 2]) {
        self.ppu_addr(addr);
        self.ppu_data(lines[1]);
        self.two_digs_to_ppu(lines[0]);
    }

    /// `twoDigsToPPU`: write a BCD byte as two digit tiles (the digit tiles are 0..=9).
    pub fn two_digs_to_ppu(&mut self, bcd: u8) {
        self.ppu_data(bcd >> 4);
        self.ppu_data(bcd & 0x0F);
    }

    /// Nametable address of the start of playfield row `row`, for the active playfield.
    /// `vramPlayfieldRows` gives the one-player column; the low byte is shifted (8-bit, as
    /// the original adds it) for the one-player layout or either two-player field.
    fn playfield_row_vram(&self, row: u8) -> u16 {
        let entry = layout::VRAM_PLAYFIELD_ROWS + row.wrapping_shl(1) as u16;
        let (low, high) = (self.rom.byte(entry), self.rom.byte(entry + 1));
        let low = if self.vars.number_of_players == 1 {
            low.wrapping_add(6)
        } else if self.vars.playfield_addr.0 >> 8 == PAGE_PLAYER2 {
            low.wrapping_add(0x0C)
        } else {
            low.wrapping_sub(2)
        };
        u16::from_be_bytes([high, low])
    }

    /// `copyPlayfieldRowToVRAM`: draw playfield row `vram_row` and move to the next; after
    /// the last row, park `vram_row` at `VRAM_ROW_DONE`.
    pub fn copy_playfield_row_to_vram(&mut self) {
        let row = self.vars.active.vram_row;
        // Signed compare, as `cpx #$15 / bpl`.
        if (row.wrapping_sub(0x15) as i8) >= 0 {
            return;
        }
        let start = self.rom.table(layout::MULT_BY10_TABLE, row as usize);
        let addr = self.playfield_row_vram(row);
        self.ppu_addr(addr);
        for i in 0..10 {
            self.ppu_data(self.playfield_cell(start.wrapping_add(i)));
        }
        let a = &mut self.vars.active;
        a.vram_row = a.vram_row.wrapping_add(1);
        if (a.vram_row.wrapping_sub(0x14) as i8) >= 0 {
            a.vram_row = VRAM_ROW_DONE;
        }
    }

    /// `updateLineClearingAnimation`: every 4th frame, blank one more pair of columns (working
    /// outwards from the middle) in each completed row. After 5 steps, move on.
    pub fn update_line_clearing_animation(&mut self) {
        if self.vars.frame_counter.0 & 0x03 != 0 {
            return;
        }
        let step = self.vars.active.row_y as u16;
        for i in 0..4 {
            let row = self.vars.active.completed_row[i];
            if row == 0 {
                continue;
            }
            let base = self.playfield_row_vram(row);
            let (high, low) = (base & 0xFF00, base as u8);
            for columns in [layout::LEFT_COLUMNS, layout::RIGHT_COLUMNS] {
                let column = self.rom.byte(columns + step);
                self.ppu_addr(high | column.wrapping_add(low) as u16);
                self.ppu_data(TILE_BLANK);
            }
        }
        let a = &mut self.vars.active;
        a.row_y = a.row_y.wrapping_add(1);
        if (a.row_y.wrapping_sub(5) as i8) >= 0 {
            a.play_state = a.play_state.wrapping_add(1);
        }
    }

    /// `updatePaletteForLevel`: load the level's colours (it cycles every 10 levels) into
    /// background palette 2 and sprite palette 2.
    ///
    /// The "mod 10" loop compares signed, so from level 128 up it doesn't reduce at all and the
    /// colours come from past the end of the table: the famous garbage palettes of very high
    /// levels.
    pub fn update_palette_for_level(&mut self) {
        let mut level = self.vars.player1.level_number;
        while (level.wrapping_sub(10) as i8) >= 0 {
            level = level.wrapping_sub(10);
        }
        let entry = layout::COLOR_TABLE + level.wrapping_shl(2) as u16;
        for palette in [0x3F08, 0x3F18] {
            self.ppu_addr(palette);
            for i in 0..4 {
                self.ppu_data(self.rom.byte(entry + i));
            }
        }
    }
}

/// Working variables of the routines above.
const RENDER_SCRATCH: &[&str] = &["general_counter", "general_counter2", "general_counter3"];

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "render_mode_play_and_demo",
        addr: layout::RENDER_MODE_PLAY_AND_DEMO,
        run: |g, _| {
            g.render_mode_play_and_demo();
            Ret::Unit
        },
        scratch: RENDER_SCRATCH,
    },
    Routine {
        name: "twoDigsToPPU",
        addr: layout::TWO_DIGS_TO_PPU,
        run: |g, r| {
            g.two_digs_to_ppu(r.a);
            Ret::Unit
        },
        scratch: RENDER_SCRATCH,
    },
    Routine {
        name: "copyPlayfieldRowToVRAM",
        addr: layout::COPY_PLAYFIELD_ROW_TO_VRAM,
        run: |g, _| {
            g.copy_playfield_row_to_vram();
            Ret::Unit
        },
        scratch: RENDER_SCRATCH,
    },
    Routine {
        name: "updateLineClearingAnimation",
        addr: layout::UPDATE_LINE_CLEARING_ANIMATION,
        run: |g, _| {
            g.update_line_clearing_animation();
            Ret::Unit
        },
        scratch: RENDER_SCRATCH,
    },
    Routine {
        name: "updatePaletteForLevel",
        addr: layout::UPDATE_PALETTE_FOR_LEVEL,
        run: |g, _| {
            g.update_palette_for_level();
            Ret::Unit
        },
        scratch: RENDER_SCRATCH,
    },
];
