//! What the app keeps between runs: keys, volume, rules, the last game's menu choices, the high
//! score table and VERSUS's name, server and room settings. The desktop keeps them in
//! `settings.toml`, the web in the browser's storage.
//! The settings screen edits them in the game (`tetris_rs::Config`); `config` and `apply`
//! convert.

use serde::{Deserialize, Serialize};
use tetris_rs::options::{LeftRight, Rules as GameRules};
use tetris_rs::saved::{Choices, HighScores};
use tetris_rs::{Config, Saved};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Output volume, 0.0..=1.0.
    pub volume: f32,
    /// Keep the high score table between launches.
    pub save_high_scores: bool,
    pub fullscreen: bool,
    /// Scale the picture by whole numbers only.
    pub integer_scale: bool,
    /// Show the next piece.
    pub next_piece: bool,
    pub keys: Keys,
    pub rules: Rules,
    pub last_game: LastGame,
    /// Saved once there is a table to save; absent means the game's own default table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high_scores: Option<Table>,
    /// VERSUS: name, server, and the room settings last hosted with. Kept by every build, so one
    /// without VERSUS doesn't drop it when it saves.
    pub versus: Versus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Versus {
    pub name: String,
    /// The room server: the deployed Worker, or `local-room` (`ws://localhost:8788`).
    pub server: String,
    pub room: Room,
}

/// The deployed Worker.
pub const SERVER: &str = "wss://nes-tetris.online";

impl Default for Versus {
    fn default() -> Self {
        Versus { name: "PLAYER".into(), server: SERVER.into(), room: Room::default() }
    }
}

#[cfg(feature = "versus")]
impl Room {
    /// The lobby's view.
    pub fn to_game(&self) -> tetris_rs::versus::RoomSettings {
        tetris_rs::versus::RoomSettings {
            level: self.level.min(19),
            identical: self.pieces == "same",
            das_delay: self.das_delay.clamp(1, 30),
            das_repeat: self.das_repeat.clamp(1, 10),
            left_right: match self.left_right.as_str() {
                "last" => LeftRight::Last,
                "neither" => LeftRight::Neither,
                _ => LeftRight::Both,
            },
            hard_drop: self.hard_drop,
            ghost: self.ghost,
            next_piece: self.next_piece,
            public: self.public,
        }
    }

    pub fn from_game(r: &tetris_rs::versus::RoomSettings) -> Self {
        Room {
            level: r.level,
            pieces: if r.identical { "same" } else { "own" }.into(),
            das_delay: r.das_delay,
            das_repeat: r.das_repeat,
            left_right: match r.left_right {
                LeftRight::Both => "both",
                LeftRight::Last => "last",
                LeftRight::Neither => "neither",
            }
            .into(),
            hard_drop: r.hard_drop,
            ghost: r.ghost,
            next_piece: r.next_piece,
            public: r.public,
        }
    }
}

/// A room's settings, as the host left them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Room {
    pub level: u8,
    /// "own" (each player's own sequence) or "same".
    pub pieces: String,
    pub das_delay: u8,
    pub das_repeat: u8,
    /// "both", "last" or "neither".
    pub left_right: String,
    pub hard_drop: bool,
    pub ghost: bool,
    pub next_piece: bool,
    /// Listed under JOIN and WATCH.
    pub public: bool,
}

impl Default for Room {
    /// Level 0, the same pieces for both, the original rules.
    fn default() -> Self {
        Room {
            level: 0,
            pieces: "same".into(),
            das_delay: 16,
            das_repeat: 6,
            left_right: "both".into(),
            hard_drop: false,
            ghost: false,
            next_piece: true,
            public: true,
        }
    }
}

/// The game keys, by winit key name (`ArrowLeft`, `KeyX`, `Space`...). Menu keys are fixed:
/// arrows, Enter, Escape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keys {
    pub move_left: String,
    pub move_right: String,
    pub soft_drop: String,
    pub hard_drop: String,
    pub rotate_right: String,
    pub rotate_left: String,
    pub pause: String,
}

impl Default for Keys {
    fn default() -> Self {
        Keys::from_game(&Config::default().keys)
    }
}

impl Keys {
    fn from_game(k: &[String; 7]) -> Self {
        let [move_left, move_right, soft_drop, hard_drop, rotate_right, rotate_left, pause] = k.clone();
        Keys { move_left, move_right, soft_drop, hard_drop, rotate_right, rotate_left, pause }
    }

    fn to_game(&self) -> [String; 7] {
        let k = self.clone();
        [k.move_left, k.move_right, k.soft_drop, k.hard_drop, k.rotate_right, k.rotate_left, k.pause]
    }
}

/// Rule changes; the defaults are the original game's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    /// Frames Left/Right is held before the piece repeats (original 16).
    pub das_delay: u8,
    /// Frames between repeats (original 6).
    pub das_repeat: u8,
    /// Left and Right held together: "both" (original), "last" or "neither".
    pub left_right: String,
    /// Down with Left/Right slows the piece (original true).
    pub slowdown: bool,
    pub hard_drop: bool,
    pub ghost: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Rules::from_game(&GameRules::ORIGINAL)
    }
}

impl Rules {
    fn from_game(r: &GameRules) -> Self {
        Rules {
            das_delay: r.das_delay,
            das_repeat: r.das_repeat,
            left_right: match r.left_right {
                LeftRight::Both => "both",
                LeftRight::Last => "last",
                LeftRight::Neither => "neither",
            }
            .into(),
            slowdown: r.slowdown,
            hard_drop: r.hard_drop,
            ghost: r.ghost,
        }
    }

    fn to_game(&self) -> GameRules {
        GameRules {
            das_delay: self.das_delay.clamp(1, 30),
            das_repeat: self.das_repeat.clamp(1, 10),
            left_right: match self.left_right.as_str() {
                "last" => LeftRight::Last,
                "neither" => LeftRight::Neither,
                _ => LeftRight::Both,
            },
            slowdown: self.slowdown,
            hard_drop: self.hard_drop,
            ghost: self.ghost,
        }
    }
}

/// The menu choices the next game starts from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LastGame {
    /// "A" or "B".
    #[serde(rename = "type")]
    pub game_type: String,
    /// "1", "2", "3" or "off".
    pub music: String,
    pub level: u8,
    /// B-type's garbage height.
    pub height: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub a: Vec<Entry>,
    pub b: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub score: u32,
    pub level: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            volume: 1.0,
            save_high_scores: true,
            fullscreen: false,
            integer_scale: false,
            next_piece: true,
            keys: Keys::default(),
            rules: Rules::default(),
            last_game: LastGame::default(),
            high_scores: None,
            versus: Versus::default(),
        }
    }
}

impl Default for LastGame {
    fn default() -> Self {
        LastGame { game_type: "A".into(), music: "1".into(), level: 0, height: 0 }
    }
}

/// Rows per game type in the game's table (it has room for a fourth it never uses).
const ROWS: usize = 3;

impl Settings {
    /// The settings screen's view of these settings.
    pub fn config(&self) -> Config {
        Config {
            keys: self.keys.to_game(),
            volume: (self.volume.clamp(0.0, 1.0) * 10.0).round() as u8,
            save_high_scores: self.save_high_scores,
            fullscreen: self.fullscreen,
            integer_scale: self.integer_scale,
            next_piece: self.next_piece,
            rules: self.rules.to_game(),
        }
    }

    /// Take what the settings screen changed.
    pub fn apply(&mut self, c: &Config) {
        self.keys = Keys::from_game(&c.keys);
        self.volume = c.volume as f32 / 10.0;
        self.save_high_scores = c.save_high_scores;
        self.fullscreen = c.fullscreen;
        self.integer_scale = c.integer_scale;
        self.next_piece = c.next_piece;
        self.rules = Rules::from_game(&c.rules);
    }

    /// What to restore at power-on.
    pub fn saved(&self) -> Saved {
        let high_scores = self.high_scores.as_ref().filter(|_| self.save_high_scores).map(Table::to_game);
        Saved { high_scores, choices: self.last_game.to_game() }
    }
}

impl LastGame {
    pub fn to_game(&self) -> Choices {
        Choices {
            game_type: (self.game_type.eq_ignore_ascii_case("B")) as u8,
            music_type: match self.music.as_str() {
                "2" => 1,
                "3" => 2,
                "off" | "OFF" | "Off" => 3,
                _ => 0,
            },
            // The level menu takes the level mod 10 (A+Start adds 10 when the game starts).
            start_level: self.level.min(19),
            start_height: self.height.min(5),
        }
    }

    pub fn from_game(c: &Choices) -> Self {
        LastGame {
            game_type: if c.game_type == 0 { "A" } else { "B" }.into(),
            music: match c.music_type {
                0 => "1",
                1 => "2",
                2 => "3",
                _ => "off",
            }
            .into(),
            level: c.start_level,
            height: c.start_height,
        }
    }
}

impl Table {
    pub fn from_game(h: &HighScores) -> Self {
        let rows = |first: usize| {
            (first..first + ROWS)
                .map(|r| {
                    let (name, score, level) = h.row(r);
                    Entry { name, score, level }
                })
                .collect()
        };
        Table { a: rows(0), b: rows(4) }
    }

    fn to_game(&self) -> HighScores {
        let mut h = HighScores::empty();
        for (first, entries) in [(0, &self.a), (4, &self.b)] {
            for (r, e) in entries.iter().take(ROWS).enumerate() {
                h.set_row(first + r, &e.name, e.score, e.level);
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let mut s = Settings::default();
        s.keys.rotate_right = "KeyK".into();
        s.rules.hard_drop = true;
        s.rules.left_right = "last".into();
        s.last_game = LastGame { game_type: "B".into(), music: "off".into(), level: 12, height: 4 };
        let mut h = HighScores::empty();
        h.set_row(0, "SAM", 999_999, 29);
        h.set_row(5, "(A.B)", 42, 3);
        s.high_scores = Some(Table::from_game(&h));
        let text = toml::to_string_pretty(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back, s);
        let saved = back.saved();
        assert_eq!(saved.high_scores.unwrap(), h);
        assert_eq!(LastGame::from_game(&saved.choices), s.last_game);
        let mut again = Settings::default();
        again.apply(&s.config());
        assert_eq!((again.keys, again.rules), (s.keys.clone(), s.rules.clone()));
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = toml::from_str("volume = 0.5\n[keys]\npause = \"KeyP\"\n").unwrap();
        assert_eq!(s.volume, 0.5);
        assert_eq!(s.keys.pause, "KeyP");
        assert_eq!(s.keys.rotate_right, "KeyX");
        assert!(s.save_high_scores);
        assert!(s.saved().high_scores.is_none());
    }

    #[test]
    #[ignore = "prints an example file"]
    fn example() {
        let mut s = Settings::default();
        let mut h = HighScores::empty();
        h.set_row(0, "HOWARD", 10_000, 9);
        s.high_scores = Some(Table::from_game(&h));
        println!("{}", toml::to_string_pretty(&s).unwrap());
    }
}
