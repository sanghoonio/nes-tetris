//! Bots playing whole games through the engine: they must clear lines, survive, and get through
//! the speed-ups, pressing only buttons.

use std::sync::Arc;

use bots::{Bot, Profile};
use tetris_rs::options::Rules;
use tetris_rs::{GameSetup, Pieces, Rom, Runner, Seed, Vars};

fn rom() -> Arc<Rom> {
    let file = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tetris.nes")).unwrap();
    Arc::new(Rom::new(&file).unwrap())
}

/// A game from `level` with `seed`, played by a bot until it tops out or `frames` pass. Returns
/// (lines, score, frames played).
fn play(profile: Profile, level: u8, rules: Rules, seed: u32, frames: u32) -> (u32, u32, u32) {
    let rom = rom();
    let setup = GameSetup {
        b_type: false,
        level,
        height: 0,
        music: 3,
        seed: Seed::from_u32(seed),
        pieces: Pieces::Original,
        rules,
        next_piece: true,
    };
    let mut game = Runner::start_game(rom.clone(), None, setup);
    let mut bot = Bot::new(&rom, profile, seed as u64);
    let mut out = game.next_frame(0);
    let mut played = 0;
    while played < frames && !out.game_over {
        let b = bot.buttons(&out.ram, &rules);
        out = game.next_frame(b);
        played += 1;
    }
    let v = Vars::from_ram(&out.ram);
    (v.player1.lines.value(), v.player1.score.value(), played)
}

#[test]
fn survives_level_0() {
    // Ten minutes of level 0 and up (it speeds up every 10 lines).
    let (lines, score, frames) = play(Profile::best(), 0, Rules::ORIGINAL, 7, 36_000);
    println!("level 0: {lines} lines, {score} points in {frames} frames");
    assert!(lines >= 100, "{lines} lines");
}

#[test]
fn plays_level_18() {
    let mut total = 0;
    for seed in [1, 2, 3] {
        let (lines, score, frames) = play(Profile::best(), 18, Rules::ORIGINAL, seed, 60_000);
        println!("level 18, seed {seed}: {lines} lines, {score} points in {frames} frames");
        total += lines;
    }
    assert!(total >= 60, "{total} lines over three games");
}

#[test]
fn holding_instead_of_tapping() {
    let profile = Profile { tap_frames: None, reaction: 10, look_ahead: false, ..Profile::best() };
    let (lines, score, frames) = play(profile, 5, Rules::ORIGINAL, 11, 20_000);
    println!("level 5, holding: {lines} lines, {score} points in {frames} frames");
    assert!(lines >= 20, "{lines} lines");
}

#[test]
fn with_hard_drop() {
    let rules = Rules { hard_drop: true, ..Rules::ORIGINAL };
    let (lines, score, frames) = play(Profile::best(), 18, rules, 5, 60_000);
    println!("level 18, hard drop: {lines} lines, {score} points in {frames} frames");
    assert!(lines >= 60, "{lines} lines");
}

/// The search's cost per piece, which the host pays in a room (and the browser on the web).
#[test]
fn choosing_is_quick() {
    let rom = rom();
    let shapes = bots::Shapes::from_rom(&rom);
    let board = bots::search::Board::empty();
    let start = std::time::Instant::now();
    let mut n = 0;
    for o in 0..19u8 {
        for p in bots::search::placements(&shapes, &board, o, 5, 0) {
            let (after, _) = board.with(&shapes, &p);
            n += bots::search::placements(&shapes, &after, 2, 5, 0).len();
        }
    }
    let per_piece = start.elapsed() / 19;
    println!("{n} two-piece placements; about {per_piece:?} a piece with look-ahead (without scoring)");
    assert!(per_piece < std::time::Duration::from_millis(5));
}
