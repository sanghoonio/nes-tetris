//! The app's menus and controls, driven the way the app drives them: keys by name, which the
//! game turns into NES buttons per screen (`pc_input`).

use std::sync::Arc;
use tetris_rs::{Config, FrameOut, Keys, Options, Rom, Runner, Vars};

pub struct App(pub Runner);

impl App {
    pub fn new(config: Config) -> Self {
        let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tetris.nes")).unwrap();
        let options = Options { menus: true, config };
        App(Runner::with_options(Arc::new(Rom::new(&rom).unwrap()), None, None, options))
    }

    /// Run a frame holding `held`, of which `pressed` were just pressed.
    pub fn frame(&mut self, held: &[&str], pressed: &[&str]) -> FrameOut {
        let keys = Keys { held: held.iter().map(|s| s.to_string()).collect(), pressed: pressed.iter().map(|s| s.to_string()).collect(), ..Default::default() };
        self.0.next_frame_keys(keys)
    }

    pub fn idle(&mut self, frames: usize) -> FrameOut {
        let mut out = self.frame(&[], &[]);
        for _ in 1..frames {
            out = self.frame(&[], &[]);
        }
        out
    }

    /// Press and release `key` (with `with` held), then let `frames` pass.
    pub fn tap_with(&mut self, with: &[&str], key: &str, frames: usize) -> FrameOut {
        let mut held: Vec<&str> = with.to_vec();
        held.push(key);
        let mut out = self.frame(&held, &[key]);
        for _ in 0..frames {
            out = self.frame(with, &[]);
        }
        out
    }

    pub fn tap(&mut self, key: &str, frames: usize) -> FrameOut {
        self.tap_with(&[], key, frames)
    }
}

fn mode(out: &FrameOut) -> u8 {
    Vars::from_ram(&out.ram).game_mode
}

/// SETTINGS' place on the title menu (VERSUS comes before it when built in).
const SETTINGS_ITEM: usize = if cfg!(feature = "versus") { 2 } else { 1 };

/// Power on and skip the legal screen: the title menu is up.
fn at_title() -> App {
    let mut app = App::new(Config::default());
    app.idle(10);
    let out = app.tap("Enter", 20);
    assert_eq!(mode(&out), 1);
    app
}

#[test]
fn title_waits_without_a_demo() {
    let mut app = at_title();
    for _ in 0..2000 {
        assert_eq!(mode(&app.idle(1)), 1, "left the title screen on its own");
    }
    // The arrows flicker: shown three frames in four.
    let shown = (0..8).filter(|_| app.idle(1).oam.chunks(4).any(|s| s[1] == 0x27 && s[0] < 0xEF)).count();
    assert_eq!(shown, 6);
}

#[test]
fn start_goes_to_the_game_type_menu() {
    let mut app = at_title();
    assert_eq!(mode(&app.tap("Enter", 10)), 2);
}

#[test]
fn exit_quits() {
    let mut app = at_title();
    app.tap("ArrowUp", 5); // wraps to EXIT
    assert!(app.tap("Enter", 1).quit);
}

#[test]
fn escape_goes_back_through_the_menus() {
    let mut app = at_title();
    app.tap("Enter", 10); // game type
    app.tap("Enter", 10); // level
    assert_eq!(mode(&app.tap("Escape", 10)), 2);
    assert_eq!(mode(&app.tap("Escape", 10)), 1);
    // The game keys do nothing in menus: Z (rotate left) used to be B, "back".
    app.tap("Enter", 10);
    assert_eq!(mode(&app.tap("KeyZ", 10)), 2);
}

#[test]
fn shift_enter_starts_ten_levels_up() {
    let mut app = at_title();
    app.tap("Enter", 10);
    app.tap("Enter", 10);
    app.tap("ArrowRight", 5); // level 1
    // Holding Shift relabels the grid 10..19.
    let plain = app.idle(1).pixels;
    app.frame(&["ShiftLeft"], &["ShiftLeft"]);
    let shifted = app.frame(&["ShiftLeft"], &[]).pixels;
    let cell = |p: &[u8]| (88..96).flat_map(|y| (56..64).map(move |x| y * 256 + x)).map(|i| p[i]).collect::<Vec<_>>();
    assert_ne!(cell(&plain[..]), cell(&shifted[..]), "level 0's cell unchanged with Shift");
    let out = app.tap_with(&["ShiftLeft"], "Enter", 5);
    assert_eq!(Vars::from_ram(&out.ram).player1.start_level, 11);
}

#[test]
fn b_type_crosses_between_level_and_height() {
    let mut app = at_title();
    app.tap("Enter", 10);
    app.tap("ArrowRight", 5); // B-type
    app.tap("Enter", 10);
    for _ in 0..4 {
        app.tap("ArrowRight", 3); // level 4, the grid's right edge
    }
    app.tap("ArrowRight", 3); // across to height
    let out = app.tap("ArrowRight", 3); // height 1
    let v = Vars::from_ram(&out.ram);
    assert_eq!((v.player1.start_level, v.player1.start_height), (4, 1));
    app.tap("ArrowLeft", 3); // height 0
    app.tap("ArrowLeft", 3); // back across to level
    let v = Vars::from_ram(&app.tap("ArrowLeft", 3).ram);
    assert_eq!((v.player1.start_level, v.player1.start_height), (3, 0));
}

/// A game under way (A-type, level 0).
fn playing(config: Config) -> App {
    let mut app = App::new(config);
    app.idle(10);
    for _ in 0..4 {
        app.tap("Enter", 10);
    }
    let out = app.idle(100);
    assert_eq!(mode(&out), 4);
    app
}

#[test]
fn pause_menu() {
    let mut app = playing(Config::default());
    let out = app.tap("Escape", 5);
    let tiles: Vec<u8> = out.oam.chunks(4).filter(|s| s[0] < 0xEF).map(|s| s[1]).collect();
    // PAUSED, RESUME and QUIT in sprites.
    for t in [0x19, 0x0A, 0x1E, 0x1C, 0x0E, 0x0D, 0x1B, 0x16, 0x1A, 0x12, 0x1D] {
        assert!(tiles.contains(&t), "tile {t:02X} missing from {tiles:02X?}");
    }
    // Escape resumes (and isn't taken as a second pause while still held).
    let held = app.frame(&["Escape"], &["Escape"]);
    let _ = held;
    for _ in 0..10 {
        app.frame(&["Escape"], &[]);
    }
    let out = app.idle(10);
    assert!(!out.oam.chunks(4).any(|s| s[1] == 0x19 && s[0] < 0xEF), "still paused");
    // Pause again, QUIT: back to the title.
    app.tap("Escape", 5);
    app.tap("ArrowDown", 3);
    let out = app.tap("Enter", 20);
    assert_eq!(mode(&out), 1);
}

#[test]
fn pause_items_centred_under_pause() {
    let mut app = playing(Config::default());
    let out = app.tap("Escape", 5);
    // Leftmost sprite of each row, centred on x 132: RESUME starts at 108, QUIT at 116.
    let left = |y: u8, tile: u8| out.oam.chunks(4).filter(|s| s[0] == y && s[1] == tile).map(|s| s[3]).min();
    assert_eq!(left(127, 0x1B), Some(108)); // R
    assert_eq!(left(139, 0x1A), Some(116)); // Q
    // "PAUSED" two rows above where the original's "PAUSE" was, centred on it.
    assert_eq!(left(103, 0x19), Some(108)); // P
}

#[test]
fn losing_focus_pauses() {
    let mut app = playing(Config::default());
    let paused = |out: &FrameOut| out.oam.chunks(4).any(|s| s[1] == 0x19 && s[0] < 0xEF);
    let away = Keys { unfocused: true, ..Default::default() };
    let mut out = app.0.next_frame_keys(away.clone());
    for _ in 0..30 {
        out = app.0.next_frame_keys(away.clone());
    }
    assert!(paused(&out), "not paused while unfocused");
    // Coming back leaves it paused until RESUME.
    assert!(paused(&app.idle(30)));
    app.tap("Enter", 10);
    assert!(!paused(&app.idle(10)), "still paused after RESUME");
}

#[test]
fn level_counter_shows_the_true_level() {
    let mut app = playing(Config::default());
    // Set player 1's level and flag the counter for redrawing.
    let mut show = |level: u8| {
        app.0.next_frame_with(0, vec![(0x64, level), (0xA3, 0x02)]);
        app.idle(2)
    };
    // The three cells under "LEVEL" ($22B9..$22BB).
    let cells = |out: &FrameOut| out.vram[0x2B9..0x2BC].to_vec();
    // The original shows 00 here (the bytes past `levelDisplayTable`).
    assert_eq!(cells(&show(35)), [0xFF, 3, 5]);
    assert_eq!(cells(&show(155)), [1, 5, 5]);
    // Past 255 the level wraps to 0, and the hundreds goes.
    assert_eq!(cells(&show(0)), [0xFF, 0, 0]);
}

/// Every level plays and draws from the ROM's data alone: none reads the original's code
/// (as its level counter does above 29, and its colours above 137); `Rom` panics if one does.
#[test]
fn every_level_reads_only_data() {
    for level in 0..=255u8 {
        let mut app = playing(Config::default());
        app.0.next_frame_with(0, vec![(0x64, level), (0xA3, 0x02)]);
        for _ in 0..300 {
            app.frame(&["ArrowDown"], &[]);
        }
    }
}

#[test]
fn game_keys_in_play() {
    let mut app = playing(Config::default());
    let x = Vars::from_ram(&app.idle(1).ram).player1.tetrimino_x;
    let out = app.tap("ArrowLeft", 1);
    assert_eq!(Vars::from_ram(&out.ram).player1.tetrimino_x, x - 1);
    // Rebound: move left on A.
    let mut config = Config::default();
    config.keys[tetris_rs::options::MOVE_LEFT] = "KeyA".into();
    let mut app = playing(config);
    let x = Vars::from_ram(&app.idle(1).ram).player1.tetrimino_x;
    assert_eq!(Vars::from_ram(&app.tap("ArrowLeft", 1).ram).player1.tetrimino_x, x, "arrow unbound");
    assert_eq!(Vars::from_ram(&app.tap("KeyA", 1).ram).player1.tetrimino_x, x - 1);
}

#[test]
fn next_piece_setting() {
    let shown = |config| {
        let mut app = playing(config);
        Vars::from_ram(&app.idle(5).ram).display_next_piece
    };
    assert_eq!(shown(Config::default()), 0);
    assert_eq!(shown(Config { next_piece: false, ..Config::default() }), 1);
}

#[test]
fn settings_rebind_and_back() {
    let mut app = at_title();
    for _ in 0..SETTINGS_ITEM {
        app.tap("ArrowDown", 5);
    }
    app.tap("Enter", 10); // CONTROLS
    for _ in 0..4 {
        app.tap("ArrowDown", 3); // ROTATE RIGHT
    }
    app.tap("Enter", 3);
    let out = app.tap("KeyK", 0);
    assert_eq!(out.config.expect("rebinding reports the new keys").keys[4], "KeyK");
    // K again on ROTATE LEFT swaps the two.
    app.tap("ArrowDown", 3);
    app.tap("Enter", 3);
    let keys = app.tap("KeyK", 0).config.unwrap().keys;
    assert_eq!((keys[4].as_str(), keys[5].as_str()), ("KeyZ", "KeyK"));
    // To SYSTEM (CONTROLS' last item, then GAME's), volume down.
    app.tap("ArrowDown", 3);
    app.tap("ArrowDown", 3); // GAME SETTINGS
    app.tap("Enter", 5);
    app.tap("ArrowUp", 3); // SYSTEM SETTINGS
    app.tap("Enter", 5);
    let out = app.tap("ArrowLeft", 0);
    assert_eq!(out.config.expect("volume change").volume, 9);
    // Escape back to the title, cursor on SETTINGS.
    assert_eq!(mode(&app.tap("Escape", 30)), 1);
    assert_eq!(mode(&app.tap("Enter", 10)), 1, "SETTINGS again, not START");
}

#[test]
fn name_entry_by_typing() {
    let mut app = playing(Config::default());
    // Top out quickly with a score for the table: fill the playfield, let the piece lock.
    let score = Vars::LAYOUT.iter().find(|l| l.0 == "player1").unwrap().1 as u16 + 0x13;
    let mut out = app.idle(1);
    for _ in 0..3000 {
        if Vars::from_ram(&out.ram).render_mode == 2 {
            break; // the congratulations screen
        }
        let mut pokes: Vec<(u16, u8)> = (0..200u16).map(|i| (0x0400 + 20 + i, 0x7B)).collect();
        pokes.extend([(score, 0x00), (score + 1, 0x00), (score + 2, 0x02)]);
        out = app.0.next_frame_with(0, pokes);
        if Vars::from_ram(&out.ram).player1.play_state == 10 {
            out = app.tap("Enter", 2);
        }
    }
    assert_eq!(Vars::from_ram(&out.ram).render_mode, 2, "no name entry");
    app.idle(10);
    for k in ["KeyS", "KeyA", "KeyM", "KeyX", "Backspace", "Digit1"] {
        app.tap(k, 3);
    }
    let out = app.tap("Enter", 10);
    let names = Vars::from_ram(&out.ram).high_score_names;
    let row: String = names[..6].iter().map(|&c| tetris_rs::saved::NAME_CHARS.chars().nth(c as usize).unwrap()).collect();
    assert!(row.starts_with("SAM1"), "{row:?}");
}
