//! The app's PC controls: which NES buttons the keys held this frame stand for, depending on
//! the screen.
//!
//! The original reads an NES controller. In the app every key has one job per screen: menus
//! use fixed keys (arrows, Enter, Escape), play uses the remappable game keys
//! (`Config::keys`). The app's own screens (title menu, settings, pause menu, name typing) read
//! the keys pressed directly instead.
//!
//! A key held while the screen changes is ignored until it's released, so it can't turn into
//! a different button on the new screen (Escape pausing, then resuming the next frame).

use crate::options::{HARD_DROP, MOVE_LEFT, MOVE_RIGHT, PAUSE, ROTATE_LEFT, ROTATE_RIGHT, SOFT_DROP};
use crate::{buttons, Game};

/// The keys this frame, by name: winit `KeyCode` names (`KeyX`, `ArrowUp`), and `Pad*` for
/// gamepad buttons (`PadSouth`, `PadEast`, `PadNorth`, `PadWest`, `PadStart`, `PadSelect`,
/// `PadUp`/`Down`/`Left`/`Right`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub held: Vec<String>,
    /// Pressed since the last frame, in order.
    pub pressed: Vec<String>,
    /// The window isn't focused (switched away from): play pauses.
    pub unfocused: bool,
}

/// Which keys mean what on the screen that's up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Screen {
    /// Decided by the game's state: menus, or play.
    #[default]
    Game,
    /// The app's own screens, which read keys directly.
    TitleMenu,
    Settings,
    Pause,
    /// Name entry: arrows and Enter as in menus, plus typing.
    NameEntry,
    /// The endings: Enter continues.
    Ending,
    /// The VERSUS screens (`pc_versus`).
    #[cfg(feature = "versus")]
    Versus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mapping {
    Menu,
    Play,
    /// The app's screens: no NES buttons.
    Own,
    NameEntry,
}

/// A menu key, as the app's own screens read them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuKey {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
}

const MENU_KEYS: [(&str, MenuKey); 13] = [
    ("ArrowUp", MenuKey::Up),
    ("ArrowDown", MenuKey::Down),
    ("ArrowLeft", MenuKey::Left),
    ("ArrowRight", MenuKey::Right),
    ("Enter", MenuKey::Confirm),
    ("NumpadEnter", MenuKey::Confirm),
    ("Escape", MenuKey::Back),
    ("PadUp", MenuKey::Up),
    ("PadDown", MenuKey::Down),
    ("PadLeft", MenuKey::Left),
    ("PadRight", MenuKey::Right),
    ("PadSouth", MenuKey::Confirm),
    ("PadEast", MenuKey::Back),
];
const PAD_START: &str = "PadStart";

/// In play: the NES buttons the keys `held` stand for, with the game keys `bindings` (by
/// `options::ACTIONS`) and the gamepad's fixed layout. Up (hard drop) only with `hard_drop`.
pub fn play_buttons(bindings: &[String; 7], hard_drop: bool, held: &[&str]) -> u8 {
    let bound = [
        (MOVE_LEFT, buttons::LEFT),
        (MOVE_RIGHT, buttons::RIGHT),
        (SOFT_DROP, buttons::DOWN),
        (HARD_DROP, if hard_drop { buttons::UP } else { 0 }),
        (ROTATE_RIGHT, buttons::A),
        (ROTATE_LEFT, buttons::B),
        (PAUSE, buttons::START),
    ];
    let pad = [
        ("PadLeft", buttons::LEFT),
        ("PadRight", buttons::RIGHT),
        ("PadDown", buttons::DOWN),
        ("PadNorth", if hard_drop { buttons::UP } else { 0 }),
        ("PadEast", buttons::A),
        ("PadSouth", buttons::B),
        (PAD_START, buttons::START),
    ];
    let mut out = 0;
    for k in held {
        for &(action, bit) in &bound {
            if bindings[action] == *k {
                out |= bit;
            }
        }
        for &(name, bit) in &pad {
            if name == *k {
                out |= bit;
            }
        }
    }
    out
}

impl Game {
    /// The NES buttons this frame's keys stand for.
    pub(crate) fn translate_keys(&mut self, keys: &Keys) -> u8 {
        let unfocused = keys.unfocused;
        let mapping = self.mapping();
        // Keys held across a change of mapping wait to be released.
        if Some(mapping) != self.ui.last_mapping {
            self.ui.stale = keys.held.clone();
            self.ui.last_mapping = Some(mapping);
        }
        self.ui.stale.retain(|k| keys.held.contains(k));
        let live: Vec<&str> = keys.held.iter().filter(|k| !self.ui.stale.contains(k)).map(String::as_str).collect();
        let mut out = 0;
        match mapping {
            Mapping::Own => {}
            Mapping::Menu | Mapping::NameEntry => {
                for k in &live {
                    out |= match MENU_KEYS.iter().find(|(n, _)| n == k).map(|&(_, m)| m) {
                        Some(MenuKey::Up) => buttons::UP,
                        Some(MenuKey::Down) => buttons::DOWN,
                        Some(MenuKey::Left) => buttons::LEFT,
                        Some(MenuKey::Right) => buttons::RIGHT,
                        Some(MenuKey::Confirm) => buttons::START,
                        // Back is B in the original's menus; name entry has no back.
                        Some(MenuKey::Back) if mapping == Mapping::Menu => buttons::B,
                        _ if *k == PAD_START => buttons::START,
                        _ => 0,
                    };
                }
            }
            Mapping::Play => {
                // Losing focus presses Start, released every other frame so the press lands
                // even on frames the game doesn't look for it (line clears, entry delay).
                if unfocused && self.joypads.held[0] & buttons::START == 0 {
                    out |= buttons::START;
                }
                out |= play_buttons(&self.options.config.keys, self.options.config.rules.hard_drop, &live);
            }
        }
        out
    }

    pub(crate) fn mapping(&self) -> Mapping {
        match self.ui.screen {
            Screen::TitleMenu | Screen::Settings | Screen::Pause => Mapping::Own,
            #[cfg(feature = "versus")]
            Screen::Versus => Mapping::Own,
            Screen::NameEntry => Mapping::NameEntry,
            Screen::Ending => Mapping::Menu,
            // Play, except the game-over curtain (Enter continues there).
            Screen::Game if self.vars.game_mode == 4 && self.vars.player1.play_state != 0x0A => Mapping::Play,
            Screen::Game => Mapping::Menu,
        }
    }

    /// The menu keys pressed this frame (the app's own screens).
    pub(crate) fn menu_pressed(&self) -> Vec<MenuKey> {
        let mut out = vec![];
        for k in &self.ui.keys.pressed {
            if let Some(&(_, m)) = MENU_KEYS.iter().find(|(n, _)| n == k) {
                out.push(m);
            } else if k == PAD_START {
                out.push(MenuKey::Confirm);
            }
        }
        out
    }

    pub(crate) fn shift_held(&self) -> bool {
        self.ui.keys.held.iter().any(|k| k == "ShiftLeft" || k == "ShiftRight")
    }
}
