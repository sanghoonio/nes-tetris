//! Saved state restored at power-on reaches the menus, and the high score rows round-trip.

use std::sync::Arc;
use tetris_rs::saved::{Choices, HighScores};
use tetris_rs::{buttons, Rom, Runner, Saved, Vars};

fn rom() -> Arc<Rom> {
    let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tetris.nes")).unwrap();
    Arc::new(Rom::new(&rom).unwrap())
}

/// Power on, press Start past the legal screen, the title and the game-type menu, and return the RAM at the
/// level menu.
fn to_level_menu(saved: Option<Saved>) -> Vars {
    let mut game = Runner::with_saved(rom(), None, saved);
    let mut ram = [0; 0x800];
    for f in 0..600 {
        let start = [300, 340, 380].iter().any(|&s| (s..s + 5).contains(&f));
        ram = game.next_frame(if start { buttons::START } else { 0 }).ram;
    }
    let v = Vars::from_ram(&ram);
    assert_eq!(v.game_mode, 3, "not at the level menu");
    v
}

#[test]
fn restores_choices_and_high_scores() {
    let mut table = HighScores::empty();
    table.set_row(0, "sam", 123_456, 18);
    table.set_row(4, "B-TYPE", 7_000, 9);
    let choices = Choices { game_type: 1, music_type: 2, start_level: 7, start_height: 3 };
    let v = to_level_menu(Some(Saved { high_scores: Some(table.clone()), choices }));
    assert_eq!((v.game_type, v.music_type, v.player1.start_level, v.player1.start_height), (1, 2, 7, 3));
    let now = HighScores::from_vars(&v);
    assert_eq!(now, table);
    assert_eq!(now.row(0), ("SAM   ".into(), 123_456, 18));
    assert_eq!(now.row(4), ("B-TYPE".into(), 7_000, 9));
}

#[test]
fn without_saved_state_the_default_table_loads() {
    let v = to_level_menu(None);
    let h = HighScores::from_vars(&v);
    assert_eq!(h.row(0), ("HOWARD".into(), 10_000, 9));
    assert_eq!(h.row(6), ("NINTEN".into(), 500, 0));
    assert_eq!((v.game_type, v.music_type, v.player1.start_level), (0, 0, 0));
}
