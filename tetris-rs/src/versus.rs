//! What the app and the VERSUS screens (`pc_versus`) exchange each frame, and text drawn straight
//! onto a picture in the game's font (the match view's countdown and result).
//!
//! The screens are drawn by the game like its other menus, but the game knows nothing of
//! networks: the app hands it a `VersusView` (where the connection is, who's in the room, the
//! room's settings, a status line) and takes back the `VersusRequest`s the player made.

use crate::options::LeftRight;
use crate::saved::NAME_CHARS;
use crate::{layout, Rom};

/// The letters of room codes (no I or O): the same as `protocol::CODE_LETTERS`.
pub const CODE_LETTERS: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ";
pub const CODE_LEN: usize = 4;
/// Longest player name (as `protocol::NAME_LEN`).
pub const NAME_LEN: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Connection {
    /// Not in a room.
    #[default]
    None,
    Connecting,
    InRoom,
    /// The connection failed or dropped; the text says why.
    Lost(String),
}

/// What the lobby shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VersusView {
    /// The player's name, as saved.
    pub name: String,
    pub connection: Connection,
    pub code: String,
    /// This player's seat (0 hosts); none when watching.
    pub seat: Option<u8>,
    /// Each seat's player: name and whether ready.
    pub players: [Option<(String, bool)>; 2],
    pub watching: u32,
    pub settings: RoomSettings,
    /// A line about what's happening (waiting for a player, ready for a rematch...).
    pub status: String,
    /// The players' matches here so far: the score between them ("ANA 1 - 0 BO"), or who of
    /// them left. One or two lines, or none.
    pub series: Vec<String>,
}

/// What the player asked for this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersusRequest {
    Host,
    Join(String),
    Watch(String),
    Ready(bool),
    /// The host changed the room's settings.
    Settings(RoomSettings),
    Leave,
    SetName(String),
}

/// A room's settings (as `protocol::Settings`): the host's choice, A-type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoomSettings {
    /// 0..=19.
    pub level: u8,
    /// The same pieces for both players, or each their own.
    pub identical: bool,
    pub das_delay: u8,
    pub das_repeat: u8,
    pub left_right: LeftRight,
    pub hard_drop: bool,
    pub ghost: bool,
    pub next_piece: bool,
}

impl Default for RoomSettings {
    /// Level 18, the same pieces for both, the original rules.
    fn default() -> Self {
        RoomSettings {
            level: 18,
            identical: true,
            das_delay: 16,
            das_repeat: 6,
            left_right: LeftRight::Both,
            hard_drop: false,
            ghost: false,
            next_piece: true,
        }
    }
}

/// The NES's white, and black.
pub const WHITE: u8 = 0x30;
pub const BLACK: u8 = 0x0F;

/// Draw `text` in the game's menu font onto `pixels` (`width` wide), top-left at (`x`, `y`), on a
/// black box a pixel wider all round. Characters the font lacks are blank.
pub fn draw_text(rom: &Rom, pixels: &mut [u8], width: usize, x: usize, y: usize, text: &str, colour: u8) {
    let height = pixels.len() / width;
    let mut put = |px: usize, py: usize, c: u8| {
        if px < width && py < height {
            pixels[py * width + px] = c;
        }
    };
    let w = 8 * text.chars().count();
    for py in y.saturating_sub(1)..y + 9 {
        for px in x.saturating_sub(1)..x + w + 1 {
            put(px, py, BLACK);
        }
    }
    draw_glyphs(rom, pixels, width, x, y, text, colour);
}

/// As `draw_text`, without the box: the letters alone.
pub fn draw_glyphs(rom: &Rom, pixels: &mut [u8], width: usize, x: usize, y: usize, text: &str, colour: u8) {
    let height = pixels.len() / width;
    let mut put = |px: usize, py: usize, c: u8| {
        if px < width && py < height {
            pixels[py * width + px] = c;
        }
    };
    for (i, ch) in text.chars().enumerate() {
        let Some(index) = NAME_CHARS.find(ch).filter(|_| ch != ' ') else { continue };
        // The menus' tiles are the first 4 KB of CHR (bank 0, the background table).
        let tile = rom.table(layout::HIGH_SCORE_CHAR_TO_TILE, index) as usize;
        for row in 0..8 {
            let (lo, hi) = (rom.chr[tile * 16 + row], rom.chr[tile * 16 + row + 8]);
            for col in 0..8 {
                if (lo >> (7 - col)) & 1 != 0 || (hi >> (7 - col)) & 1 != 0 {
                    put(x + 8 * i + col, y + row, colour);
                }
            }
        }
    }
}
