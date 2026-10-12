//! The match view's background: the game screen's pattern of outlined tetrominoes, laid afresh
//! across the whole picture. Cutting strips out of the game screen and pasting them side by side
//! cuts its pieces in half; here every piece is whole, except where the picture's edges cut it,
//! as the game screen's own edges do.
//!
//! Each cell is a tile. Its outline follows the game's pattern tiles ($67, $70..$87 in the game's
//! CHR): a light line along the top and left where the piece ends there, a black one along the
//! bottom and right, and a single corner pixel where the piece turns a corner inside.

use tetris_rs::{layout, Rom};

/// The pattern's colours: outline light and dark, and the fill.
const LIGHT: u8 = 0x3C;
const DARK: u8 = 0x0F;
const FILL: u8 = 0x00;

/// The game screen's pattern tiles: every other tile is part of a box.
const PATTERN: [u8; 18] = [0x67, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x77, 0x78, 0x79, 0x7A, 0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x87];

/// The 19 tetrominoes, each as cells (row, column) from its first cell in reading order.
const PIECES: [[(i32, i32); 4]; 19] = [
    [(0, 0), (0, 1), (0, 2), (0, 3)],
    [(0, 0), (1, 0), (2, 0), (3, 0)],
    [(0, 0), (0, 1), (1, 0), (1, 1)],
    [(0, 0), (0, 1), (0, 2), (1, 1)],
    [(0, 0), (1, -1), (1, 0), (1, 1)],
    [(0, 0), (1, 0), (1, 1), (2, 0)],
    [(0, 0), (1, -1), (1, 0), (2, 0)],
    [(0, 0), (0, 1), (1, -1), (1, 0)],
    [(0, 0), (1, 0), (1, 1), (2, 1)],
    [(0, 0), (0, 1), (1, 1), (1, 2)],
    [(0, 0), (1, -1), (1, 0), (2, -1)],
    [(0, 0), (0, 1), (0, 2), (1, 0)],
    [(0, 0), (0, 1), (0, 2), (1, 2)],
    [(0, 0), (1, 0), (1, 1), (1, 2)],
    [(0, 0), (1, -2), (1, -1), (1, 0)],
    [(0, 0), (1, 0), (2, 0), (2, 1)],
    [(0, 0), (1, 0), (2, -1), (2, 0)],
    [(0, 0), (0, 1), (1, 0), (2, 0)],
    [(0, 0), (0, 1), (1, 1), (2, 1)],
];

/// Cells the pattern may run on past each edge of the picture, so pieces there can be cut off.
const BEYOND: usize = 3;

/// The game screen's cells: true for a box's (copied from the game's picture), false for the
/// pattern's.
pub fn game_boxes(rom: &Rom) -> [[bool; 32]; 30] {
    let mut tiles = [[0xFFu8; 32]; 30];
    // The game screen's upload (`copyToPpu` commands): address, control, data.
    let mut ptr = rom.word(layout::GAME_MODE_STATE_INIT_GAME_BACKGROUND_COPY_GAME_NAMETABLE + 3);
    while rom.byte(ptr) < 0x80 {
        let mut addr = u16::from_be_bytes([rom.byte(ptr), rom.byte(ptr + 1)]) as usize;
        let control = rom.byte(ptr + 2);
        let repeat = control & 0x40 != 0;
        let count = match (control & 0x3F, repeat) {
            (0, false) => 64,
            (0, true) => 256,
            (n, _) => n as usize,
        };
        let mut offset = 2u16 + repeat as u16;
        for _ in 0..count {
            if !repeat {
                offset += 1;
            }
            let at = addr.wrapping_sub(0x2000);
            if at < 960 {
                tiles[at / 32][at % 32] = rom.byte(ptr + offset);
            }
            addr += if control & 0x80 != 0 { 32 } else { 1 };
        }
        ptr += (offset as u8 as u16) + 1;
    }
    tiles.map(|row| row.map(|t| !PATTERN.contains(&t)))
}

/// The picture's pattern: which piece each cell belongs to (none for a box's cell), with the
/// cells beyond its edges.
pub struct Backdrop {
    cols: usize,
    rows: usize,
    piece: Vec<Option<u32>>,
}

impl Backdrop {
    /// Lay whole pieces over every cell of a `cols` by `rows` picture that isn't `boxed`.
    pub fn new(cols: usize, rows: usize, boxed: impl Fn(usize, usize) -> bool) -> Self {
        let (w, h) = (cols + 2 * BEYOND, rows + 2 * BEYOND);
        let free: Vec<bool> = (0..w * h)
            .map(|i| {
                let (r, c) = (i / w, i % w);
                let inside = (BEYOND..BEYOND + rows).contains(&r) && (BEYOND..BEYOND + cols).contains(&c);
                !inside || !boxed(r - BEYOND, c - BEYOND)
            })
            .collect();
        let piece = tile(w, h, &free).expect("the pattern can always be laid");
        Backdrop { cols, rows, piece }
    }

    /// Draw the pattern onto `pixels` (`cols` cells wide), leaving box cells alone.
    pub fn draw(&self, pixels: &mut [u8]) {
        let width = self.cols * 8;
        let w = self.cols + 2 * BEYOND;
        let b = BEYOND as i32;
        let at = |r: i32, c: i32| self.piece[(r + b) as usize * w + (c + b) as usize];
        for r in 0..self.rows as i32 {
            for c in 0..self.cols as i32 {
                let Some(me) = at(r, c) else { continue };
                let same = |dr: i32, dc: i32| at(r + dr, c + dc) == Some(me);
                let (top, left, bottom, right) = (!same(-1, 0), !same(0, -1), !same(1, 0), !same(0, 1));
                let mut cell = [[FILL; 8]; 8];
                // The corners inside a piece's turns.
                if !top && !left && !same(-1, -1) {
                    cell[0][0] = LIGHT;
                }
                if !top && !right && !same(-1, 1) {
                    cell[0][7] = LIGHT;
                }
                if !bottom && !left && !same(1, -1) {
                    cell[7][0] = LIGHT;
                }
                if !bottom && !right && !same(1, 1) {
                    cell[7][7] = DARK;
                }
                if top {
                    cell[0] = [LIGHT; 8];
                }
                if left {
                    for row in cell.iter_mut() {
                        row[0] = LIGHT;
                    }
                }
                if right {
                    for row in cell.iter_mut() {
                        row[7] = DARK;
                    }
                }
                if bottom {
                    cell[7] = [DARK; 8];
                }
                for (y, row) in cell.iter().enumerate() {
                    let start = (r as usize * 8 + y) * width + c as usize * 8;
                    pixels[start..start + 8].copy_from_slice(row);
                }
            }
        }
    }
}

/// Cover every free cell of a `w` by `h` grid with tetrominoes. Each step takes the empty cell
/// with the fewest pieces that could still cover it and tries those in turn, backing up when a
/// cell is left with none. The order of the tries comes from a fixed sequence, so the pattern is
/// the same every time.
fn tile(w: usize, h: usize, free: &[bool]) -> Option<Vec<Option<u32>>> {
    // Every placement of every piece on free cells, and the placements over each cell.
    let mut placements: Vec<[usize; 4]> = vec![];
    let mut covers: Vec<Vec<usize>> = vec![vec![]; w * h];
    for at in 0..w * h {
        let (r, c) = ((at / w) as i32, (at % w) as i32);
        'shape: for shape in &PIECES {
            let mut cells = [0; 4];
            for (k, &(dr, dc)) in shape.iter().enumerate() {
                let (rr, cc) = (r + dr, c + dc);
                if rr < 0 || cc < 0 || rr as usize >= h || cc as usize >= w || !free[rr as usize * w + cc as usize] {
                    continue 'shape;
                }
                cells[k] = rr as usize * w + cc as usize;
            }
            for &i in &cells {
                covers[i].push(placements.len());
            }
            placements.push(cells);
        }
    }
    let mut piece: Vec<Option<u32>> = vec![None; w * h];
    // Per placement, how many of its cells are taken; per cell, how many placements are open.
    let mut taken = vec![0u8; placements.len()];
    let mut open: Vec<usize> = covers.iter().map(Vec::len).collect();
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    let set = |piece: &mut [Option<u32>], taken: &mut [u8], open: &mut [usize], p: usize, id: Option<u32>| {
        for &i in &placements[p] {
            piece[i] = id;
            for &q in &covers[i] {
                if id.is_some() {
                    if taken[q] == 0 {
                        placements[q].iter().for_each(|&j| open[j] -= 1);
                    }
                    taken[q] += 1;
                } else {
                    taken[q] -= 1;
                    if taken[q] == 0 {
                        placements[q].iter().for_each(|&j| open[j] += 1);
                    }
                }
            }
        }
    };
    // Per piece placed: its choices, how far through them, and the one placed.
    let mut stack: Vec<(Vec<usize>, usize, Option<usize>)> = vec![];
    let mut budget = 1_000_000u32;
    loop {
        let empty = (0..w * h).filter(|&i| free[i] && piece[i].is_none());
        let Some(cell) = empty.min_by_key(|&i| open[i]) else { return Some(piece) };
        let mut choices: Vec<usize> = covers[cell].iter().copied().filter(|&p| taken[p] == 0).collect();
        for i in (1..choices.len()).rev() {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            choices.swap(i, (rng % (i as u64 + 1)) as usize);
        }
        stack.push((choices, 0, None));
        loop {
            budget = budget.checked_sub(1)?;
            let id = stack.len() as u32 - 1;
            let (choices, next, placed) = stack.last_mut()?;
            if let Some(p) = placed.take() {
                set(&mut piece, &mut taken, &mut open, p, None);
            }
            if let Some(&p) = choices.get(*next) {
                *next += 1;
                set(&mut piece, &mut taken, &mut open, p, Some(id));
                *placed = Some(p);
                // A dead end if some empty cell can no longer be covered.
                if (0..w * h).any(|i| free[i] && piece[i].is_none() && open[i] == 0) {
                    continue;
                }
                break;
            }
            stack.pop();
            if stack.is_empty() {
                return None;
            }
        }
    }
}
