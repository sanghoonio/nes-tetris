//! What the app keeps between launches: the high score table and the last game's menu choices.
//!
//! They're the game's own variables. `Saved::from_ram` reads them from a frame's RAM, and
//! `initRam` puts them back at power-on (`Game::saved`), the way a warm boot keeps the high
//! scores, so none of the game's routines change.

use crate::Vars;

/// The name-entry alphabet, in the order of `highScoreCharToTile` (0 is the dash a new entry
/// starts with; 43 is a space).
pub const NAME_CHARS: &str = "-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789,/()\". ";

/// The high score table as the game stores it ($0700..$074F). Each game type has 3 rows plus
/// an unused fourth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighScores {
    /// 8 names of 6 letters (indices into `NAME_CHARS`): A-type's 4, then B-type's 4.
    pub names: [u8; 48],
    /// 8 three-byte BCD scores, most significant byte first.
    pub scores: [u8; 24],
    pub levels: [u8; 8],
}

/// The menu choices the next game starts from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    /// 0 = A-type, 1 = B-type.
    pub game_type: u8,
    /// 0..=2 = music 1..3, 3 = off.
    pub music_type: u8,
    pub start_level: u8,
    pub start_height: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub high_scores: Option<HighScores>,
    pub choices: Choices,
}

impl HighScores {
    pub fn from_vars(v: &Vars) -> Self {
        let mut scores = [0; 24];
        scores[..12].copy_from_slice(&v.high_score_scores_a);
        scores[12..].copy_from_slice(&v.high_score_scores_b);
        HighScores { names: v.high_score_names, scores, levels: v.high_score_levels }
    }

    fn store(&self, v: &mut Vars) {
        v.high_score_names = self.names;
        v.high_score_scores_a.copy_from_slice(&self.scores[..12]);
        v.high_score_scores_b.copy_from_slice(&self.scores[12..]);
        v.high_score_levels = self.levels;
    }

    /// Row `row` (0..8) as (name, score, level). Letters outside the alphabet show as '?'.
    pub fn row(&self, row: usize) -> (String, u32, u8) {
        let name = self.names[row * 6..][..6]
            .iter()
            .map(|&c| NAME_CHARS.chars().nth(c as usize).unwrap_or('?'))
            .collect();
        let bcd = &self.scores[row * 3..][..3];
        let score = bcd.iter().fold(0, |n, &b| n * 100 + (b >> 4) as u32 * 10 + (b & 0x0F) as u32);
        (name, score, self.levels[row])
    }

    /// Set row `row` from (name, score, level); the name is padded or cut to 6 letters, and
    /// characters outside the alphabet become spaces. The score is capped at 999999.
    pub fn set_row(&mut self, row: usize, name: &str, score: u32, level: u8) {
        let mut letters = name.chars().map(|ch| NAME_CHARS.find(ch.to_ascii_uppercase()).unwrap_or(43) as u8);
        for c in &mut self.names[row * 6..][..6] {
            *c = letters.next().unwrap_or(43);
        }
        let mut n = score.min(999_999);
        for b in self.scores[row * 3..][..3].iter_mut().rev() {
            *b = ((n / 10 % 10) << 4 | n % 10) as u8;
            n /= 100;
        }
        self.levels[row] = level;
    }

    /// An all-blank table (every row "------", 0, level 0).
    pub fn empty() -> Self {
        HighScores { names: [0; 48], scores: [0; 24], levels: [0; 8] }
    }
}

impl Saved {
    /// What a frame's RAM would save.
    pub fn from_ram(ram: &[u8; 0x800]) -> Self {
        let v = Vars::from_ram(ram);
        Saved {
            high_scores: Some(HighScores::from_vars(&v)),
            choices: Choices {
                game_type: v.game_type,
                music_type: v.music_type,
                start_level: v.player1.start_level,
                start_height: v.player1.start_height,
            },
        }
    }

    /// Restore into freshly cleared RAM. Writing the table also writes the power-on marker, so
    /// `initRam` keeps it instead of loading the default table.
    pub(crate) fn store(&self, v: &mut Vars) {
        if let Some(h) = &self.high_scores {
            h.store(v);
            v.init_magic = crate::flow::INIT_MAGIC;
        }
        v.game_type = self.choices.game_type;
        v.music_type = self.choices.music_type;
        v.player1.start_level = self.choices.start_level;
        v.player1.start_height = self.choices.start_height;
    }
}
