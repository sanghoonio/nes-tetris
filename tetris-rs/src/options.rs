//! The app's additions to the game, and the settings it edits.
//!
//! `Options::original()` is the game as it was: no title menu, the attract demo, original
//! rules. The tests run that. The app turns the menus on and passes its saved `Config`.

/// The actions a key can be bound to: the eight NES buttons, then Back (leaves the settings
/// screen).
pub const ACTIONS: [&str; 9] = ["UP", "DOWN", "LEFT", "RIGHT", "ROTATE RIGHT", "ROTATE LEFT", "START", "SELECT", "BACK"];
pub const BACK: usize = 8;

/// What happens when Left and Right are held together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftRight {
    /// Both reach the game, as on a controller that allows it.
    Both,
    /// The one pressed last wins.
    Last,
    /// Neither.
    Neither,
}

/// Rule changes, each off (the original) by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// Frames Left/Right is held before the piece starts repeating (the original's 16).
    pub das_delay: u8,
    /// Frames between repeats after that (the original's 6).
    pub das_repeat: u8,
    pub left_right: LeftRight,
    /// Holding Down+Left+Right runs the game at half speed (the original: the held buttons, $07,
    /// are left in A and happen to equal the next `gameModeState`, so the main loop waits an
    /// extra frame every frame).
    pub slowdown: bool,
    /// Up drops the piece to the bottom and locks it.
    pub hard_drop: bool,
    /// Show where the piece would land: its sprites at the landing row, every other frame (the
    /// NES's way of drawing something see-through).
    pub ghost: bool,
}

impl Rules {
    pub const ORIGINAL: Rules = Rules {
        das_delay: 16,
        das_repeat: 6,
        left_right: LeftRight::Both,
        slowdown: true,
        hard_drop: false,
        ghost: false,
    };
}

impl Default for Rules {
    fn default() -> Self {
        Rules::ORIGINAL
    }
}

/// Everything the settings screen edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The key bound to each of `ACTIONS`, by name (`KeyX`, `ArrowUp`, `Enter`...).
    pub keys: [String; 9],
    /// 0..=10.
    pub volume: u8,
    pub save_high_scores: bool,
    pub fullscreen: bool,
    /// Scale the picture by whole numbers only.
    pub integer_scale: bool,
    pub rules: Rules,
}

impl Default for Config {
    fn default() -> Self {
        let keys = ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "KeyX", "KeyZ", "Enter", "ShiftRight", "Escape"];
        Config {
            keys: keys.map(String::from),
            volume: 10,
            save_high_scores: true,
            fullscreen: false,
            integer_scale: false,
            rules: Rules::ORIGINAL,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The title menu (START / SETTINGS / EXIT) and the settings screen; no attract demo.
    pub menus: bool,
    pub config: Config,
}

impl Options {
    /// The game as it was.
    pub fn original() -> Self {
        Options { menus: false, config: Config::default() }
    }
}

/// A key's name as the game can print it (letters, digits and a little punctuation).
pub fn key_label(name: &str) -> String {
    let named = [
        ("ArrowUp", "UP"),
        ("ArrowDown", "DOWN"),
        ("ArrowLeft", "LEFT"),
        ("ArrowRight", "RIGHT"),
        ("Enter", "ENTER"),
        ("NumpadEnter", "ENTER"),
        ("Escape", "ESC"),
        ("Space", "SPACE"),
        ("Tab", "TAB"),
        ("Backspace", "BKSP"),
        ("ShiftLeft", "L SHIFT"),
        ("ShiftRight", "R SHIFT"),
        ("ControlLeft", "L CTRL"),
        ("ControlRight", "R CTRL"),
        ("AltLeft", "L OPT"),
        ("AltRight", "R OPT"),
        ("SuperLeft", "L CMD"),
        ("SuperRight", "R CMD"),
        ("MetaLeft", "L CMD"),
        ("MetaRight", "R CMD"),
        ("CapsLock", "CAPS"),
        ("Comma", ","),
        ("Period", "."),
        ("Slash", "/"),
        ("Minus", "-"),
        ("Quote", "\""),
        ("BracketLeft", "("),
        ("BracketRight", ")"),
    ];
    if let Some((_, label)) = named.iter().find(|(n, _)| *n == name) {
        return label.to_string();
    }
    let short = name.strip_prefix("Key").or_else(|| name.strip_prefix("Digit")).unwrap_or(name);
    let short = short.replace("Numpad", "NUM ");
    short.to_uppercase().chars().filter(|c| crate::saved::NAME_CHARS.contains(*c)).take(9).collect()
}
