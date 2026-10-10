//! The pseudo-random number generator.

use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

/// One step of the game's 16-bit LFSR: shift right, feeding bit 1 XOR bit 9 into bit 15.
pub fn next(seed: u16) -> u16 {
    let feedback = ((seed >> 1) ^ (seed >> 9)) & 1;
    feedback << 15 | seed >> 1
}

impl Game {
    /// `generateNextPseudorandomNumber` on the main seed (every caller passes `rng_seed`, 2 bytes).
    pub fn step_rng(&mut self) {
        let seed = self.vars.rng_seed.0;
        // The original computes the feedback bit in tmp1, leaving bit 1 of the low byte there.
        self.vars.tmp1 = (seed as u8) & 0x02;
        self.vars.rng_seed.0 = next(seed);
    }
}

pub const SHADOW: &[Routine] = &[Routine {
    name: "generateNextPseudorandomNumber",
    addr: layout::GENERATE_NEXT_PSEUDORANDOM_NUMBER,
    run: |g, _| {
        g.step_rng();
        Ret::Unit
    },
    scratch: &[],
}];
