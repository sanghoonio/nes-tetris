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
//!     cargo test -p recomp --test oracle

mod common;
use common::reference::{rom_bytes, Reference};
use recomp::scripts::{self, Coverage, Script};
use recomp::stepper::Stepper;

fn compare(name: &str, frames: usize, video: bool, mut script: Script) -> Coverage {
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
            let reference = r.pixels();
            let bad: Vec<usize> = VISIBLE.filter(|&i| px[i] != reference[i]).collect();
            if let Some(&first) = bad.first() {
                panic!(
                    "{name}: frame {f} differs in {} pixels, first at ({}, {}): ref {:02X} ours {:02X}",
                    bad.len(), first % 256, first / 256, reference[first], px[first]
                );
            }
        }
        cov.observe(&ram, &snap.ram);
        ram = snap.ram;
    }
    println!("{name}: {cov:?}");
    cov
}

#[test]
fn attract_mode() {
    let cov = compare("attract", 6000, false, scripts::attract());
    assert!(cov.demo_frames > 1000, "demo never ran: {cov:?}");
}

#[test]
fn attract_mode_video() {
    compare("attract-video", 3000, true, scripts::attract());
}

#[test]
fn random_play_a() {
    let cov = compare("random-1", 20000, false, scripts::random_play(1));
    assert!(cov.games >= 3, "{cov:?}");
}

#[test]
fn random_play_b() {
    let cov = compare("random-2", 20000, false, scripts::random_play(0x9E3779B97F4A7C15));
    assert!(cov.games >= 3, "{cov:?}");
}

#[test]
fn random_play_video() {
    compare("random-video", 4000, true, scripts::random_play(7));
}

#[test]
fn level_29() {
    let cov = compare("level-29", 8000, false, scripts::level_29(29));
    assert!(cov.level_29_frames > 500, "{cov:?}");
}

#[test]
fn line_clears() {
    let cov = compare("line-clears", 15000, false, scripts::line_clears(3));
    assert!(cov.max_lines >= 240 && cov.level_29_frames > 0, "{cov:?}");
}

#[test]
fn line_clears_video() {
    let cov = compare("line-clears-video", 6000, true, scripts::line_clears(5));
    assert!(cov.line_clear_frames > 100, "{cov:?}");
}
