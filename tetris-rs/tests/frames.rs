//! Whole-game oracle: the readable game against the recompile, frame by frame.
//!
//! Both run the same scripted input. At every frame boundary (the original's NMI) every named
//! variable must match, and video memory (nametables, palette, sprites), and the visible picture (rows 8..232; see recomp's oracle for why the
//! overscan rows are left out).
//!
//! Pictures aren't compared on frames the original changed while drawing them: a PPU write or a
//! CHR/mirroring switch during the visible lines (screen transitions; the NMI's work overrunning
//! vblank when every panel is redrawn; palette uploads with rendering off, when the backdrop is
//! whichever entry the PPU address points at). The recompile draws line by line and shows part
//! old, part new; this game draws each frame from its end state. The recompile flags these
//! frames itself (`Snapshot::drawn_over`). Such frames that differ are counted and reported;
//! their variables are still compared.
//!
//!     cargo test -p tetris-rs --test frames -- --nocapture

use recomp::scripts::{self, Coverage, Script};
use recomp::stepper::Stepper;
use std::sync::Arc;
use tetris_rs::vars::{Player, SCRATCH};
use tetris_rs::{Config, Options, Rom, Runner, Vars};

fn rom_bytes() -> Vec<u8> {
    let path = std::env::var_os("TETRIS_ROM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("ROM {}: {e}", path.display()))
}

/// Working variables the original reuses as scratch in many routines. The shadow oracle already
/// lists them as scratch per routine, and the rewrites don't reproduce every leftover value in
/// them. They aren't compared here either: a leftover that matters shows up elsewhere, in another
/// variable (the demo's zero-page input) or in video memory (the B-type ending draws
/// generalCounter4/5, which is why the falling-piece sprite stager now keeps them exactly).
const FRAME_SCRATCH: &[&str] = &[
    "general_counter",
    "general_counter2",
    "general_counter3",
    "general_counter4",
    "general_counter5",
    "original_y",
    "selecting_level_or_height",
];

/// Named variables that differ between two RAM images, as "name: ours .. original ..".
fn var_diffs(ours: &[u8; 0x800], theirs: &[u8; 0x800]) -> Vec<String> {
    let mut out = Vec::new();
    for &(name, addr, size) in Vars::LAYOUT {
        if SCRATCH.contains(&name) || FRAME_SCRATCH.contains(&name) {
            continue;
        }
        if ["active", "player1", "player2"].contains(&name) {
            for &(field, off, fsize) in Player::LAYOUT {
                let r = addr + off..addr + off + fsize;
                if ours[r.clone()] != theirs[r.clone()] {
                    out.push(format!("{name}.{field}: ours {:02X?} original {:02X?}", &ours[r.clone()], &theirs[r]));
                }
            }
            continue;
        }
        let r = addr..addr + size;
        if ours[r.clone()] != theirs[r.clone()] {
            let firsts: Vec<String> = r
                .clone()
                .filter(|&a| ours[a] != theirs[a])
                .take(4)
                .map(|a| format!("[{}] ours {:02X} original {:02X}", a - addr, ours[a], theirs[a]))
                .collect();
            out.push(format!("{name}: {}", firsts.join(", ")));
        }
    }
    out
}

fn compare(name: &str, frames: usize, script: Script) -> Coverage {
    compare_with(name, frames, script, Options::original())
}

/// As `compare`, running ours with `options`. With the app's menus on, the title screen differs
/// (its menu and arrows), so video isn't compared until the next screen is up.
fn compare_with(name: &str, frames: usize, mut script: Script, options: Options) -> Coverage {
    let rom = rom_bytes();
    let menus = options.menus;
    let mut ours = Runner::with_options(Arc::new(Rom::new(&rom).expect("ROM")), None, None, options);
    let mut since_title = 0;
    let mut original = Stepper::new(&rom);
    let mut cov = Coverage::default();
    let mut ram = [0u8; 0x800];
    let mut blank_frame_diffs = 0;
    for f in 0..frames {
        let (buttons, pokes) = script(f, &ram);
        let theirs = original.next_frame_with(buttons, true, pokes.clone());
        let mine = ours.next_frame_with(buttons, pokes);
        let mut diffs = var_diffs(&mine.ram, &theirs.ram);
        if menus && theirs.ram[0xC0] == 1 {
            // The menu's arrows, staged as sprites.
            diffs.retain(|d| !d.starts_with("oam_staging"));
        }
        assert!(diffs.is_empty(), "{name}: frame {f} (game mode {}, state {}):\n  {}",
            theirs.ram[0xC0], theirs.ram[0xA7], diffs.join("\n  "));
        since_title = if theirs.ram[0xC0] <= 1 { 0 } else { since_title + 1 };
        if menus && since_title < 10 {
            continue_after_picture(&mut cov, &mut ram, &theirs.ram);
            continue;
        }
        if let Some(i) = (0..0x800).find(|&i| mine.vram[i] != theirs.vram[i]) {
            panic!("{name}: frame {f} nametable byte ${:04X} (of the 2 KB): ours {:02X} original {:02X}",
                i, mine.vram[i], theirs.vram[i]);
        }
        assert_eq!(mine.palette, theirs.palette, "{name}: frame {f} palette");
        assert_eq!(mine.oam, theirs.oam, "{name}: frame {f} OAM");
        let px = theirs.pixels.as_ref().unwrap();
        let bad: Vec<usize> = (8 * 256..232 * 256).filter(|&i| px[i] != mine.pixels[i]).collect();
        if theirs.drawn_over {
            if !bad.is_empty() {
                blank_frame_diffs += 1;
            }
            continue_after_picture(&mut cov, &mut ram, &theirs.ram);
            continue;
        }
        if let Some(&first) = bad.first() {
            panic!("{name}: frame {f} picture differs in {} pixels, first at ({}, {}): ours {:02X} original {:02X}",
                bad.len(), first % 256, first / 256, mine.pixels[first], px[first]);
        }
        continue_after_picture(&mut cov, &mut ram, &theirs.ram);
    }
    println!("{name}: {cov:?}; {blank_frame_diffs} transition frames differ in their picture");
    cov
}

fn continue_after_picture(cov: &mut Coverage, ram: &mut [u8; 0x800], now: &[u8; 0x800]) {
    cov.observe(ram, now);
    *ram = *now;
}

#[test]
fn attract() {
    compare("attract", 6000, scripts::attract());
}

#[test]
fn random_play() {
    let cov = compare("random-1", 12000, scripts::random_play(1));
    assert!(cov.games >= 3, "{cov:?}");
}

#[test]
fn level_29() {
    compare("level-29", 6000, scripts::level_29(29));
}

#[test]
fn line_clears() {
    compare("line-clears", 15000, scripts::line_clears(3));
}

/// The app's title menu, START chosen at once, then random play: everything after the title is
/// the original game. Nothing is pressed on the legal screen, which the app lets Start skip
/// early and the original doesn't.
#[test]
fn title_menu() {
    let mut play = scripts::random_play(2);
    let script: Script = Box::new(move |f, ram| {
        if ram[0xC0] == 0 {
            (0, vec![])
        } else if ram[0xC0] == 1 {
            (if f % 20 == 0 { 0x10 } else { 0 }, vec![])
        } else {
            play(f, ram)
        }
    });
    let cov = compare_with("title-menu", 8000, script, Options { menus: true, config: Config::default() });
    assert!(cov.games >= 2, "{cov:?}");
}

#[test]
fn endings() {
    compare("endings", 48000, scripts::endings(11));
}
