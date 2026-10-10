//! The high score table: showing it, shifting entries down for a new score, and bonus points.

use crate::shadow::{Regs, Ret, Routine};
use crate::{layout, Game};

/// Each row of the table: a 6-letter name, a 3-byte BCD score, and a level byte.
const NAME_LEN: u8 = 6;
const NAME_STRIDE: u8 = 6;
const SCORE_LEN: u8 = 3;
/// Game type B's rows start after game type A's four rows (4 * 6 name bytes, 4 * 3 score bytes,
/// 4 level bytes).
const TABLE_B_FIRST_ENTRY: u8 = 4;
const TABLE_B_NAME_OFFSET: u8 = 0x18;
const TABLE_B_SCORE_OFFSET: u8 = 0x0C;
const TABLE_B_LEVEL_OFFSET: u8 = 4;
/// The entry index just past the last row of each game type's table.
const TABLE_A_END: u8 = 3;
const TABLE_B_END: u8 = 7;
/// Blank tile drawn after each name and score.
const TILE_BLANK: u8 = 0xFF;
/// The 24 score bytes are one run in RAM: table A's 12, then table B's 12.
const SCORES_A_LEN: usize = 12;
/// `outOfDateRenderFlags` bit: the letter being entered needs drawing.
const RENDER_HIGH_SCORE_LETTER: u8 = 0x80;

impl Game {
    /// `showHighScores`: draw the high score table(s) for one player. Game type A's rows come
    /// first, then B's, depending on which type is being shown.
    pub fn show_high_scores(&mut self) {
        if self.vars.number_of_players != 1 {
            return;
        }
        self.bulk_copy_to_ppu(layout::HIGH_SCORES_NAMETABLE);
        let mut entry = if self.vars.game_type != 0 { TABLE_B_FIRST_ENTRY } else { 0 };
        loop {
            self.ppu_addr(self.high_score_ppu_addr(entry));
            for i in 0..NAME_LEN {
                let letter = self.vars.high_score_names[(entry * NAME_STRIDE + i) as usize];
                let tile = self.rom.table(layout::HIGH_SCORE_CHAR_TO_TILE, letter as usize);
                self.ppu_data(tile);
            }
            self.ppu_data(TILE_BLANK);
            for i in 0..SCORE_LEN {
                let bcd = self.high_score_score(entry * SCORE_LEN + i);
                self.two_digs_to_ppu(bcd);
            }
            self.ppu_data(TILE_BLANK);
            let level = self.vars.high_score_levels[entry as usize];
            let bcd = self.rom.table(layout::BYTE_TO_BCD_TABLE, level as usize);
            self.two_digs_to_ppu(bcd);

            entry += 1;
            if entry == TABLE_A_END || entry == TABLE_B_END {
                break;
            }
        }
        self.vars.general_counter2 = entry;
    }

    /// `copyHighScoreNameToNextIndex`: move the name at row `a` down one row (`a + 6`), within
    /// game type B's rows when that is the game type. Letters are copied last to first.
    pub fn copy_high_score_name_to_next_index(&mut self, a: u8) {
        let base = if self.vars.game_type != 0 { a.wrapping_add(TABLE_B_NAME_OFFSET) } else { a };
        for i in (0..NAME_LEN).rev() {
            let from = base.wrapping_add(i);
            let letter = self.vars.high_score_names[from as usize];
            self.vars.high_score_names[from.wrapping_add(NAME_STRIDE) as usize] = letter;
            self.vars.general_counter3 = letter;
        }
        // The original's index counter runs down past zero and is left at $FF.
        self.vars.general_counter = base;
        self.vars.general_counter2 = 0xFF;
    }

    /// `copyHighScoreScoreToNextIndex`: move the 3-byte score at row `a` down one row (`a + 3`).
    pub fn copy_high_score_score_to_next_index(&mut self, a: u8) {
        let from = if self.vars.game_type != 0 { a.wrapping_add(TABLE_B_SCORE_OFFSET) } else { a };
        for i in 0..SCORE_LEN {
            let score = self.high_score_score(from.wrapping_add(i));
            self.set_high_score_score(from.wrapping_add(i).wrapping_add(SCORE_LEN), score);
        }
    }

    /// `copyHighScoreLevelToNextIndex`: move the level at row `a` down one row (`a + 1`).
    pub fn copy_high_score_level_to_next_index(&mut self, a: u8) {
        let from = if self.vars.game_type != 0 { a.wrapping_add(TABLE_B_LEVEL_OFFSET) } else { a } as usize;
        self.vars.high_score_levels[from + 1] = self.vars.high_score_levels[from];
    }

    /// `render_mode_congratulations_screen`: draw the letter being entered at its position in
    /// the name, once it has changed, then clear the flag.
    pub fn render_mode_congratulations_screen(&mut self) {
        if self.vars.out_of_date_render_flags & RENDER_HIGH_SCORE_LETTER == 0 {
            return;
        }
        let (hi, lo) = self.high_score_ppu_addr_bytes(self.vars.high_score_entry_raw_pos);
        // The original leaves the row's low address byte in generalCounter.
        self.vars.general_counter = lo;
        let letter_offset = self.vars.high_score_entry_name_offset_for_letter;
        self.ppu_addr(u16::from_be_bytes([hi, lo.wrapping_add(letter_offset)]));
        let letter = self.vars.high_score_entry_current_letter;
        let tile = self.rom.table(layout::HIGH_SCORE_CHAR_TO_TILE, letter as usize);
        self.ppu_data(tile);
        self.vars.ppu_scroll_x = 0;
        self.vars.ppu_scroll_y = 0;
        self.ppu_scroll(0, 0);
        self.vars.out_of_date_render_flags = 0;
    }

    /// `add100Points`: add 100 to the BCD total score. The hundreds digit is the low nibble of
    /// `totalScore+1`, so adding 1 there and correcting each nibble past 9 does the BCD add.
    pub fn add_100_points(&mut self) {
        let total = &mut self.vars.total_score.0;
        total[1] = total[1].wrapping_add(1);
        if total[1] & 0x0F >= 0x0A {
            total[1] = total[1].wrapping_add(6);
        }
        // A carry out of the high nibble adds 1 to the next byte, which the original does with
        // an $60 add plus an increment.
        if total[1] & 0xF0 >= 0xA0 {
            total[1] = total[1].wrapping_add(0x60);
            total[2] = total[2].wrapping_add(1);
        }
        if total[2] & 0x0F >= 0x0A {
            total[2] = total[2].wrapping_add(6);
        }
    }

    /// The PPU address of row `entry`'s name, from `highScorePpuAddrTable` (big-endian words).
    fn high_score_ppu_addr(&self, entry: u8) -> u16 {
        let (hi, lo) = self.high_score_ppu_addr_bytes(entry);
        u16::from_be_bytes([hi, lo])
    }

    fn high_score_ppu_addr_bytes(&self, entry: u8) -> (u8, u8) {
        let t = layout::HIGH_SCORE_PPU_ADDR_TABLE + (entry & 3) as u16 * 2;
        (self.rom.byte(t), self.rom.byte(t + 1))
    }

    /// Byte `i` of the 24-byte score run (table A then table B).
    fn high_score_score(&self, i: u8) -> u8 {
        let i = i as usize;
        if i < SCORES_A_LEN {
            self.vars.high_score_scores_a[i]
        } else {
            self.vars.high_score_scores_b[i - SCORES_A_LEN]
        }
    }

    fn set_high_score_score(&mut self, i: u8, value: u8) {
        let i = i as usize;
        if i < SCORES_A_LEN {
            self.vars.high_score_scores_a[i] = value;
        } else {
            self.vars.high_score_scores_b[i - SCORES_A_LEN] = value;
        }
    }
}

/// No scratch: the loop counters and the tile/address temporaries are left as the original leaves them.
const HIGH_SCORES_SCRATCH: &[&str] = &[];

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "showHighScores",
        addr: layout::SHOW_HIGH_SCORES,
        run: |g, _| {
            g.show_high_scores();
            Ret::Unit
        },
        scratch: HIGH_SCORES_SCRATCH,
    },
    Routine {
        name: "copyHighScoreNameToNextIndex",
        addr: layout::COPY_HIGH_SCORE_NAME_TO_NEXT_INDEX,
        run: |g, r: Regs| {
            g.copy_high_score_name_to_next_index(r.a);
            Ret::Unit
        },
        scratch: HIGH_SCORES_SCRATCH,
    },
    Routine {
        name: "copyHighScoreScoreToNextIndex",
        addr: layout::COPY_HIGH_SCORE_SCORE_TO_NEXT_INDEX,
        run: |g, r: Regs| {
            g.copy_high_score_score_to_next_index(r.a);
            Ret::Unit
        },
        scratch: HIGH_SCORES_SCRATCH,
    },
    Routine {
        name: "copyHighScoreLevelToNextIndex",
        addr: layout::COPY_HIGH_SCORE_LEVEL_TO_NEXT_INDEX,
        run: |g, r: Regs| {
            g.copy_high_score_level_to_next_index(r.a);
            Ret::Unit
        },
        scratch: HIGH_SCORES_SCRATCH,
    },
    Routine {
        name: "render_mode_congratulations_screen",
        addr: layout::RENDER_MODE_CONGRATULATIONS_SCREEN,
        run: |g, _| {
            g.render_mode_congratulations_screen();
            Ret::Unit
        },
        scratch: HIGH_SCORES_SCRATCH,
    },
    Routine {
        name: "add100Points",
        addr: layout::ADD100_POINTS,
        run: |g, _| {
            g.add_100_points();
            Ret::Unit
        },
        scratch: HIGH_SCORES_SCRATCH,
    },
];
