//! Online matches: the messages a client and a room exchange (JSON over a websocket), and the
//! room itself as a state machine (`room`), with no networking, so that it runs the same in the
//! Cloudflare Worker and in tests.
//!
//! A room seats two players, the host (seat 0, who chooses the settings) and an opponent (seat 1);
//! everyone else watches. Each player's game runs on their own machine. Every 15 frames a player
//! sends the buttons of those frames and their score; the room passes them on, and every other
//! client replays them to show that game. The match is decided CTWC style: when one player tops
//! out, the other plays on until passing the score or topping out too.

use serde::{Deserialize, Serialize};

pub mod room;

pub use room::{Event, Room, Send};

/// A connection, numbered by whoever runs the room.
pub type ConnId = u64;

/// Seat 0 is the host, seat 1 the opponent.
pub type Seat = u8;

/// Frames per input batch: 250 ms.
pub const BATCH_FRAMES: usize = 15;
/// Frames between game checksums.
pub const CHECKSUM_FRAMES: u32 = 60;
/// The countdown before a match starts.
pub const COUNTDOWN_MS: u64 = 3_000;
/// Longest name, in characters the game's font can show (`A`-`Z`, `0`-`9`, space, `-`, `.`).
pub const NAME_LEN: usize = 8;

/// Client to room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToRoom {
    /// The first message on a connection. Players take the first free seat (while no match is on);
    /// `watch`, or a full room, makes a spectator.
    Hello { name: String, watch: bool },
    /// The host's choice, between matches.
    Settings { settings: Settings },
    Ready { ready: bool },
    /// The buttons of this player's frames `frame..frame + buttons.len()` (NES bits, A = $80), the
    /// score after the last of them, and whether the game has ended (topped out).
    Inputs { frame: u32, buttons: Vec<u8>, score: u32, over: bool },
    /// A checksum of this player's game at `frame`, for the others' replicas to compare.
    Checksum { frame: u32, hash: u64 },
    /// After a result: the score of each seat's game at the frame the result names, as this
    /// client's games and replicas have it.
    Report { match_id: u32, scores: [u32; 2] },
}

/// Room to client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FromRoom {
    /// The answer to `Hello`: this connection's seat, or none for a spectator.
    Welcome { seat: Option<Seat> },
    /// Who's here and what's set, whenever it changes.
    Lobby { players: [Option<Player>; 2], spectators: u32, settings: Settings, playing: bool },
    /// A match begins `countdown_ms` from now: each seat's game is a direct start of the settings
    /// with its seed (the same seed for both with identical pieces). Spectators arriving during a
    /// match get it with a countdown of 0, then every batch so far.
    Start { match_id: u32, settings: Settings, seeds: [u32; 2], countdown_ms: u64 },
    /// A player's buttons, passed on (see `ToRoom::Inputs`).
    Inputs { seat: Seat, frame: u32, buttons: Vec<u8>, score: u32, over: bool },
    Checksum { seat: Seat, frame: u32, hash: u64 },
    /// A player's connection dropped during a match. Before topping out that's a forfeit (a
    /// `Result` follows); after, their board stops where it is and the match goes on.
    Gone { seat: Seat },
    /// The match is decided. Each seat's game counts up to `frames[seat]` frames, where its score
    /// was `scores[seat]`. Clients stop the games, then report the scores they have at those frames.
    Result { match_id: u32, outcome: Outcome, scores: [u32; 2], frames: [u32; 2] },
    /// Whether the players' reports agreed with the scores the result was decided on.
    Confirmed { match_id: u32, agreed: bool },
    /// Both players left during a match: no result.
    Abandoned { match_id: u32 },
    Error { message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Win { seat: Seat },
    Draw,
    /// `seat` left during the match before topping out.
    Forfeit { seat: Seat },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub ready: bool,
}

/// What the host chooses. Matches are A-type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// 0..=19.
    pub level: u8,
    pub pieces: Pieces,
    pub rules: Rules,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { level: 18, pieces: Pieces::Separate, rules: Rules::default() }
    }
}

impl Settings {
    pub fn valid(&self) -> bool {
        self.level <= 19 && (1..=30).contains(&self.rules.das_delay) && (1..=30).contains(&self.rules.das_repeat)
    }
}

/// How the players' pieces are dealt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pieces {
    /// Each player's own sequence, as on two consoles.
    Separate,
    /// The same sequence for both, whatever their timing.
    Identical,
}

/// The game's rule options (`tetris_rs::options::Rules`), plus the next-piece preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    pub das_delay: u8,
    pub das_repeat: u8,
    pub left_right: LeftRight,
    pub hard_drop: bool,
    pub ghost: bool,
    pub next_piece: bool,
}

impl Default for Rules {
    /// The original game's.
    fn default() -> Self {
        Rules { das_delay: 16, das_repeat: 6, left_right: LeftRight::Both, hard_drop: false, ghost: false, next_piece: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeftRight {
    Both,
    Last,
    Neither,
}

/// Room codes: four letters, without I and O.
pub const CODE_LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
pub const CODE_LEN: usize = 4;

/// A room code from random bits.
pub fn room_code(mut random: u64) -> String {
    (0..CODE_LEN)
        .map(|_| {
            let c = CODE_LETTERS[(random % CODE_LETTERS.len() as u64) as usize] as char;
            random /= CODE_LETTERS.len() as u64;
            c
        })
        .collect()
}

/// Whether `code` (upper case) could be a room code.
pub fn valid_code(code: &str) -> bool {
    code.len() == CODE_LEN && code.bytes().all(|b| CODE_LETTERS.contains(&b))
}

/// A name as the game's font can show it: upper case, letters, digits, space, `-` and `.`, at
/// most `NAME_LEN` long; empty becomes `PLAYER`.
pub fn clean_name(name: &str) -> String {
    let kept: String =
        name.to_uppercase().chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || " -.".contains(*c)).collect();
    let words: Vec<&str> = kept.split_whitespace().collect();
    let name: String = words.join(" ").chars().take(NAME_LEN).collect();
    let name = name.trim_end().to_string();
    if name.is_empty() { "PLAYER".to_string() } else { name }
}

impl ToRoom {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("messages serialise")
    }

    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}

impl FromRoom {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("messages serialise")
    }

    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}
