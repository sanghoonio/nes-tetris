//! What a room server allows, against floods: how many may wait in a room (`Room` applies
//! these), watch the list, how long a message may be, and how many a connection may send. The
//! Worker applies them all; `local-room` all but the message rate, since tests play faster than
//! real time.
//!
//! A connection that ends without closing (a laptop asleep, a network gone) is found by its
//! silence: a room link sends `PING` every `PING_MS`, answered `PONG` (by the Worker without
//! waking the room), and the Worker closes one that hasn't pinged for `SILENCE_MS`.

/// Spectators a room takes.
pub const MAX_SPECTATORS: usize = 16;
/// Connections a room keeps that haven't said `Hello`; another closes the oldest. A real client
/// says `Hello` at once, so only idle ones go.
pub const MAX_PENDING: usize = 4;
/// Connections watching the list of rooms.
pub const MAX_WATCHERS: usize = 1000;
/// Longest message a server reads, in bytes (an input batch of 60 frames is about 300).
pub const MAX_MESSAGE_BYTES: usize = 4096;
/// Messages a connection may send at once, and how many a second after that. A player sends
/// about 4 a second, 8 with a bot.
pub const BURST: f64 = 60.0;
pub const PER_SECOND: f64 = 30.0;

/// The heartbeat: bare text, not messages. Hidden browser tabs run timers about once a minute,
/// hence the margin.
pub const PING: &str = "ping";
pub const PONG: &str = "pong";
pub const PING_MS: u64 = 30_000;
pub const SILENCE_MS: u64 = 3 * 60_000;

/// The reasons in `FromRoom::Closed`.
pub const FULL: &str = "room is full";
pub const NO_HELLO: &str = "no hello";
pub const TOO_LONG: &str = "message too long";
pub const TOO_MANY: &str = "too many messages";
pub const NO_PING: &str = "no ping";

/// A connection's allowance of messages: `BURST`, refilled at `PER_SECOND`.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    left: f64,
    at: u64,
}

impl Budget {
    pub fn new(now_ms: u64) -> Self {
        Budget { left: BURST, at: now_ms }
    }

    /// Spend one message at `now_ms`; false if there's none left.
    pub fn spend(&mut self, now_ms: u64) -> bool {
        let refill = now_ms.saturating_sub(self.at) as f64 / 1000.0 * PER_SECOND;
        self.left = (self.left + refill).min(BURST);
        self.at = self.at.max(now_ms);
        if self.left < 1.0 {
            return false;
        }
        self.left -= 1.0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_then_the_rate() {
        let mut b = Budget::new(0);
        assert!((0..60).all(|_| b.spend(0)));
        assert!(!b.spend(0));
        // A second later, 30 more.
        assert!((0..30).all(|_| b.spend(1000)));
        assert!(!b.spend(1000));
        // A player's pace (8 a second) never runs out.
        let mut b = Budget::new(0);
        assert!((0..10_000u64).all(|i| b.spend(i * 125)));
    }
}
