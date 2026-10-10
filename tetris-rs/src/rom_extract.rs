//! What the game keeps of the user's ROM: the PRG bytes in `rom_data::PRG_RANGES` (data, not the
//! original's code) and the CHR graphics. `build.rs` extracts these into the binary; `Rom::new`
//! goes through the same extraction, so the tests run on exactly what the binary holds.

use crate::rom_data::PRG_RANGES;

pub const PRG_SIZE: usize = 0x8000;
pub const CHR_SIZE: usize = 0x4000;

/// From PRG + CHR (the ROM file minus its header): the kept PRG bytes in `PRG_RANGES` order,
/// then the CHR.
pub fn extract(body: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    for &(start, len) in PRG_RANGES {
        let at = start as usize - 0x8000;
        out.extend_from_slice(&body[at..at + len as usize]);
    }
    out.extend_from_slice(&body[PRG_SIZE..PRG_SIZE + CHR_SIZE]);
    out
}

/// The length of what `extract` returns.
pub fn extracted_len() -> usize {
    PRG_RANGES.iter().map(|&(_, len)| len as usize).sum::<usize>() + CHR_SIZE
}

/// Back from `extract`: PRG ($8000-$FFFF, zero where not kept), which PRG bytes were kept,
/// and the CHR.
pub fn expand(extracted: &[u8]) -> (Vec<u8>, Vec<bool>, Vec<u8>) {
    let mut prg = vec![0; PRG_SIZE];
    let mut kept = vec![false; PRG_SIZE];
    let mut from = 0;
    for &(start, len) in PRG_RANGES {
        let (at, len) = (start as usize - 0x8000, len as usize);
        prg[at..at + len].copy_from_slice(&extracted[from..from + len]);
        kept[at..at + len].fill(true);
        from += len;
    }
    (prg, kept, extracted[from..].to_vec())
}
