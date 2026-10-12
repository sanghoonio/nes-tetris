//! The playfield as the bot sees it, and where a piece can come to rest: every orientation and
//! column it can be turned and moved to from where it is (on its current row), then dropped
//! straight down.

use crate::shapes::{Shapes, ORIENTATIONS};

pub const WIDTH: usize = 10;
pub const HEIGHT: usize = 20;
/// The playfield in RAM, row by row; a tile below this is a block.
const PLAYFIELD: usize = 0x400;
const EMPTY: u8 = 0xEF;

/// Filled cells, row 0 at the top.
#[derive(Clone, PartialEq, Eq)]
pub struct Board(pub [[bool; WIDTH]; HEIGHT]);

/// Where a piece ends up: its orientation and position when it locks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub orientation: u8,
    pub x: u8,
    pub y: u8,
}

impl Board {
    pub fn from_ram(ram: &[u8; 0x800]) -> Board {
        Board(std::array::from_fn(|r| std::array::from_fn(|c| ram[PLAYFIELD + r * WIDTH + c] < EMPTY)))
    }

    pub fn empty() -> Board {
        Board([[false; WIDTH]; HEIGHT])
    }

    /// Whether orientation `o` fits at (`x`, `y`): every cell in the columns, above the floor and
    /// on an empty cell (cells above the top count as empty).
    pub fn fits(&self, shapes: &Shapes, o: u8, x: i32, y: i32) -> bool {
        shapes.cells[o as usize].iter().all(|&(dy, dx)| {
            let (r, c) = (y + dy as i32, x + dx as i32);
            (0..WIDTH as i32).contains(&c) && r < HEIGHT as i32 && (r < 0 || !self.0[r as usize][c as usize])
        })
    }

    /// The board with `place`'s piece locked in and full rows cleared, and how many were.
    pub fn with(&self, shapes: &Shapes, place: &Placement) -> (Board, u32) {
        let mut b = self.0;
        for &(dy, dx) in &shapes.cells[place.orientation as usize] {
            let (r, c) = (place.y as i32 + dy as i32, place.x as i32 + dx as i32);
            if r >= 0 {
                b[r as usize][c as usize] = true;
            }
        }
        let kept: Vec<[bool; WIDTH]> = b.iter().filter(|row| !row.iter().all(|&f| f)).copied().collect();
        let lines = (HEIGHT - kept.len()) as u32;
        let mut out = [[false; WIDTH]; HEIGHT];
        out[lines as usize..].copy_from_slice(&kept);
        (Board(out), lines)
    }

    /// Each column's height (0 when empty).
    pub fn heights(&self) -> [usize; WIDTH] {
        std::array::from_fn(|c| (0..HEIGHT).find(|&r| self.0[r][c]).map_or(0, |r| HEIGHT - r))
    }
}

/// Every place the piece (orientation `o` at `x`, `y`) can rest: turned and moved on its row,
/// then dropped.
pub fn placements(shapes: &Shapes, board: &Board, o: u8, x: u8, y: u8) -> Vec<Placement> {
    let (x, y) = (x as i32, y as i32);
    if !board.fits(shapes, o, x, y) {
        return vec![];
    }
    // Reachable (orientation, column) on this row: by turning either way and moving a column at a
    // time, each step having to fit.
    let mut seen = [[false; WIDTH + 4]; ORIENTATIONS];
    let mut todo = vec![(o, x)];
    let mut out = vec![];
    seen[o as usize][(x + 2) as usize] = true;
    while let Some((o, x)) = todo.pop() {
        let mut rest = y;
        while board.fits(shapes, o, x, rest + 1) {
            rest += 1;
        }
        out.push(Placement { orientation: o, x: x as u8, y: rest as u8 });
        for (o2, x2) in [(shapes.cw[o as usize], x), (shapes.ccw[o as usize], x), (o, x - 1), (o, x + 1)] {
            if (-2..WIDTH as i32 + 2).contains(&x2) && !seen[o2 as usize][(x2 + 2) as usize] && board.fits(shapes, o2, x2, y) {
                seen[o2 as usize][(x2 + 2) as usize] = true;
                todo.push((o2, x2));
            }
        }
    }
    out
}
