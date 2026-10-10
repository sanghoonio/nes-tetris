//! The readable game's sound: the game-type menu's music comes out audible, at the right rate.

use std::sync::Arc;
use tetris_rs::{buttons, Rom, Runner};

#[test]
fn menu_music_is_audible() {
    let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tetris.nes")).unwrap();
    let mut game = Runner::new(Arc::new(Rom::new(&rom).unwrap()), Some(44100));
    let mut samples = Vec::new();
    for f in 0..600 {
        let start = (300..305).contains(&f) || (340..345).contains(&f);
        samples.extend(game.next_frame(if start { buttons::START } else { 0 }).samples);
    }
    let secs = samples.len() as f64 / 44100.0;
    assert!((9.9..10.1).contains(&secs), "{} samples for 600 frames", samples.len());
    let tail = &samples[samples.len() / 2..];
    let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
    assert!(rms > 0.01, "silent: rms {rms}");
}
