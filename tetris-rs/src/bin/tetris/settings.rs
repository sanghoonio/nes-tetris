//! `settings.toml`: keys, volume, the last game's menu choices and the high score table, kept in
//! `~/Library/Application Support/Tetris/` so they outlast the app (and rebuilds of it).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tetris_rs::saved::{Choices, HighScores};
use tetris_rs::Saved;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Output volume, 0.0..=1.0.
    pub volume: f32,
    /// Keep the high score table between launches.
    pub save_high_scores: bool,
    pub keys: frontend::Bindings,
    pub last_game: LastGame,
    /// Saved once there is a table to save; absent means the game's own default table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high_scores: Option<Table>,
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
            keys: frontend::Bindings::default(),
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

    /// What to restore at power-on.
    pub fn saved(&self) -> Saved {
        let high_scores = self.high_scores.as_ref().filter(|_| self.save_high_scores).map(Table::to_game);
        Saved { high_scores, choices: self.last_game.to_game() }
    }
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
