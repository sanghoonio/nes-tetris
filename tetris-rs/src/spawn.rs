//! Choosing and spawning the next piece, and the piece statistics.

use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

/// The falling piece's play state, where the player controls it.
const PLAY_STATE_FALLING: u8 = 1;
/// The piece has come down to this row (`vramRow`) before the next one may spawn.
const VRAM_ROW_READY: u8 = 0x20;
/// Left and right pieces spawn in column 5 of the playfield, at the top.
const SPAWN_X: u8 = 5;
const SPAWN_Y: u8 = 0;
/// Two-player: frames the second player's piece waits after the delay starts.
const TWO_PLAYER_PIECE_DELAY_FRAMES: u8 = 0x1C;
const ONE_PLAYER: u8 = 1;
/// The game mode that plays back a recorded demo.
const GAME_MODE_DEMO: u8 = 5;
/// The spawn table's used entries; index 7 is excluded (the table's extra trailing byte is
/// never read).
const SPAWN_TABLE_INVALID_INDEX: u8 = 7;
/// Render flag for the statistics panel.
const RENDER_STATS: u8 = 0x40;

impl Game {
    /// `playState_spawnNextTetrimino`: once the falling piece has reached row `VRAM_ROW_READY`,
    /// spawn the queued piece at the top and queue the one after it. In two-player games the
    /// next piece is chosen when a delay starts, and the spawn waits for the delay to run out.
    pub fn play_state_spawn_next_tetrimino(&mut self) {
        // Signed compare, as the original's `cmp / bmi`.
        if (self.vars.active.vram_row.wrapping_sub(VRAM_ROW_READY) as i8) < 0 {
            return;
        }
        if self.vars.number_of_players != ONE_PLAYER {
            if self.vars.two_player_piece_delay_counter == 0 {
                self.vars.two_player_piece_delay_counter = 1;
                self.vars.two_player_piece_delay_player = self.vars.active_player;
                self.vars.two_player_piece_delay_piece = self.choose_next_tetrimino();
                return;
            }
            if self.vars.two_player_piece_delay_player != self.vars.active_player
                || self.vars.two_player_piece_delay_counter != TWO_PLAYER_PIECE_DELAY_FRAMES
            {
                return;
            }
        }
        self.spawn_piece();
    }

    /// The spawn half of `playState_spawnNextTetrimino`: put the queued piece at the top, count
    /// it, and queue the next one (from the second player's delay, in two-player games).
    fn spawn_piece(&mut self) {
        self.vars.two_player_piece_delay_counter = 0;
        let p = &mut self.vars.active;
        p.fall_timer = 0;
        p.tetrimino_y = SPAWN_Y;
        p.play_state = PLAY_STATE_FALLING;
        p.tetrimino_x = SPAWN_X;
        let orientation = self
            .rom
            .byte(layout::SPAWN_ORIENTATION_FROM_ORIENTATION + self.vars.next_piece as u16);
        self.vars.active.current_piece = orientation;
        self.increment_piece_stat(orientation);
        if self.vars.number_of_players == ONE_PLAYER {
            self.vars.next_piece = self.choose_next_tetrimino();
        } else {
            self.vars.next_piece = self.vars.two_player_piece_delay_piece;
        }
        self.vars.active.autorepeat_y = 0;
    }

    /// `chooseNextTetrimino`: the spawn id of the next piece. In a demo, the demo's type table
    /// gives it (high nibble of each entry, one entry per piece). Otherwise it's random.
    pub fn choose_next_tetrimino(&mut self) -> u8 {
        if self.vars.game_mode != GAME_MODE_DEMO {
            return self.pick_random_tetrimino();
        }
        let index = self.vars.demo_index;
        self.vars.demo_index = index.wrapping_add(1);
        let entry = self.rom.byte(layout::DEMO_TETRIMINO_TYPE_TABLE + index as u16);
        let spawn_index = (entry >> 4) & 7;
        self.rom.byte(layout::SPAWN_TABLE + spawn_index as u16)
    }

    /// `pickRandomTetrimino` (through its `@realStart` body): pick the next piece from the spawn
    /// table. It normally takes the entry for `(seed high byte + spawnCount) & 7`, but if that
    /// repeats `spawnID` (the last piece dealt), it rolls again from `(seed & 7) + spawnID`,
    /// reduced modulo 7. The leading RNG step under `NWC = 1` is not in this build.
    pub fn pick_random_tetrimino(&mut self) -> u8 {
        self.vars.spawn_count = self.vars.spawn_count.wrapping_add(1);
        let index = self.rng_seed_high().wrapping_add(self.vars.spawn_count) & 7;
        if index != SPAWN_TABLE_INVALID_INDEX {
            let piece = self.rom.byte(layout::SPAWN_TABLE + index as u16);
            if piece != self.vars.spawn_id {
                self.vars.spawn_id = piece;
                return piece;
            }
        }
        self.step_rng();
        // The original's `clc / adc spawnID` drops the carry, so this sum wraps at 8 bits.
        let mut index = (self.rng_seed_high() & 7).wrapping_add(self.vars.spawn_id);
        while index >= SPAWN_TABLE_INVALID_INDEX {
            index -= SPAWN_TABLE_INVALID_INDEX;
        }
        let piece = self.rom.byte(layout::SPAWN_TABLE + index as u16);
        self.vars.spawn_id = piece;
        piece
    }

    /// The high byte of `rng_seed`, which the original reads with `lda rng_seed`.
    fn rng_seed_high(&self) -> u8 {
        (self.vars.rng_seed.0 >> 8) as u8
    }

    /// `incrementPieceStat`: count one more piece of the given orientation's type. Each count is
    /// a two-byte BCD number in `stats_by_type`; the BCD carry is done by hand, adding 6 past a
    /// nibble of 9 and 0x60 past 0x99. Then the statistics panel is flagged for redrawing.
    pub fn increment_piece_stat(&mut self, orientation: u8) {
        let piece_type = self.rom.byte(layout::TETRIMINO_TYPE_FROM_ORIENTATION + orientation as u16);
        let index = (piece_type << 1) as usize;
        let stats = &mut self.vars.stats_by_type;
        let mut count = stats[index].wrapping_add(1);
        // Signed compare, as the original's `cmp / bmi`: a low nibble of 10 or more carries.
        if ((count & 0x0F).wrapping_sub(10) as i8) >= 0 {
            count = count.wrapping_add(6);
            if count >= 0xA0 {
                count = count.wrapping_add(0x60);
                stats[index + 1] = stats[index + 1].wrapping_add(1);
            }
        }
        self.vars.general_counter = count;
        stats[index] = count;
        self.vars.out_of_date_render_flags |= RENDER_STATS;
    }
}

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "playState_spawnNextTetrimino",
        addr: layout::PLAY_STATE_SPAWN_NEXT_TETRIMINO,
        run: |g, _| {
            g.play_state_spawn_next_tetrimino();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "chooseNextTetrimino",
        addr: layout::CHOOSE_NEXT_TETRIMINO,
        run: |g, _| Ret::A(g.choose_next_tetrimino()),
        scratch: &[],
    },
    Routine {
        name: "pickRandomTetrimino",
        addr: layout::PICK_RANDOM_TETRIMINO,
        run: |g, _| Ret::A(g.pick_random_tetrimino()),
        scratch: &[],
    },
    Routine {
        // Not in `layout`: the local label `@realStart` inside pickRandomTetrimino, at 0x9907,
        // where the original's body begins (pickRandomTetrimino just jumps in with `jsr`).
        name: "@realStart",
        addr: 0x9907,
        run: |g, _| Ret::A(g.pick_random_tetrimino()),
        scratch: &[],
    },
    Routine {
        name: "incrementPieceStat",
        addr: layout::INCREMENT_PIECE_STAT,
        // The original takes the orientation in A.
        run: |g, r| {
            g.increment_piece_stat(r.a);
            Ret::Unit
        },
        scratch: &[],
    },
];
