//! Scripted inputs for oracle runs, shared by the recompile's tests and the reimplementation's.
//!
//! A script sees the frame number and the RAM at the previous NMI and returns the buttons to hold
//! for the next frame plus any RAM pokes to apply first (pokes skip ahead: a level, a playfield).
//! [`Coverage`] records what a run actually exercised, so a passing run can't be one that sat in
//! a menu.

use crate::machine::*;

/// RAM addresses (from disasm/tetris-ram.asm) the scripts steer by.
pub mod ram {
    pub const GAME_MODE: usize = 0x00C0;
    pub const LEVEL_NUMBER: usize = 0x0044;
    pub const CURRENT_PIECE: usize = 0x0042;
    pub const PLAYER1_TETRIMINO_Y: usize = 0x0061;
    pub const PLAYER1_CURRENT_PIECE: usize = 0x0062;
    pub const PLAYER1_LEVEL_NUMBER: usize = 0x0064;
    pub const PLAYER1_PLAY_STATE: usize = 0x0068;
    pub const PLAYER1_LINES: usize = 0x0070;
    pub const PLAYER1_SCORE: usize = 0x0073;
    pub const COMPLETED_LINES: usize = 0x0056;
    pub const PLAYFIELD: usize = 0x0400;
    pub const START_LEVEL: usize = 0x0047;
    pub const START_HEIGHT: usize = 0x0059;
    pub const PLAYER1_START_LEVEL: usize = 0x0067;
    pub const PLAYER1_START_HEIGHT: usize = 0x0079;
    pub const RENDER_MODE: usize = 0x00BD;
    pub const GAME_TYPE: usize = 0x00C1;
}

/// `renderMode` values.
pub const RENDER_CONGRATULATIONS: u8 = 2;
pub const RENDER_ENDING: u8 = 4;

/// `gameMode` values.
pub const MODE_PLAY: u8 = 4;
pub const MODE_DEMO: u8 = 5;
/// A filled playfield cell, and an empty one.
pub const BLOCK_TILE: u8 = 0x7B;
pub const EMPTY_TILE: u8 = 0xEF;
/// Orientation id of a vertical I piece.
pub const I_VERTICAL: u8 = 0x11;

pub type Script = Box<dyn FnMut(usize, &[u8; 0x800]) -> (u8, Vec<(u16, u8)>) + Send>;

#[derive(Debug, Default, Clone)]
pub struct Coverage {
    pub play_frames: usize,
    pub demo_frames: usize,
    pub games: usize,
    pub line_clear_frames: usize,
    pub max_lines: u32,
    pub max_score: u32,
    pub level_29_frames: usize,
    /// Frames of the high-score name entry and of the ending animations.
    pub congratulations_frames: usize,
    pub ending_frames: usize,
}

impl Coverage {
    /// Account one frame, given RAM at the previous and current NMI.
    pub fn observe(&mut self, prev: &[u8; 0x800], now: &[u8; 0x800]) {
        match now[ram::GAME_MODE] {
            MODE_PLAY => {
                self.play_frames += 1;
                if prev[ram::GAME_MODE] != MODE_PLAY {
                    self.games += 1;
                }
                if now[ram::COMPLETED_LINES] > 0 {
                    self.line_clear_frames += 1;
                }
                self.max_lines = self.max_lines.max(bcd(&now[ram::PLAYER1_LINES..][..2]));
                self.max_score = self.max_score.max(bcd(&now[ram::PLAYER1_SCORE..][..3]));
                if now[ram::PLAYER1_LEVEL_NUMBER] == 29 {
                    self.level_29_frames += 1;
                }
            }
            MODE_DEMO => self.demo_frames += 1,
            _ => {}
        }
        match now[ram::RENDER_MODE] {
            RENDER_CONGRATULATIONS => self.congratulations_frames += 1,
            RENDER_ENDING => self.ending_frames += 1,
            _ => {}
        }
    }
}

/// Little-endian BCD, two digits per byte.
pub fn bcd(bytes: &[u8]) -> u32 {
    bytes.iter().rev().fold(0, |acc, &b| acc * 100 + (b >> 4) as u32 * 10 + (b & 15) as u32)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// No input: legal screen, title, then the demo game, repeatedly.
pub fn attract() -> Script {
    Box::new(|_, _| (0, vec![]))
}

/// Menus: tap Start every 20 frames, sometimes moving the cursor first.
fn menu_buttons(f: usize, rng: &mut Rng) -> u8 {
    match f % 20 {
        0 => BTN_START,
        10 => [BTN_RIGHT, BTN_DOWN, BTN_A, 0][(rng.next() % 4) as usize],
        _ => 0,
    }
}

fn on_game_over_screen(ram: &[u8; 0x800]) -> bool {
    matches!(ram[ram::PLAYER1_PLAY_STATE], 0 | 10)
}

/// Random play: random movement and rotation, held down often so pieces drop fast. Each new game
/// gets its bottom 8 rows pre-filled except for one column, so random drops complete lines
/// sometimes; Start is tapped on the game-over screens.
pub fn random_play(seed: u64) -> Script {
    let mut rng = Rng(seed);
    let mut held = 0u8;
    let mut in_game = false;
    Box::new(move |f, ram| {
        if ram[ram::GAME_MODE] != MODE_PLAY {
            in_game = false;
            return (menu_buttons(f, &mut rng), vec![]);
        }
        let mut pokes = vec![];
        if !in_game && ram[ram::PLAYER1_PLAY_STATE] == 1 {
            in_game = true;
            let hole = (rng.next() % 10) as usize;
            for row in 12..20 {
                for col in (0..10).filter(|&c| c != hole) {
                    pokes.push(((ram::PLAYFIELD + row * 10 + col) as u16, BLOCK_TILE));
                }
            }
        }
        if on_game_over_screen(ram) {
            return (if f % 20 == 0 { BTN_START } else { 0 }, pokes);
        }
        if f % 6 == 0 {
            let r = rng.next();
            held = 0;
            if r & 1 != 0 {
                held |= BTN_DOWN;
            }
            if r & 6 == 2 {
                held |= BTN_LEFT;
            }
            if r & 6 == 4 {
                held |= BTN_RIGHT;
            }
            if r & 8 != 0 {
                held |= BTN_A;
            }
            if r & 0x30 == 0x10 {
                held |= BTN_B;
            }
        }
        (held, pokes)
    })
}

/// Random play with the level held at 29 once each game is under way.
pub fn level_29(seed: u64) -> Script {
    let mut play = random_play(seed);
    Box::new(move |f, ram| {
        let (b, mut pokes) = play(f, ram);
        if ram[ram::GAME_MODE] == MODE_PLAY
            && ram[ram::PLAYER1_PLAY_STATE] == 1
            && ram[ram::PLAYER1_LEVEL_NUMBER] != 29
        {
            pokes.push((ram::LEVEL_NUMBER as u16, 29));
            pokes.push((ram::PLAYER1_LEVEL_NUMBER as u16, 29));
        }
        (b, pokes)
    })
}

/// Every piece becomes a vertical I over a one-column well at its spawn column, held down: a
/// Tetris per piece, so levels, scoring, the line-clear animation and level 29+ all run. Games
/// start at level 18, as high-level play does; it reaches 29 after 240 lines.
pub fn line_clears(seed: u64) -> Script {
    const WELL_COLUMN: usize = 5;
    let mut rng = Rng(seed);
    let mut last_y = 0xFF;
    Box::new(move |f, ram| {
        if ram[ram::GAME_MODE] != MODE_PLAY || on_game_over_screen(ram) {
            last_y = 0xFF;
            let b = if ram[ram::GAME_MODE] == MODE_PLAY {
                if f % 20 == 0 { BTN_START } else { 0 }
            } else {
                menu_buttons(f, &mut rng)
            };
            return (b, vec![]);
        }
        let y = ram[ram::PLAYER1_TETRIMINO_Y];
        let mut pokes = vec![];
        if last_y == 0xFF {
            pokes.push((ram::LEVEL_NUMBER as u16, 18));
            pokes.push((ram::PLAYER1_LEVEL_NUMBER as u16, 18));
        }
        if ram[ram::PLAYER1_PLAY_STATE] == 1 && y < last_y {
            // A piece just spawned.
            pokes.push((ram::CURRENT_PIECE as u16, I_VERTICAL));
            pokes.push((ram::PLAYER1_CURRENT_PIECE as u16, I_VERTICAL));
            for row in 16..20 {
                for col in 0..10 {
                    let tile = if col == WELL_COLUMN { EMPTY_TILE } else { BLOCK_TILE };
                    pokes.push(((ram::PLAYFIELD + row * 10 + col) as u16, tile));
                }
            }
        }
        last_y = if ram[ram::PLAYER1_PLAY_STATE] == 1 { y } else { last_y.min(0xFE) };
        (BTN_DOWN, pokes)
    })
}

/// Games played to their endings, cycling through every kind: A-type games scored past each
/// rocket threshold (30k, 50k, 70k, 100k, 120k) and then topped out on purpose, and B-type games
/// cleared down to 0 lines: at level 9 (the concert) for heights 0..=5, and at levels 3 and 7
/// (the cathedral). Pieces become vertical I pieces over a cleared
/// playfield with a one-column well, so each drop is a Tetris; Start is pressed sparingly during
/// endings so the animations play out.
pub fn endings(seed: u64) -> Script {
    const WELL_COLUMN: usize = 5;
    /// (B-type?, A-type target for `score[2]`, B-type height, start level)
    const GAMES: [(bool, u8, u8, u8); 13] = [
        (false, 0x03, 0, 9),
        (true, 0, 0, 9),
        (false, 0x05, 0, 9),
        (true, 0, 1, 9),
        (false, 0x07, 0, 9),
        (true, 0, 2, 3),
        (false, 0x10, 0, 9),
        (true, 0, 3, 9),
        (false, 0x12, 0, 9),
        (true, 0, 4, 9),
        (true, 0, 5, 9),
        (true, 0, 2, 9),
        (true, 0, 1, 7),
    ];
    let mut rng = Rng(seed);
    let mut game = 0usize;
    let mut last_y = 0xFF;
    let mut since_ending = 0usize;
    Box::new(move |f, ram| {
        let (b_type, target, height, level) = GAMES[game % GAMES.len()];
        let mode = ram[ram::GAME_MODE];
        if mode == 2 || mode == 3 {
            last_y = 0xFF;
            let pokes = vec![
                (ram::GAME_TYPE as u16, b_type as u8),
                (ram::START_LEVEL as u16, level),
                (ram::PLAYER1_START_LEVEL as u16, level),
                (ram::START_HEIGHT as u16, height),
                (ram::PLAYER1_START_HEIGHT as u16, height),
            ];
            return (if f % 20 == 0 { BTN_START } else { 0 }, pokes);
        }
        if mode != MODE_PLAY {
            return (menu_buttons(f, &mut rng), vec![]);
        }
        if ram[ram::RENDER_MODE] == RENDER_ENDING {
            since_ending += 1;
            return (if since_ending % 300 == 299 { BTN_START } else { 0 }, vec![]);
        }
        since_ending = 0;
        if on_game_over_screen(ram) || ram[ram::RENDER_MODE] == RENDER_CONGRATULATIONS {
            if last_y != 0xFF {
                game += 1;
                last_y = 0xFF;
            }
            return (if f % 20 == 0 { BTN_START } else { 0 }, vec![]);
        }
        let y = ram[ram::PLAYER1_TETRIMINO_Y];
        let scoring = b_type || ram[ram::PLAYER1_SCORE + 2] < target;
        let mut pokes = vec![];
        if scoring && ram[ram::PLAYER1_PLAY_STATE] == 1 && y < last_y {
            pokes.push((ram::CURRENT_PIECE as u16, I_VERTICAL));
            pokes.push((ram::PLAYER1_CURRENT_PIECE as u16, I_VERTICAL));
            for row in 0..20 {
                for col in 0..10 {
                    let tile = if row < 16 || col == WELL_COLUMN { EMPTY_TILE } else { BLOCK_TILE };
                    pokes.push(((ram::PLAYFIELD + row * 10 + col) as u16, tile));
                }
            }
        }
        if ram[ram::PLAYER1_PLAY_STATE] == 1 {
            last_y = y;
        } else if last_y == 0xFF {
            last_y = 0xFE;
        }
        (BTN_DOWN, pokes)
    })
}
