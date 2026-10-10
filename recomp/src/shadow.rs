//! Shadow mode: run the game normally, and on every call to a watched routine hand the caller the
//! machine state before and after the call. The reimplementation's tests replay `before` through
//! the readable version of the routine and compare against `after`, so every routine is checked
//! on real in-game inputs while the original keeps the game going.

use crate::machine::Machine;

pub struct Call<'a> {
    pub addr: u16,
    /// State at entry, after the `jsr` pushed its return address.
    pub before: Machine,
    /// State just after the routine returned.
    pub after: &'a Machine,
    /// APU register writes the routine made, in order.
    pub apu_writes: Vec<(u16, u8)>,
}

/// Run `body` (a recompiled routine) and report the call.
pub fn watch(m: &mut Machine, addr: u16, body: fn(&mut Machine)) {
    let before = m.fork();
    let outer_log = m.apu_log.replace(Vec::new());
    body(m);
    let apu_writes = std::mem::replace(&mut m.apu_log, outer_log).unwrap_or_default();
    if let Some(log) = &mut m.apu_log {
        log.extend_from_slice(&apu_writes);
    }
    if let Some(mut handler) = m.shadow.take() {
        handler(Call { addr, before, after: m, apu_writes });
        m.shadow = Some(handler);
    }
}
