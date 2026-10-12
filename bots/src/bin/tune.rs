//! Tune the bots' weights by self-play: the cross-entropy method. Each generation draws
//! candidate weights around the current mean, plays each in the same set of headless games (the
//! strongest bot's play, starts at several levels), keeps the best quarter by mean score, and
//! moves the mean and spread towards them. New seeds every generation, so it can't learn one
//! game.
//!
//!     cargo run --release -p bots --bin tune [hours] [path/to/tetris.nes]
//!
//! Prints each generation and writes the current mean, with its average score over the last ten
//! generations (each generation's games are new, so one generation's score is noisy), to
//! `target/bots-tuned.txt`, as Rust to paste into `Weights::TUNED`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bots::eval::{FEATURES, N};
use bots::{Bot, Profile, Weights, ROSTER};
use tetris_rs::options::Rules;
use tetris_rs::{GameSetup, Pieces, Rom, Runner, Seed, Vars};

const CANDIDATES: usize = 28;
const ELITE: usize = 7;
/// Games per candidate per generation: their start levels.
const LEVELS: [u8; 6] = [18, 18, 19, 15, 12, 18];
/// Longest game, in frames (about 10 minutes; a level 18 start reaches 29 well before).
const FRAMES: u32 = 36_000;

fn main() {
    let mut args = std::env::args().skip(1);
    let hours: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(6.0);
    let path = args.next().unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../tetris.nes").to_string());
    let rom = Arc::new(Rom::new(&std::fs::read(&path).expect("the ROM")).expect("Tetris (USA)"));
    // All but three cores, leaving room for other work meanwhile.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).saturating_sub(3).max(1);
    let deadline = Instant::now() + Duration::from_secs_f64(hours * 3600.0);
    let mut mean = Weights::DEFAULT.0;
    let mut spread: [f64; N] = mean.map(|w| w.abs() * 0.5 + 1.0);
    let mut rng = 0x2545_F491_4F6C_DD1Du64;
    let mut recent: Vec<f64> = vec![];
    println!("tuning for {hours} hours on {threads} threads; {CANDIDATES} candidates x {} games a generation", LEVELS.len());
    let mut generation = 0;
    while Instant::now() < deadline {
        generation += 1;
        let start = Instant::now();
        let seeds: Vec<u32> = LEVELS.iter().map(|_| (next(&mut rng) >> 32) as u32).collect();
        // The mean itself, and candidates around it.
        let mut candidates = vec![mean];
        for _ in 1..CANDIDATES {
            candidates.push(std::array::from_fn(|i| mean[i] + spread[i] * normal(&mut rng)));
        }
        let scores = evaluate(&rom, &candidates, &seeds, threads);
        let mut order: Vec<usize> = (0..CANDIDATES).collect();
        order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
        let elite: Vec<[f64; N]> = order[..ELITE].iter().map(|&i| candidates[i]).collect();
        for i in 0..N {
            let m = elite.iter().map(|e| e[i]).sum::<f64>() / ELITE as f64;
            let v = elite.iter().map(|e| (e[i] - m).powi(2)).sum::<f64>() / ELITE as f64;
            mean[i] = 0.3 * mean[i] + 0.7 * m;
            // Never narrower than a little, so it keeps looking.
            spread[i] = (0.3 * spread[i] + 0.7 * v.sqrt()).max(0.05 + 0.02 * mean[i].abs());
        }
        let mean_score = scores[0];
        recent.push(mean_score);
        let last = &recent[recent.len().saturating_sub(10)..];
        let average = last.iter().sum::<f64>() / last.len() as f64;
        save(&mean, average, generation);
        println!(
            "generation {generation}: mean weights scored {:.0}, best candidate {:.0}, ({:.0?})",
            mean_score,
            scores[order[0]],
            start.elapsed()
        );
    }
    println!("done");
}

/// Each candidate's mean score over a game per seed (with the matching start level).
fn evaluate(rom: &Arc<Rom>, candidates: &[[f64; N]], seeds: &[u32], threads: usize) -> Vec<f64> {
    let jobs: Vec<(usize, usize)> = (0..candidates.len()).flat_map(|c| (0..seeds.len()).map(move |g| (c, g))).collect();
    let next_job = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(vec![0.0; candidates.len()]);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let j = next_job.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&(c, g)) = jobs.get(j) else { break };
                let score = play(rom, Weights(candidates[c]), LEVELS[g], seeds[g]);
                results.lock().unwrap()[c] += score as f64 / seeds.len() as f64;
            });
        }
    });
    results.into_inner().unwrap()
}

/// A game played by the strongest bot with `weights`: its final score.
fn play(rom: &Arc<Rom>, weights: Weights, level: u8, seed: u32) -> u32 {
    let rules = Rules::ORIGINAL;
    let setup = GameSetup { b_type: false, level, height: 0, music: 3, seed: Seed::from_u32(seed), pieces: Pieces::Original, rules, next_piece: true };
    let mut game = Runner::start_game(rom.clone(), None, setup);
    let profile = Profile { weights, ..ROSTER[ROSTER.len() - 1].1 };
    let mut bot = Bot::new(rom, profile, seed as u64);
    let mut out = game.next_frame(0);
    for _ in 0..FRAMES {
        if out.game_over {
            break;
        }
        out = game.next_frame(bot.buttons(&out.ram, &rules));
    }
    Vars::from_ram(&out.ram).player1.score.value()
}

/// Write `weights` to `target/bots-tuned.txt`, as `Weights::TUNED` would be.
fn save(weights: &[f64; N], score: f64, generation: usize) {
    let mut text = format!("// generation {generation}: the mean's score over the last ten generations {score:.0}\n");
    text.push_str("pub const TUNED: Weights = Weights([\n");
    for (w, name) in weights.iter().zip(FEATURES) {
        text.push_str(&format!("    {w:.4}, // {name}\n"));
    }
    text.push_str("]);\n");
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/bots-tuned.txt");
    let _ = std::fs::write(path, text);
}

fn next(rng: &mut u64) -> u64 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    *rng
}

fn normal(rng: &mut u64) -> f64 {
    let u = ((next(rng) >> 11) as f64 + 0.5) / (1u64 << 53) as f64;
    let v = ((next(rng) >> 11) as f64 + 0.5) / (1u64 << 53) as f64;
    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}
