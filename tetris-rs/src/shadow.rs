//! The routine registry for the shadow oracle (`tests/shadow.rs`).
//!
//! Each module lists the original routines it reimplements: the address of the original, how to
//! run the readable version on a `Game`, any extra variables it uses only as scratch, and what it
//! hands back to its caller (6502 routines often return through a register or flag).

use crate::Game;

/// What a routine returns to its caller, to compare with the original's registers or flags.
pub enum Ret {
    /// Nothing the callers use.
    Unit,
    /// The zero flag (true = Z set), as tested by the caller's `beq`/`bne`.
    Zero(bool),
    /// The accumulator.
    A(u8),
}

/// The CPU registers at the call, for routines that take arguments in them.
#[derive(Clone, Copy, Debug)]
pub struct Regs {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub carry: bool,
}

pub struct Routine {
    /// The original's label.
    pub name: &'static str,
    pub addr: u16,
    pub run: fn(&mut Game, Regs) -> Ret,
    /// Named variables this routine uses as scratch, beyond `vars::SCRATCH`: whatever it leaves
    /// in them isn't compared.
    pub scratch: &'static [&'static str],
}

/// Every reimplemented routine.
pub fn all() -> Vec<&'static Routine> {
    [
        crate::rng::SHADOW,
        crate::controls::SHADOW,
        crate::input::SHADOW,
        crate::play::SHADOW,
        crate::player::SHADOW,
        crate::spawn::SHADOW,
        crate::sprites::SHADOW,
        crate::system::SHADOW,
    ].into_iter().flatten().collect()
}
