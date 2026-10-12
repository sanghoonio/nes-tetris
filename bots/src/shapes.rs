//! The pieces as the game has them, from the ROM: each orientation's four cells
//! (`orientationTable`), what A and B turn it into (`rotationTable`), and where each piece
//! appears (`spawnTable`; pieces appear at column 5, row 0).

use tetris_rs::{layout, Rom};

/// Orientations (0x00..=0x12); 0x13 is "no piece".
pub const ORIENTATIONS: usize = 19;

pub struct Shapes {
    /// Each orientation's cells, (dy, dx) from the piece's position.
    pub cells: [[(i8, i8); 4]; ORIENTATIONS],
    /// What B (anticlockwise) and A (clockwise) turn each orientation into.
    pub ccw: [u8; ORIENTATIONS],
    pub cw: [u8; ORIENTATIONS],
    /// The orientation each piece type (0..7) appears in.
    pub spawn: [u8; 7],
    /// Each orientation's piece type.
    pub kind: [u8; ORIENTATIONS],
}

impl Shapes {
    pub fn from_rom(rom: &Rom) -> Shapes {
        let cells = std::array::from_fn(|o| {
            let bytes = rom.bytes(layout::ORIENTATION_TABLE + o as u16 * 12, 12);
            std::array::from_fn(|k| (bytes[k * 3] as i8, bytes[k * 3 + 2] as i8))
        });
        let ccw = std::array::from_fn(|o| rom.byte(layout::ROTATION_TABLE + o as u16 * 2));
        let cw = std::array::from_fn(|o| rom.byte(layout::ROTATION_TABLE + o as u16 * 2 + 1));
        let spawn = std::array::from_fn(|t| rom.byte(layout::SPAWN_TABLE + t as u16));
        let kind = std::array::from_fn(|o| rom.byte(layout::TETRIMINO_TYPE_FROM_ORIENTATION + o as u16));
        Shapes { cells, ccw, cw, spawn, kind }
    }

    /// Where a piece given as an orientation (the game's `nextPiece`) appears: its spawn
    /// orientation, at column 5, row 0.
    pub fn spawn(&self, orientation: u8) -> (u8, u8, u8) {
        let kind = self.kind[orientation as usize % ORIENTATIONS] as usize % 7;
        (self.spawn[kind], 5, 0)
    }

    /// Whether to turn `from` clockwise (A) to reach `to` in fewest presses.
    pub fn turns(&self, from: u8, to: u8) -> bool {
        let steps = |table: &[u8; ORIENTATIONS]| {
            let mut o = from;
            for n in 1..=4 {
                o = table[o as usize];
                if o == to {
                    return n;
                }
            }
            99
        };
        steps(&self.cw) <= steps(&self.ccw)
    }
}
