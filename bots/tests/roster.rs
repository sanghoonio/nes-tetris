//! The roster is in order: over the same games, each bot scores more than the one before it.

use std::sync::Arc;

use bots::{Bot, ROSTER};
use tetris_rs::options::Rules;
use tetris_rs::{GameSetup, Pieces, Rom, Runner, Seed, Vars};

/// Start level and games, each played to the top-out (at most about half an hour).
const LEVEL: u8 = 9;
const SEEDS: [u32; 12] = [11, 22, 33, 44, 55, 66, 77, 88, 99, 111, 122, 133];
const FRAMES: u32 = 120_000;

fn score(rom: &Arc<Rom>, bot: usize, seed: u32) -> u32 {
    let rules = Rules::ORIGINAL;
    let setup = GameSetup { b_type: false, level: LEVEL, height: 0, music: 3, seed: Seed::from_u32(seed), pieces: Pieces::Original, rules, next_piece: true };
    let mut game = Runner::start_game(rom.clone(), None, setup);
    let mut player = Bot::new(rom, ROSTER[bot].1, seed as u64);
    let mut out = game.next_frame(0);
    for _ in 0..FRAMES {
        if out.game_over {
            break;
        }
        out = game.next_frame(player.buttons(&out.ram, &rules));
    }
    Vars::from_ram(&out.ram).player1.score.value()
}

#[test]
fn each_beats_the_one_before() {
    let file = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tetris.nes")).unwrap();
    let rom = Arc::new(Rom::new(&file).unwrap());
    let means: Vec<f64> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..ROSTER.len())
            .map(|b| {
                let rom = &rom;
                s.spawn(move || SEEDS.iter().map(|&seed| score(rom, b, seed) as f64).sum::<f64>() / SEEDS.len() as f64)
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (b, mean) in means.iter().enumerate() {
        println!("{}: {mean:.0} points on average from level {LEVEL}", ROSTER[b].0);
    }
    for b in 1..ROSTER.len() {
        assert!(means[b] > means[b - 1], "{} ({:.0}) should beat {} ({:.0})", ROSTER[b].0, means[b], ROSTER[b - 1].0, means[b - 1]);
    }
}
