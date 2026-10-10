//! The app's menus: the title menu (START / SETTINGS / EXIT, no attract demo) and the settings
//! screen, driven the way the app drives them (buttons plus the key pressed, by name).

use std::sync::Arc;
use tetris_rs::{buttons, Config, FrameOut, Options, Rom, Runner, Vars};

fn app() -> Runner {
    let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tetris.nes")).unwrap();
    let options = Options { menus: true, config: Config::default() };
    Runner::with_options(Arc::new(Rom::new(&rom).unwrap()), None, None, options)
}

/// Tap `buttons` (or a key) for one frame, then let `frames` pass.
fn tap(game: &mut Runner, buttons: u8, key: Option<&str>, frames: usize) -> FrameOut {
    let mut out = game.next_frame_with_key(buttons, key.map(String::from));
    for _ in 0..frames {
        out = game.next_frame(0);
    }
    out
}

/// Power on and pass the legal screen: the title menu is up.
fn at_title() -> Runner {
    let mut game = app();
    for _ in 0..300 {
        game.next_frame(0);
    }
    tap(&mut game, buttons::START, None, 20);
    game
}

fn mode(out: &FrameOut) -> u8 {
    Vars::from_ram(&out.ram).game_mode
}

#[test]
fn title_waits_without_a_demo() {
    let mut game = at_title();
    for _ in 0..2000 {
        assert_eq!(mode(&game.next_frame(0)), 1, "left the title screen on its own");
    }
    // The arrows flicker: shown three frames in four.
    let shown = (0..8).filter(|_| game.next_frame(0).oam.chunks(4).any(|s| s[1] == 0x27 && s[0] < 0xEF)).count();
    assert_eq!(shown, 6);
}

#[test]
fn start_goes_to_the_game_type_menu() {
    let mut game = at_title();
    let out = tap(&mut game, buttons::START, None, 10);
    assert_eq!(mode(&out), 2);
}

#[test]
fn exit_quits() {
    let mut game = at_title();
    tap(&mut game, buttons::UP, None, 5); // wraps to EXIT
    let out = tap(&mut game, buttons::START, None, 1);
    assert!(out.quit);
}

#[test]
fn settings_change_and_back() {
    let mut game = at_title();
    tap(&mut game, buttons::DOWN, None, 5);
    tap(&mut game, buttons::START, None, 10); // settings: the CONTROLS page
    // Rebind ROTATE RIGHT (A) to K.
    for _ in 0..4 {
        tap(&mut game, buttons::DOWN, None, 3);
    }
    tap(&mut game, buttons::START, Some("Enter"), 3);
    let out = tap(&mut game, 0, Some("KeyK"), 0);
    assert_eq!(out.config.expect("rebinding reports the new keys").keys[4], "KeyK");
    // Binding K to ROTATE LEFT too swaps them.
    tap(&mut game, buttons::DOWN, None, 3);
    tap(&mut game, buttons::START, Some("Enter"), 3);
    let out = tap(&mut game, 0, Some("KeyK"), 0);
    let keys = out.config.unwrap().keys;
    assert_eq!((keys[4].as_str(), keys[5].as_str()), ("KeyZ", "KeyK"));
    // To the GAME page (CONTROLS' last item), then SYSTEM (GAME's last), then volume down.
    for _ in 0..6 {
        tap(&mut game, buttons::UP, None, 3); // wraps to GAME SETTINGS
    }
    tap(&mut game, buttons::START, None, 5);
    tap(&mut game, buttons::UP, None, 3); // wraps to SYSTEM SETTINGS
    tap(&mut game, buttons::START, None, 5);
    let out = game.next_frame_with_key(buttons::LEFT, None);
    let out = if out.config.is_some() { out } else { game.next_frame(0) };
    assert_eq!(out.config.expect("volume change").volume, 9);
    // Back (Escape) returns to the title, cursor on SETTINGS.
    let out = tap(&mut game, 0, Some("Escape"), 30);
    assert_eq!(mode(&out), 1);
    let out = tap(&mut game, buttons::START, None, 10);
    assert_eq!(mode(&out), 1, "SETTINGS again, not START");
}

#[test]
fn enter_skips_the_legal_screen() {
    let mut game = app();
    tap(&mut game, 0, None, 10);
    let out = tap(&mut game, buttons::START, Some("Enter"), 10);
    assert_eq!(mode(&out), 1, "at the title");
}

#[test]
fn back_leaves_the_game_type_and_level_menus() {
    let mut game = at_title();
    tap(&mut game, buttons::START, None, 10); // game type menu
    tap(&mut game, buttons::START, None, 10); // level menu
    let out = tap(&mut game, 0, Some("Escape"), 10);
    assert_eq!(mode(&out), 2, "back to the game type menu");
    let out = tap(&mut game, 0, Some("Escape"), 10);
    assert_eq!(mode(&out), 1, "back to the title");
}

#[test]
fn quit_from_pause() {
    let mut game = at_title();
    tap(&mut game, buttons::START, None, 10);
    tap(&mut game, buttons::START, None, 10);
    tap(&mut game, buttons::START, None, 120); // playing
    assert_eq!(mode(&game.next_frame(0)), 4);
    let out = tap(&mut game, buttons::START, None, 10); // paused
    assert!(out.oam.chunks(4).any(|s| s[1] == 0x19), "PAUSE shown");
    // The hint: "ESC" / "TO QUIT" in sprites.
    let tiles: Vec<u8> = out.oam.chunks(4).filter(|s| s[0] < 0xEF).map(|s| s[1]).collect();
    for t in [0x0E, 0x1C, 0x0C, 0x1D, 0x18, 0x1A, 0x1E, 0x12] {
        assert!(tiles.contains(&t), "hint tile {t:02X} missing from {tiles:02X?}");
    }
    let out = tap(&mut game, 0, Some("Escape"), 20);
    assert_eq!(mode(&out), 1, "quit to the title");
    // And the title menu works as before.
    let out = tap(&mut game, buttons::START, None, 10);
    assert_eq!(mode(&out), 2);
}
