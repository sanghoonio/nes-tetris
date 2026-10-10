//! Invariant the reimplementation relies on: NMI only ever interrupts the game while it idles
//! waiting for vblank, never in the middle of game logic (the game never lags). That lets the
//! readable port run the NMI handler synchronously at each wait instead of preemptively.

mod common;
use common::reference::rom_bytes;
use recomp::scripts::{self, Script};
use recomp::stepper::Stepper;
use std::collections::BTreeMap;

/// Every address the NMI may interrupt: the two `lda verticalBlankingInterval / beq` idle
/// loops, in updateAudioWaitForNmiAndResetOamStaging ($AA37) and updateAudioAndWaitForNmi ($AA4D).
const WAIT_LOOPS: [u16; 4] = [0xAA37, 0xAA39, 0xAA4D, 0xAA4F];

fn interrupted(script: Script, frames: usize, seen: &mut BTreeMap<u16, usize>) {
    let mut s = Stepper::new(&rom_bytes());
    let mut script = script;
    let mut ram = [0u8; 0x800];
    for f in 0..frames {
        let (b, pokes) = script(f, &ram);
        let snap = s.next_frame_with(b, false, pokes);
        let sp = snap.sp as usize;
        let ret = u16::from_le_bytes([snap.ram[0x100 + sp + 2], snap.ram[0x100 + sp + 3]]);
        *seen.entry(ret).or_default() += 1;
        ram = snap.ram;
    }
}

#[test]
fn nmi_only_interrupts_idle_loops() {
    let mut seen = BTreeMap::new();
    interrupted(scripts::attract(), 6000, &mut seen);
    interrupted(scripts::random_play(1), 20000, &mut seen);
    interrupted(scripts::level_29(29), 8000, &mut seen);
    interrupted(scripts::line_clears(3), 15000, &mut seen);
    println!("interrupted addresses: {seen:04X?}");
    for a in seen.keys() {
        assert!(WAIT_LOOPS.contains(a), "NMI interrupted {a:04X}, outside the idle loops");
    }
}
