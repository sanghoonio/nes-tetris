//! The play states after a piece lands: locking it into the playfield, finding and removing
//! completed rows, two-player garbage, and counting lines, levels and score.

use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

const TILE_EMPTY: u8 = 0xEF;
/// Orientation id meaning "no piece on the board".
const HIDDEN: u8 = 0x13;
/// Garbage blocks, and the hole in a garbage row.
const TILE_GARBAGE: u8 = 0x78;
const TILE_GARBAGE_HOLE: u8 = 0xFF;
/// `vramRow` reaches this once the playfield has been fully redrawn after the last change.
const VRAM_ROW_DONE: u8 = 0x20;

const PLAY_STATE_GAME_OVER_CURTAIN: u8 = 0x0A;
/// `outOfDateRenderFlags` bits.
const RENDER_LINES: u8 = 0x01;
const RENDER_LEVEL: u8 = 0x02;
const RENDER_SCORE: u8 = 0x04;
/// Sound effects (slot 0: game over; slot 1: the rest).
const SFX_GAME_OVER: u8 = 0x02;
const SFX_TETRIS: u8 = 0x04;
const SFX_LEVEL_UP: u8 = 0x06;
const SFX_LOCK: u8 = 0x07;
const SFX_LINE_CLEARING: u8 = 0x0A;
const MODE_DEMO: u8 = 5;
pub(crate) const MUSIC_SILENCE: u8 = 0xFF;

/// Signed 8-bit "a < b", as the original's `cmp` followed by `bmi`.
fn signed_lt(a: u8, b: u8) -> bool {
    (a.wrapping_sub(b) as i8) < 0
}

impl Game {
    /// A byte of the active player's playfield page (`playfieldAddr` points at one of the two).
    pub(crate) fn playfield_cell(&self, index: u8) -> u8 {
        self.playfield_page()[index as usize]
    }

    pub(crate) fn set_playfield_cell(&mut self, index: u8, tile: u8) {
        self.playfield_page_mut()[index as usize] = tile;
    }

    fn playfield_page(&self) -> &[u8; 256] {
        if self.vars.playfield_addr.0 >> 8 == 0x05 {
            &self.vars.playfield_for_second_player
        } else {
            &self.vars.playfield
        }
    }

    fn playfield_page_mut(&mut self) -> &mut [u8; 256] {
        if self.vars.playfield_addr.0 >> 8 == 0x05 {
            &mut self.vars.playfield_for_second_player
        } else {
            &mut self.vars.playfield
        }
    }

    /// `playState_lockTetrimino`: if the piece overlaps the stack the game is over; otherwise,
    /// once the playfield has finished redrawing, write the piece's four tiles into it.
    pub fn lock_tetrimino(&mut self) {
        if !self.is_position_valid() {
            self.vars.sfx_init[0] = SFX_GAME_OVER;
            self.vars.active.play_state = PLAY_STATE_GAME_OVER_CURTAIN;
            self.vars.active.curtain_row = 0xF0;
            self.update_audio2();
            return;
        }
        if signed_lt(self.vars.active.vram_row, VRAM_ROW_DONE) {
            return;
        }
        let p = &self.vars.active;
        let (x, y) = (p.tetrimino_x, p.tetrimino_y);
        // y * 10 + x with the original's carry leak (see is_position_valid).
        let (y10, carry) = y.wrapping_shl(3).overflowing_add(y.wrapping_shl(1));
        let origin = y10.wrapping_add(x).wrapping_add(carry as u8);
        let cells = self.rom.bytes(layout::ORIENTATION_TABLE + p.current_piece as u16 * 12, 12).to_vec();
        for cell in cells.chunks(3) {
            let (dy, tile, dx) = (cell[0], cell[1], cell[2]);
            let index = dy.wrapping_mul(10).wrapping_add(origin).wrapping_add(dx);
            self.set_playfield_cell(index, tile);
        }
        self.vars.active.line_index = 0;
        self.update_playfield();
        self.update_music_speed();
        self.vars.active.play_state = self.vars.active.play_state.wrapping_add(1);
    }

    /// `playState_checkForCompletedRows`: check one of the four rows the piece could have
    /// completed (from two above it), one row per frame. A complete row is removed at once:
    /// everything above moves down and the top row empties. After the fourth row, move on.
    pub fn check_for_completed_rows(&mut self) {
        if signed_lt(self.vars.active.vram_row, VRAM_ROW_DONE) {
            return;
        }
        let p = &self.vars.active;
        let top = if (p.tetrimino_y.wrapping_sub(2) as i8) < 0 { 0 } else { p.tetrimino_y.wrapping_sub(2) };
        let row = top.wrapping_add(p.line_index);
        let start = row.wrapping_mul(10);
        let line_index = p.line_index as usize;

        let complete = (0..10u8).all(|i| self.playfield_cell(start.wrapping_add(i)) != TILE_EMPTY);
        if complete {
            self.vars.sfx_init[1] = SFX_LINE_CLEARING;
            self.vars.active.completed_lines = self.vars.active.completed_lines.wrapping_add(1);
            self.vars.active.completed_row[line_index] = row;
            self.drop_rows_above(start);
            for i in 0..10 {
                self.set_playfield_cell(i, TILE_EMPTY);
            }
            self.vars.active.current_piece = HIDDEN;
        } else {
            self.vars.active.completed_row[line_index] = 0;
        }

        let p = &mut self.vars.active;
        p.line_index = p.line_index.wrapping_add(1);
        if signed_lt(p.line_index, 4) {
            return;
        }
        let garbage = self.rom.table(layout::GARBAGE_LINES, self.vars.active.completed_lines as usize);
        self.vars.pending_garbage_inactive_player = self.vars.pending_garbage_inactive_player.wrapping_add(garbage);
        let p = &mut self.vars.active;
        p.vram_row = 0;
        p.row_y = 0;
        if p.completed_lines == 4 {
            self.vars.sfx_init[1] = SFX_TETRIS;
        }
        let p = &mut self.vars.active;
        p.play_state = p.play_state.wrapping_add(1);
        if p.completed_lines == 0 {
            // Nothing cleared: skip the line-clear animation state too.
            p.play_state = p.play_state.wrapping_add(1);
            self.vars.sfx_init[1] = SFX_LOCK;
        }
    }

    /// Move every cell before playfield index `start` down one row (10 cells).
    ///
    /// The original counts an 8-bit index down from `start - 1` to 0. When the top row itself
    /// completes (`start` = 0) that index starts at 255, so it shifts the whole 256-byte page,
    /// spilling 10 bytes into the next page of RAM.
    fn drop_rows_above(&mut self, start: u8) {
        if start != 0 {
            let page = self.playfield_page_mut();
            for i in (0..start as usize).rev() {
                page[i + 10] = page[i];
            }
            return;
        }
        let base = self.vars.playfield_addr.0 as usize & 0xFF00;
        self.vars.with_ram(|ram| {
            for i in (0..256).rev() {
                ram[(base + i + 10) & 0x7FF] = ram[(base + i) & 0x7FF];
            }
        });
    }

    /// `playState_receiveGarbage` (two-player only): push the playfield up by the pending
    /// garbage rows and fill the bottom with garbage, one hole per row at `garbage_hole`.
    pub fn receive_garbage(&mut self) {
        if self.vars.number_of_players == 1 || self.vars.pending_garbage == 0 {
            self.vars.active.play_state = self.vars.active.play_state.wrapping_add(1);
            return;
        }
        if signed_lt(self.vars.active.vram_row, VRAM_ROW_DONE) {
            return;
        }
        let shift = self.rom.table(layout::MULT_BY10_TABLE, self.vars.pending_garbage as usize);
        let (mut to, mut from) = (0u8, shift);
        loop {
            let tile = self.playfield_cell(from);
            self.set_playfield_cell(to, tile);
            to = to.wrapping_add(1);
            from = from.wrapping_add(1);
            if from == 200 {
                break;
            }
        }
        let hole = self.vars.active.garbage_hole;
        let mut column = 0u8;
        let mut index = to;
        loop {
            self.set_playfield_cell(index, if column == hole { TILE_GARBAGE_HOLE } else { TILE_GARBAGE });
            column += 1;
            if column == 10 {
                column = 0;
            }
            index = index.wrapping_add(1);
            if index == 200 {
                break;
            }
        }
        self.vars.pending_garbage = 0;
        self.vars.active.vram_row = 0;
        self.vars.active.play_state = self.vars.active.play_state.wrapping_add(1);
    }

    /// `playState_updateLinesAndStatistics`: count the cleared lines (A-type up, B-type down),
    /// level up every 10 lines once past the start level, and score the hold-down points and the
    /// line clear. All of it is BCD arithmetic done digit by digit, with the original's quirks.
    pub fn update_lines_and_statistics(&mut self) {
        self.update_music_speed();
        let cleared = self.vars.active.completed_lines;
        if cleared != 0 {
            self.count_cleared_lines(cleared);
        }
        self.add_hold_down_points();
        self.add_line_clear_points();
    }

    fn count_cleared_lines(&mut self, cleared: u8) {
        // Clears of each size (1..=4 lines), one BCD byte each.
        let stat = &mut self.vars.line_clear_stats_by_type[cleared as usize - 1];
        *stat = stat.wrapping_add(1);
        if *stat & 0x0F >= 0x0A {
            *stat = stat.wrapping_add(6);
        }
        self.vars.out_of_date_render_flags |= RENDER_LINES;

        if self.vars.game_type != 0 {
            // B-type counts down to zero.
            let lines = &mut self.vars.active.lines.0;
            let (low, _) = lines[0].overflowing_sub(cleared);
            lines[0] = low;
            if (low as i8) < 0 {
                lines[0] = 0;
            } else if low & 0x0F >= 0x0A {
                lines[0] = low.wrapping_sub(6);
            }
            return;
        }

        for _ in 0..cleared {
            let lines = &mut self.vars.active.lines.0;
            lines[0] = lines[0].wrapping_add(1);
            if lines[0] & 0x0F >= 0x0A {
                lines[0] = lines[0].wrapping_add(6);
                if lines[0] & 0xF0 >= 0xA0 {
                    lines[0] &= 0x0F;
                    lines[1] = lines[1].wrapping_add(1);
                }
            }
            if lines[0] & 0x0F != 0 {
                continue;
            }
            // A new ten: level up if the tens count has passed the level (signed compare).
            let tens = (u16::from_le_bytes(*lines) >> 4) as u8;
            if !signed_lt(self.vars.active.level_number, tens) {
                continue;
            }
            self.vars.active.level_number = self.vars.active.level_number.wrapping_add(1);
            self.vars.sfx_init[1] = SFX_LEVEL_UP;
            self.vars.out_of_date_render_flags |= RENDER_LEVEL;
        }
    }

    /// One point per row soft-dropped, less one (`holdDownPoints` counts from 1). Only the
    /// score's low byte takes them.
    fn add_hold_down_points(&mut self) {
        let points = self.vars.active.hold_down_points;
        if signed_lt(points, 2) {
            return;
        }
        let score = &mut self.vars.active.score.0;
        score[0] = score[0].wrapping_sub(1).wrapping_add(points);
        if score[0] & 0x0F >= 0x0A {
            score[0] = score[0].wrapping_add(6);
        }
        if score[0] & 0xF0 >= 0xA0 {
            // Quirk: the carry into the hundreds is done on the masked tens digit, which
            // drops the ones digit.
            score[0] = (score[0] & 0xF0).wrapping_add(0x60);
            score[1] = score[1].wrapping_add(1);
        }
        self.vars.out_of_date_render_flags |= RENDER_SCORE;
    }

    /// Points for the clear (`pointsTable[lines]`, BCD) times (level + 1), added one level's
    /// worth at a time. The score saturates at 999999.
    fn add_line_clear_points(&mut self) {
        self.vars.active.hold_down_points = 0;
        let mut times = self.vars.active.level_number.wrapping_add(1);
        let entry = layout::POINTS_TABLE + self.vars.active.completed_lines.wrapping_shl(1) as u16;
        let (low, high) = (self.rom.byte(entry), self.rom.byte(entry + 1));
        loop {
            let score = &mut self.vars.active.score.0;
            score[0] = score[0].wrapping_add(low);
            if score[0] >= 0xA0 {
                score[0] = score[0].wrapping_add(0x60);
                score[1] = score[1].wrapping_add(1);
            }
            score[1] = score[1].wrapping_add(high);
            if score[1] & 0x0F >= 0x0A {
                score[1] = score[1].wrapping_add(6);
            }
            if score[1] & 0xF0 >= 0xA0 {
                score[1] = score[1].wrapping_add(0x60);
                score[2] = score[2].wrapping_add(1);
            }
            if score[2] & 0x0F >= 0x0A {
                score[2] = score[2].wrapping_add(6);
            }
            if score[2] & 0xF0 >= 0xA0 {
                *score = [0x99; 3];
            }
            times = times.wrapping_sub(1);
            if times == 0 {
                break;
            }
        }
        self.vars.out_of_date_render_flags |= RENDER_SCORE;
        self.vars.active.completed_lines = 0;
        self.vars.active.play_state = self.vars.active.play_state.wrapping_add(1);
    }

    /// `updateMusicSpeed`: switch to the fast version of the music while anything is stacked
    /// as high as row 5, and back when it clears.
    pub fn update_music_speed(&mut self) {
        let row5 = self.rom.table(layout::MULT_BY10_TABLE, 5);
        let high_stack = (0..10u8).any(|i| self.playfield_cell(row5.wrapping_add(i)) != TILE_EMPTY);
        let music_type = self.vars.music_type as u16;
        if high_stack && self.vars.allegro == 0 {
            self.vars.allegro = 0xFF;
            let track = self.rom.byte(layout::MUSIC_SELECTION_TABLE + music_type + 4);
            self.set_music_track(track);
        } else if !high_stack && self.vars.allegro != 0 {
            self.vars.allegro = 0;
            let track = self.rom.byte(layout::MUSIC_SELECTION_TABLE + music_type);
            self.set_music_track(track);
        }
    }

    /// `setMusicTrack`: request a track (the demo plays none).
    pub fn set_music_track(&mut self, track: u8) {
        self.vars.music_track = if self.vars.game_mode == MODE_DEMO { MUSIC_SILENCE } else { track };
    }
}

/// Working variables of the routines above, which leave nothing a later routine reads.
const ROW_SCRATCH: &[&str] = &[
    "general_counter",
    "general_counter2",
    "general_counter3",
    "general_counter4",
    "general_counter5",
    "selecting_level_or_height",
    // isPositionValid's scratch, via lock.
];

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "playState_lockTetrimino",
        addr: layout::PLAY_STATE_LOCK_TETRIMINO,
        run: |g, _| {
            g.lock_tetrimino();
            Ret::Unit
        },
        scratch: ROW_SCRATCH,
    },
    Routine {
        name: "playState_checkForCompletedRows",
        addr: layout::PLAY_STATE_CHECK_FOR_COMPLETED_ROWS,
        run: |g, _| {
            g.check_for_completed_rows();
            Ret::Unit
        },
        scratch: ROW_SCRATCH,
    },
    Routine {
        name: "playState_receiveGarbage",
        addr: layout::PLAY_STATE_RECEIVE_GARBAGE,
        run: |g, _| {
            g.receive_garbage();
            Ret::Unit
        },
        scratch: ROW_SCRATCH,
    },
    Routine {
        name: "playState_updateLinesAndStatistics",
        addr: layout::PLAY_STATE_UPDATE_LINES_AND_STATISTICS,
        run: |g, _| {
            g.update_lines_and_statistics();
            Ret::Unit
        },
        scratch: ROW_SCRATCH,
    },
    Routine {
        name: "updateMusicSpeed",
        addr: layout::UPDATE_MUSIC_SPEED,
        run: |g, _| {
            g.update_music_speed();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "setMusicTrack",
        addr: layout::SET_MUSIC_TRACK,
        run: |g, r| {
            g.set_music_track(r.a);
            Ret::Unit
        },
        scratch: &[],
    },
];
