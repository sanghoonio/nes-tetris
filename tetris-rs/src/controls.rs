//! Moving the falling piece: shifting with DAS, rotating, and dropping under gravity or Down.

use crate::shadow::{Ret, Routine};
use crate::{buttons, layout, Game};

/// DAS: frames of holding left/right before the first repeat, and between repeats.
const DAS_RESET: u8 = 0x10;
const DAS_DELAY: u8 = 0x0A;
/// Sound effects requested in slot 1.
const SFX_SHIFT: u8 = 0x03;
const SFX_ROTATE: u8 = 0x05;
const PLAY_STATE_LOCK: u8 = 2;
const TILE_EMPTY: u8 = 0xEF;

impl Game {
    /// `playState_playerControlsActiveTetrimino`: one frame of player control.
    pub fn player_controls_active_tetrimino(&mut self) {
        self.shift_tetrimino();
        self.rotate_tetrimino();
        self.drop_tetrimino();
    }

    /// `isPositionValid`: whether the active piece fits at its position. Each of its four cells
    /// must be within rows -2..=19 and columns 0..=9, on an empty cell.
    ///
    /// The cell index is 8-bit, as in the original: a cell above the top row wraps around to the
    /// end of the playfield page, which holds empty tiles.
    pub fn is_position_valid(&self) -> bool {
        let p = &self.vars.active;
        let (x, y) = (p.tetrimino_x, p.tetrimino_y);
        // y * 10 as the original computes it (y*8 + y*2, whose carry leaks into "+ x").
        let (y10, carry) = y.wrapping_shl(3).overflowing_add(y.wrapping_shl(1));
        let origin = y10.wrapping_add(x).wrapping_add(carry as u8);
        let cells = self.rom.bytes(layout::ORIENTATION_TABLE + p.current_piece as u16 * 12, 12);
        cells.chunks(3).all(|cell| {
            let (dy, dx) = (cell[0], cell[2]);
            // Row check: dy + y + 2 < 22, again with the carry of the first add leaking in.
            let (row, carry) = dy.overflowing_add(y);
            let row_ok = row.wrapping_add(2).wrapping_add(carry as u8) < 22;
            let index = dy.wrapping_mul(10).wrapping_add(origin).wrapping_add(dx);
            let empty = self.playfield_cell(index) >= TILE_EMPTY;
            let col_ok = dx.wrapping_add(x) < 10;
            row_ok && empty && col_ok
        })
    }

    /// `shift_tetrimino`: move left/right. A fresh press moves at once; holding repeats after
    /// DAS_RESET frames, then every DAS_RESET - DAS_DELAY frames. Down blocks shifting. A blocked
    /// move charges DAS fully so the next frame retries.
    pub fn shift_tetrimino(&mut self) {
        let held = self.vars.held_buttons;
        let pressed = self.vars.newly_pressed_buttons;
        let original_x = self.vars.active.tetrimino_x;
        self.vars.original_y = original_x;
        if held & buttons::DOWN != 0 {
            return;
        }
        if pressed & (buttons::LEFT | buttons::RIGHT) != 0 {
            self.vars.active.autorepeat_x = 0;
        } else {
            if held & (buttons::LEFT | buttons::RIGHT) == 0 {
                return;
            }
            let p = &mut self.vars.active;
            p.autorepeat_x = p.autorepeat_x.wrapping_add(1);
            // Signed compare, as the original's `cmp / bmi`.
            if (p.autorepeat_x.wrapping_sub(DAS_RESET) as i8) < 0 {
                return;
            }
            p.autorepeat_x = DAS_DELAY;
        }
        let step: i8 = if held & buttons::RIGHT != 0 {
            1
        } else if held & buttons::LEFT != 0 {
            -1
        } else {
            return;
        };
        let p = &mut self.vars.active;
        p.tetrimino_x = p.tetrimino_x.wrapping_add_signed(step);
        if self.is_position_valid() {
            self.vars.sfx_init[1] = SFX_SHIFT;
        } else {
            self.vars.active.tetrimino_x = original_x;
            self.vars.active.autorepeat_x = DAS_RESET;
        }
    }

    /// `rotate_tetrimino`: A rotates clockwise, B anticlockwise, if the new orientation fits.
    pub fn rotate_tetrimino(&mut self) {
        let original = self.vars.active.current_piece;
        self.vars.original_y = original;
        let pressed = self.vars.newly_pressed_buttons;
        // rotationTable holds (anticlockwise, clockwise) per orientation.
        let entry = layout::ROTATION_TABLE + original as u16 * 2;
        let rotated = if pressed & buttons::A != 0 {
            self.rom.byte(entry + 1)
        } else if pressed & buttons::B != 0 {
            self.rom.byte(entry)
        } else {
            return;
        };
        self.vars.active.current_piece = rotated;
        if self.is_position_valid() {
            self.vars.sfx_init[1] = SFX_ROTATE;
        } else {
            self.vars.active.current_piece = original;
        }
    }

    /// `drop_tetrimino`: gravity and soft drop. Holding Down (alone) drops a row every other
    /// frame, scoring a point per row. At the start of a game `autorepeat_y` is negative and
    /// counts up as an entry delay that pressing Down cuts short. When the piece can't move down
    /// it locks.
    pub fn drop_tetrimino(&mut self) {
        let held = self.vars.held_buttons;
        let pressed = self.vars.newly_pressed_buttons;
        let p = &mut self.vars.active;

        if (p.autorepeat_y as i8) < 0 {
            if pressed & buttons::DOWN == 0 {
                p.autorepeat_y = p.autorepeat_y.wrapping_add(1);
                return;
            }
            p.autorepeat_y = 0;
        }

        if p.autorepeat_y != 0 {
            if held & buttons::DPAD != buttons::DOWN {
                p.autorepeat_y = 0;
                p.hold_down_points = 0;
            } else {
                p.autorepeat_y = p.autorepeat_y.wrapping_add(1);
                if p.autorepeat_y >= 3 {
                    p.autorepeat_y = 1;
                    p.hold_down_points = p.hold_down_points.wrapping_add(1);
                    self.drop_one_row();
                    return;
                }
            }
        } else if held & (buttons::LEFT | buttons::RIGHT) == 0 && pressed & buttons::DPAD == buttons::DOWN {
            p.autorepeat_y = 1;
        }

        let speed = self.rom.frames_per_drop(self.vars.active.level_number);
        self.vars.drop_speed = speed;
        // Signed compare, as the original's `cmp / bpl`.
        if (self.vars.active.fall_timer.wrapping_sub(speed) as i8) >= 0 {
            self.drop_one_row();
        }
    }

    /// Move the piece down a row, or lock it if it can't go.
    fn drop_one_row(&mut self) {
        let p = &mut self.vars.active;
        p.fall_timer = 0;
        let original_y = p.tetrimino_y;
        self.vars.original_y = original_y;
        p.tetrimino_y = p.tetrimino_y.wrapping_add(1);
        if self.is_position_valid() {
            return;
        }
        let p = &mut self.vars.active;
        p.tetrimino_y = original_y;
        p.play_state = PLAY_STATE_LOCK;
        self.update_playfield();
    }

    /// `updatePlayfield`: mark rows from two above the piece downwards for redrawing.
    pub fn update_playfield(&mut self) {
        let top = (self.vars.active.tetrimino_y.wrapping_sub(2) as i8).max(0) as u8;
        // Signed compare, as the original's `cmp / bpl`.
        if (top.wrapping_sub(self.vars.active.vram_row) as i8) < 0 {
            self.vars.active.vram_row = top;
        }
    }
}

/// isPositionValid's working variables (it leaves the result in `general_counter` too, but every
/// caller tests the zero flag instead).
const POSITION_SCRATCH: &[&str] = &[
    "general_counter",
    "general_counter2",
    "general_counter3",
    "general_counter4",
    "selecting_level_or_height",
];
const MOVE_SCRATCH: &[&str] = &[
    "general_counter",
    "general_counter2",
    "general_counter3",
    "general_counter4",
    "selecting_level_or_height",
    "original_y",
];

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "isPositionValid",
        addr: layout::IS_POSITION_VALID,
        run: |g, _| Ret::Zero(g.is_position_valid()),
        scratch: POSITION_SCRATCH,
    },
    Routine {
        name: "shift_tetrimino",
        addr: layout::SHIFT_TETRIMINO,
        run: |g, _| {
            g.shift_tetrimino();
            Ret::Unit
        },
        scratch: MOVE_SCRATCH,
    },
    Routine {
        name: "rotate_tetrimino",
        addr: layout::ROTATE_TETRIMINO,
        run: |g, _| {
            g.rotate_tetrimino();
            Ret::Unit
        },
        scratch: MOVE_SCRATCH,
    },
    Routine {
        name: "drop_tetrimino",
        addr: layout::DROP_TETRIMINO,
        run: |g, _| {
            g.drop_tetrimino();
            Ret::Unit
        },
        scratch: MOVE_SCRATCH,
    },
    Routine {
        name: "updatePlayfield",
        addr: layout::UPDATE_PLAYFIELD,
        run: |g, _| {
            g.update_playfield();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "playState_playerControlsActiveTetrimino",
        addr: layout::PLAY_STATE_PLAYER_CONTROLS_ACTIVE_TETRIMINO,
        run: |g, _| {
            g.player_controls_active_tetrimino();
            Ret::Unit
        },
        scratch: MOVE_SCRATCH,
    },
];
