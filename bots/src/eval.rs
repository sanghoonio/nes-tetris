//! How good a board is: a weighted sum of what it's like after a piece locks. The features are
//! the usual ones for Tetris bots (Pierre Dellacherie's, as tuned by El-Tetris), plus a reward
//! for Tetrises, the most points per line.

use crate::search::{Board, HEIGHT, WIDTH};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights {
    /// Per row of height where the piece landed.
    pub landing_height: f64,
    /// Per line cleared by the piece.
    pub lines: f64,
    /// Extra for clearing four at once.
    pub tetris: f64,
    /// Per change between filled and empty along each row (walls count as filled).
    pub row_transitions: f64,
    /// Per change between filled and empty down each column (the floor counts as filled).
    pub column_transitions: f64,
    /// Per empty cell with a filled one somewhere above it.
    pub holes: f64,
    /// Per cell of the wells: empty cells with filled (or wall) on both sides, deeper wells
    /// counting more (1 + 2 + ... for a well's depth).
    pub wells: f64,
}

impl Default for Weights {
    /// El-Tetris's weights, and a modest Tetris bonus.
    fn default() -> Self {
        Weights {
            landing_height: -4.500158825082766,
            lines: 3.4181268101392694,
            tetris: 8.0,
            row_transitions: -3.2178882868487753,
            column_transitions: -9.348695305445199,
            holes: -7.899265427351652,
            wells: -3.3855972247263626,
        }
    }
}

impl Weights {
    /// The board after a piece landed with its position's row `y` and cleared `lines`.
    pub fn score(&self, b: &Board, lines: u32, y: u8) -> f64 {
        let landing = (HEIGHT as f64) - y as f64;
        let filled = |r: i32, c: i32| c < 0 || c >= WIDTH as i32 || b.0[r as usize][c as usize];
        let mut row_t = 0;
        for r in 0..HEIGHT as i32 {
            for c in 0..=WIDTH as i32 {
                if filled(r, c - 1) != filled(r, c) {
                    row_t += 1;
                }
            }
        }
        let (mut col_t, mut holes, mut wells) = (0, 0, 0);
        for c in 0..WIDTH {
            let mut above = false;
            let mut prev = false;
            let mut depth = 0;
            for r in 0..HEIGHT {
                let f = b.0[r][c];
                if f != prev && r > 0 {
                    col_t += 1;
                }
                prev = f;
                if f {
                    above = true;
                    depth = 0;
                } else {
                    if above {
                        holes += 1;
                    }
                    let (l, rr) = (filled(r as i32, c as i32 - 1), filled(r as i32, c as i32 + 1));
                    if l && rr {
                        depth += 1;
                        wells += depth;
                    } else {
                        depth = 0;
                    }
                }
            }
            if !prev {
                col_t += 1; // empty at the bottom against the floor
            }
        }
        self.landing_height * landing
            + self.lines * lines as f64
            + if lines == 4 { self.tetris } else { 0.0 }
            + self.row_transitions * row_t as f64
            + self.column_transitions * col_t as f64
            + self.holes * holes as f64
            + self.wells * wells as f64
    }
}
