//! Frame oracle: the recompile against tetanes-core running the original ROM.
//!
//! Both run the same inputs and are compared at every NMI entry: all of RAM (including the live
//! part of the stack), and optionally the rendered frame.
//!
//! One deliberate tolerance: our cycle count is within a couple of cycles of tetanes but not
//! identical, so the vblank edge can land on a different instruction of the game's idle loop
//! (`lda verticalBlankingInterval / beq`). That changes only the 3 bytes the interrupt pushes
//! (status and return address), which are dead once the handler returns. Those are checked
//! separately: the two return addresses must be within 4 bytes of each other.
//!
//! Frames are compared over rows 8..232, the part a TV shows. During palette uploads the game
//! leaves rendering off, and the PPU then shows whichever palette entry the VRAM address points
//! at; that colour changes mid-line as the upload proceeds, so the top overscan rows depend on
//! exact write cycles. The renderer models the effect but only per line.
//!
//!     cargo test --release -p recomp --test oracle

mod common;
use common::reference::{rom_bytes, Reference};
use recomp::machine::*;
use recomp::stepper::Stepper;

/// RAM addresses from disasm/tetris-ram.asm used to steer scripted runs.
const GAME_MODE: u16 = 0x00C0;
const LEVEL_NUMBER: u16 = 0x0044;
const PLAYER1_LEVEL_NUMBER: u16 = 0x0064;
const PLAY_STATE: u16 = 0x0048;

const PLAYER1_LINES: usize = 0x0070;
const PLAYER1_SCORE: usize = 0x0073;
const COMPLETED_LINES: usize = 0x0056;
const PLAYER1_PLAY_STATE: usize = 0x0068;
const PLAYFIELD: u16 = 0x0400;
const BLOCK_TILE: u8 = 0x7B;

/// What a run actually exercised, so a passing test can't be a run that sat in a menu.
#[derive(Debug, Default)]
struct Coverage {
    play_frames: usize,
    demo_frames: usize,
    games: usize,
    line_clear_frames: usize,
    max_lines: u16,
    max_score: u32,
    level_29_frames: usize,
}

fn bcd(bytes: &[u8]) -> u32 {
    bytes.iter().rev().fold(0, |acc, &b| acc * 100 + (b >> 4) as u32 * 10 + (b & 15) as u32)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Run both for `frames` frames. `script(frame, ram)` returns the buttons to hold and any RAM pokes
/// to apply (to both) before running the frame.
fn compare(name: &str, frames: usize, video: bool, mut script: impl FnMut(usize, &[u8; 0x800]) -> (u8, Vec<(u16, u8)>)) -> Coverage {
    let mut cov = Coverage::default();
    let mut r = Reference::with_video(video);
    let mut s = Stepper::new(&rom_bytes());
    let mut ram = [0u8; 0x800];
    for f in 0..frames {
        let (buttons, pokes) = script(f, &ram);
        for &(a, v) in &pokes {
            r.poke(a, v);
        }
        r.run_to_nmi(buttons);
        let snap = s.next_frame_with(buttons, video, pokes);
        assert!(snap.fallback_log.is_empty(), "{name}: interpreter ran at {:04X?}", snap.fallback_log);
        assert_eq!(r.sp(), snap.sp, "{name}: SP differs at frame {f}");
        let frame_bytes = 0x100 + snap.sp as usize + 1..0x100 + snap.sp as usize + 4;
        let ret = |ram: &[u8]| u16::from_le_bytes([ram[frame_bytes.start + 1], ram[frame_bytes.start + 2]]);
        let (ref_ret, our_ret) = (ret(&r.wram()[..]), ret(&snap.ram[..]));
        assert!(
            ref_ret.abs_diff(our_ret) <= 4,
            "{name}: NMI interrupted different code at frame {f}: ref {ref_ret:04X} ours {our_ret:04X}"
        );
        let live_stack = frame_bytes.end..0x200;
        let diffs: Vec<String> = (0..0x800)
            .filter(|a| !(0x100..0x200).contains(a) || live_stack.contains(a))
            .filter(|&a| r.wram()[a] != snap.ram[a])
            .map(|a| format!("{a:04X}: ref {:02X} ours {:02X}", r.wram()[a], snap.ram[a]))
            .collect();
        assert!(diffs.is_empty(), "{name}: RAM differs at frame {f}:\n  {}", diffs.join("\n  "));
        if let Some(px) = &snap.pixels {
            const VISIBLE: std::ops::Range<usize> = 8 * 256..232 * 256;
            let reference = &r.pixels()[VISIBLE];
            let px = &px[VISIBLE];
            let bad = px.iter().zip(reference.iter()).filter(|(a, b)| a != b).count();
            if bad > 0 {
                let idx: Vec<usize> = (0..px.len()).filter(|&i| px[i] != reference[i]).map(|i| i + VISIBLE.start).collect();
                let (px, reference) = (&snap.pixels.as_ref().unwrap()[..], &r.pixels());
                let (xs, ys): (Vec<usize>, Vec<usize>) = idx.iter().map(|&i| (i % 256, i / 256)).unzip();
                let sample: Vec<String> = idx.iter().take(6).map(|&i| format!("({},{}) ref {:02X} ours {:02X}", i % 256, i / 256, reference[i], px[i])).collect();
                panic!(
                    "{name}: frame {f} differs in {bad} pixels, x {}..={} y {}..={}; {}",
                    xs.iter().min().unwrap(), xs.iter().max().unwrap(), ys.iter().min().unwrap(), ys.iter().max().unwrap(),
                    sample.join(", ")
                );
            }
        }
        let mode = snap.ram[GAME_MODE as usize];
        if mode == 4 {
            cov.play_frames += 1;
            if ram[GAME_MODE as usize] != 4 {
                cov.games += 1;
            }
            if snap.ram[COMPLETED_LINES] > 0 {
                cov.line_clear_frames += 1;
            }
            cov.max_lines = cov.max_lines.max(bcd(&snap.ram[PLAYER1_LINES..PLAYER1_LINES + 2]) as u16);
            cov.max_score = cov.max_score.max(bcd(&snap.ram[PLAYER1_SCORE..PLAYER1_SCORE + 3]));
            if snap.ram[PLAYER1_LEVEL_NUMBER as usize] == 29 {
                cov.level_29_frames += 1;
            }
        } else if mode == 5 {
            cov.demo_frames += 1;
        }
        ram = snap.ram;
    }
    println!("{name}: {cov:?}");
    cov
}

#[test]
fn attract_mode() {
    // No input: legal screen, title, then the demo game, repeatedly.
    let cov = compare("attract", 6000, false, |_, _| (0, vec![]));
    assert!(cov.demo_frames > 1000, "demo never ran: {cov:?}");
}

#[test]
fn attract_mode_video() {
    compare("attract-video", 3000, true, |_, _| (0, vec![]));
}

/// Random play: mash Start through the menus, random movement and rotation in game, held down
/// often so pieces drop fast. Each new game gets its bottom 8 rows pre-filled except for one
/// column, so random drops actually complete lines; Start is tapped on the game-over screens.
fn random_play(seed: u64) -> impl FnMut(usize, &[u8; 0x800]) -> (u8, Vec<(u16, u8)>) {
    let mut rng = Rng(seed);
    let mut held = 0u8;
    let mut in_game = false;
    move |f, ram| {
        if ram[GAME_MODE as usize] != 4 {
            in_game = false;
            // Menus: tap Start every 20 frames, sometimes moving the cursor.
            return match f % 20 {
                0 => (BTN_START, vec![]),
                10 => ([BTN_RIGHT, BTN_DOWN, BTN_A, 0][(rng.next() % 4) as usize], vec![]),
                _ => (0, vec![]),
            };
        }
        let mut pokes = vec![];
        if !in_game && ram[PLAYER1_PLAY_STATE] == 1 {
            in_game = true;
            let hole = (rng.next() % 10) as u16;
            for row in 12..20u16 {
                for col in 0..10u16 {
                    if col != hole {
                        pokes.push((PLAYFIELD + row * 10 + col, BLOCK_TILE));
                    }
                }
            }
        }
        if matches!(ram[PLAYER1_PLAY_STATE], 0 | 10) {
            return (if f % 20 == 0 { BTN_START } else { 0 }, pokes);
        }
        if f % 6 == 0 {
            let r = rng.next();
            held = 0;
            if r & 1 != 0 { held |= BTN_DOWN; }
            if r & 6 == 2 { held |= BTN_LEFT; }
            if r & 6 == 4 { held |= BTN_RIGHT; }
            if r & 8 != 0 { held |= BTN_A; }
            if r & 0x30 == 0x10 { held |= BTN_B; }
        }
        (held, pokes)
    }
}

#[test]
fn random_play_a() {
    let cov = compare("random-1", 20000, false, random_play(1));
    assert!(cov.games >= 3, "{cov:?}");
}

#[test]
fn random_play_b() {
    let cov = compare("random-2", 20000, false, random_play(0x9E3779B97F4A7C15));
    assert!(cov.games >= 3, "{cov:?}");
}

#[test]
fn random_play_video() {
    compare("random-video", 4000, true, random_play(7));
}

#[test]
fn level_29() {
    // Start a game, then jump the level to 29 once play begins and let pieces fall.
    let mut play = random_play(29);
    let mut poked = false;
    let cov = compare("level-29", 8000, false, move |f, ram| {
        let (b, mut pokes) = play(f, ram);
        if ram[GAME_MODE as usize] == 4 && ram[PLAY_STATE as usize] == 1 && !poked {
            pokes.push((LEVEL_NUMBER, 29));
            pokes.push((PLAYER1_LEVEL_NUMBER, 29));
            poked = true;
        }
        if ram[GAME_MODE as usize] != 4 {
            poked = false;
        }
        (b, pokes)
    });
    assert!(cov.level_29_frames > 500, "{cov:?}");
}

#[test]
fn line_clears() {
    // Every piece becomes a vertical I over a one-column well at its spawn column, held down:
    // a Tetris per piece, so levels, scoring, the line-clear animation and level 29+ all run.
    // Games start at level 18, as high-level play does; it reaches 29 after 240 lines.
    const PLAYER1_TETRIMINO_Y: usize = 0x0061;
    const CURRENT_PIECE: u16 = 0x0042;
    const PLAYER1_CURRENT_PIECE: u16 = 0x0062;
    const I_VERTICAL: u8 = 0x11;
    const WELL_COLUMN: u16 = 5;
    let mut menus = random_play(3);
    let mut last_y = 0xFF;
    let cov = compare("line-clears", 15000, false, move |f, ram| {
        if ram[GAME_MODE as usize] != 4 || matches!(ram[PLAYER1_PLAY_STATE], 0 | 10) {
            last_y = 0xFF;
            let (b, _) = menus(f, ram);
            return (b, vec![]);
        }
        let y = ram[PLAYER1_TETRIMINO_Y];
        let mut pokes = vec![];
        if last_y == 0xFF {
            pokes.push((LEVEL_NUMBER, 18));
            pokes.push((PLAYER1_LEVEL_NUMBER, 18));
        }
        if ram[PLAYER1_PLAY_STATE] == 1 && y < last_y {
            // A piece just spawned.
            pokes.push((CURRENT_PIECE, I_VERTICAL));
            pokes.push((PLAYER1_CURRENT_PIECE, I_VERTICAL));
            for row in 16..20u16 {
                for col in 0..10u16 {
                    let tile = if col == WELL_COLUMN { 0xEF } else { BLOCK_TILE };
                    pokes.push((PLAYFIELD + row * 10 + col, tile));
                }
            }
        }
        last_y = if ram[PLAYER1_PLAY_STATE] == 1 { y } else { last_y.min(0xFE) };
        (BTN_DOWN, pokes)
    });
    assert!(cov.max_lines >= 240 && cov.level_29_frames > 0, "{cov:?}");
}
