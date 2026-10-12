//! How good a board is: a weighted sum of what it's like after a piece locks.
//!
//! The features are the usual ones for Tetris bots (Pierre Dellacherie's: landing height, row
//! and column transitions, holes, wells) and the ones that make a bot play for Tetrises, as NES
//! players (and StackRabbit, the strongest NES Tetris AI) do: keep the right-hand column open
//! as a well for the long bar, don't spend lines on smaller clears ("burns"), and count the rows
//! ready for a Tetris. The weights are tuned by self-play (`bin/tune.rs`).

use crate::search::{Board, HEIGHT, WIDTH};

/// The column kept open for the long bar.
pub const WELL: usize = WIDTH - 1;

/// What's measured, in the order of `Weights`.
pub const FEATURES: [&str; 12] = [
    "landing height",
    "holes",
    "row transitions",
    "column transitions",
    "bumpiness",
    "stack height",
    "danger",
    "blocks in the well",
    "other wells",
    "burned lines",
    "tetris",
    "tetris ready",
];
pub const N: usize = FEATURES.len();

/// One weight per feature (`FEATURES`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights(pub [f64; N]);

impl Default for Weights {
    fn default() -> Self {
        Weights::DEFAULT
    }
}

impl Weights {
    /// Hand-set starting weights (the tuner's starting point; `TUNED` once it has run).
    pub const DEFAULT: Weights = Weights([-4.5, -8.0, -3.2, -9.3, -1.0, -0.5, -2.0, -6.0, -3.4, -4.0, 30.0, 3.0]);

    /// The weights the tuner found (735 generations; the strongest bot's games scored about 620,000
    /// points on average from mixed starts at 12 to 19), which the bots play with.
    pub const TUNED: Weights = Weights([-4.7684, -14.5448, -2.9392, -11.2736, -0.4415, -0.1793, -2.9444, -7.4475, -3.3131, -5.7657, 20.1585, 3.5964]);

    /// The board after a piece landed with its position's row `y` and cleared `lines`.
    pub fn score(&self, b: &Board, lines: u32, y: u8) -> f64 {
        let f = features(b, lines, y);
        self.0.iter().zip(f).map(|(w, f)| w * f).sum()
    }
}

/// The features of a board after a piece landed at row `y` and cleared `lines`.
pub fn features(b: &Board, lines: u32, y: u8) -> [f64; N] {
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
    let heights = b.heights();
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
                // Wells other than the one kept for the long bar.
                if c != WELL && filled(r as i32, c as i32 - 1) && filled(r as i32, c as i32 + 1) {
                    depth += 1;
                    wells += depth;
                } else {
                    depth = 0;
                }
            }
        }
        if !prev {
            col_t += 1;
        }
    }
    let stack = &heights[..WELL];
    let bumpiness: usize = stack.windows(2).map(|w| w[0].abs_diff(w[1])).sum();
    let tallest = *stack.iter().max().unwrap();
    let average = stack.iter().sum::<usize>() as f64 / stack.len() as f64;
    let danger = (tallest as f64 - 10.0).max(0.0).powi(2);
    // Blocks in the well column (above its lowest hole-free stretch): each one in the way.
    let in_well = (0..HEIGHT).filter(|&r| b.0[r][WELL]).count();
    // Rows ready for a Tetris: full but for the well, with the well open above them.
    let well_top = (0..HEIGHT).find(|&r| b.0[r][WELL]).unwrap_or(HEIGHT);
    let ready = (0..well_top).filter(|&r| (0..WELL).all(|c| b.0[r][c])).count().min(4);
    [
        landing,
        holes as f64,
        row_t as f64,
        col_t as f64,
        bumpiness as f64,
        average,
        danger,
        in_well as f64,
        wells as f64,
        if (1..4).contains(&lines) { lines as f64 } else { 0.0 },
        (lines == 4) as u8 as f64,
        ready as f64,
    ]
}
