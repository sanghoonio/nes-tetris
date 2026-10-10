//! `settings.toml`: keys, volume, rules, the last game's menu choices and the high score table,
//! kept in `~/Library/Application Support/Tetris/` so they outlast the app (and rebuilds of it).
//! The settings screen edits them in the game (`tetris_rs::Config`); `config` and `apply`
//! convert.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use frontend::KeyCode;
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
    pub keys: frontend::Bindings,
    pub rules: Rules,
    pub last_game: LastGame,
    /// Saved once there is a table to save; absent means the game's own default table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high_scores: Option<Table>,
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
            keys: frontend::Bindings::default(),
            rules: Rules::default(),
            last_game: LastGame::default(),
            high_scores: None,
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
    pub fn dir() -> PathBuf {
        let home = std::env::var_os("HOME").expect("HOME");
        PathBuf::from(home).join("Library/Application Support/Tetris")
    }

    fn path() -> PathBuf {
        Self::dir().join("settings.toml")
    }

    /// Read the file, or the defaults if there isn't one. A file that doesn't parse is kept
    /// as `settings.invalid.toml` (so it isn't overwritten) and the defaults are used.
    pub fn load() -> Self {
        let path = Self::path();
        let Ok(text) = std::fs::read_to_string(&path) else { return Self::default() };
        match toml::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                let keep = Self::dir().join("settings.invalid.toml");
                eprintln!("{}: {e}\nkept as {}; using defaults", path.display(), keep.display());
                let _ = std::fs::rename(&path, keep);
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(Self::dir())?;
            let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
            // Write then rename, so a crash mid-write can't leave half a file.
            let tmp = Self::dir().join("settings.toml.tmp");
            std::fs::write(&tmp, text)?;
            std::fs::rename(tmp, Self::path())
        };
        if let Err(e) = write() {
            eprintln!("saving {}: {e}", Self::path().display());
        }
    }

    /// The settings screen's view of these settings.
    pub fn config(&self) -> Config {
        let k = &self.keys;
        let keys = [k.up, k.down, k.left, k.right, k.a, k.b, k.start, k.select, k.back].map(key_name);
        Config {
            keys,
            volume: (self.volume.clamp(0.0, 1.0) * 10.0).round() as u8,
            save_high_scores: self.save_high_scores,
            fullscreen: self.fullscreen,
            integer_scale: self.integer_scale,
            rules: self.rules.to_game(),
        }
    }

    /// Take what the settings screen changed.
    pub fn apply(&mut self, c: &Config) {
        let key = |i: usize, old: KeyCode| key_code(&c.keys[i]).unwrap_or(old);
        let k = self.keys.clone();
        self.keys = frontend::Bindings {
            up: key(0, k.up),
            down: key(1, k.down),
            left: key(2, k.left),
            right: key(3, k.right),
            a: key(4, k.a),
            b: key(5, k.b),
            start: key(6, k.start),
            select: key(7, k.select),
            back: key(8, k.back),
        };
        self.volume = c.volume as f32 / 10.0;
        self.save_high_scores = c.save_high_scores;
        self.fullscreen = c.fullscreen;
        self.integer_scale = c.integer_scale;
        self.rules = Rules::from_game(&c.rules);
    }

    /// The window's view of these settings.
    pub fn frontend(&self) -> frontend::Options {
        frontend::Options {
            bindings: self.keys.clone(),
            volume: self.volume,
            fullscreen: self.fullscreen,
            integer_scale: self.integer_scale,
        }
    }

    /// What to restore at power-on.
    pub fn saved(&self) -> Saved {
        let high_scores = self.high_scores.as_ref().filter(|_| self.save_high_scores).map(Table::to_game);
        Saved { high_scores, choices: self.last_game.to_game() }
    }
}

/// A key's name as the settings file writes it (`KeyX`, `ArrowUp`...).
pub fn key_name(code: KeyCode) -> String {
    match toml::Value::try_from(code) {
        Ok(toml::Value::String(s)) => s,
        _ => format!("{code:?}"),
    }
}

pub fn key_code(name: &str) -> Option<KeyCode> {
    toml::Value::String(name.to_string()).try_into().ok()
}

impl LastGame {
    fn to_game(&self) -> Choices {
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
        s.keys.a = frontend::KeyCode::KeyK;
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
        assert_eq!(key_name(KeyCode::ShiftRight), "ShiftRight");
        assert_eq!(key_code("Digit7"), Some(KeyCode::Digit7));
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = toml::from_str("volume = 0.5\n[keys]\nstart = \"Space\"\n").unwrap();
        assert_eq!(s.volume, 0.5);
        assert_eq!(s.keys.start, frontend::KeyCode::Space);
        assert_eq!(s.keys.a, frontend::KeyCode::KeyX);
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
