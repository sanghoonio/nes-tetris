//! Smoke test for the APU: the title music produces non-silent audio at the expected rate.

mod common;
use common::reference::rom_bytes;
use recomp::stepper::Stepper;

#[test]
fn title_music_is_audible() {
    let mut s = Stepper::with_audio(&rom_bytes(), Some(44100));
    let mut samples = Vec::new();
    // Legal screen, Start to the title screen, Start again to the game-type menu and its music.
    for f in 0..600 {
        let start = (300..305).contains(&f) || (340..345).contains(&f);
        let snap = s.next_frame(if start { recomp::machine::BTN_START } else { 0 }, false);
        if f % 100 == 99 {
            println!("frame {f}: gameMode {} apu {:02X?}", snap.ram[0xC0], snap.apu_regs);
        }
        samples.extend(snap.samples);
    }
    let secs = samples.len() as f64 / 44100.0;
    assert!((9.9..10.1).contains(&secs), "{} samples for 600 frames", samples.len());
    let tail = &samples[samples.len() / 2..];
    let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
    println!("rms {rms:.4} over {} samples", tail.len());
    assert!(rms > 0.01, "silent: rms {rms}");
}
