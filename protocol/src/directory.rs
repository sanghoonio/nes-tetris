//! The list of rooms: what every public room last said about itself (`Room::summary`). Whoever
//! runs it (the Worker's directory, `local-room`, a test) feeds in the rooms' reports and sends the
//! list to whoever is watching it whenever it changes.
//!
//! A room reports whenever its summary changes and when it closes. One that stops without saying
//! so (its server restarted) would stay listed, so entries not refreshed for `ENTRY_TTL_MS` are
//! dropped; a room with people in it refreshes every `REFRESH_MS`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::RoomSummary;

/// How long an entry lasts without a report.
pub const ENTRY_TTL_MS: u64 = 10 * 60 * 1000;
/// How often a room with people in it reports, changed or not (the Worker's rooms also close
/// silent connections then, `limits::SILENCE_MS`).
pub const REFRESH_MS: u64 = 60 * 1000;

/// What a room tells the directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Report {
    /// The room as it is now.
    Listed { summary: RoomSummary },
    /// The room is empty, or no longer public.
    Unlisted { code: String },
}

impl Report {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("messages serialise")
    }

    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    summary: RoomSummary,
    /// When the room was first listed, and last reported (ms, the directory's clock).
    since: u64,
    seen: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Directory {
    entries: BTreeMap<String, Entry>,
}

impl Directory {
    /// Take a room's report at time `now`. Returns whether the list changed.
    pub fn report(&mut self, report: Report, now: u64) -> bool {
        match report {
            Report::Listed { summary } => match self.entries.get_mut(&summary.code) {
                Some(entry) => {
                    entry.seen = now;
                    let changed = entry.summary != summary;
                    entry.summary = summary;
                    changed
                }
                None => {
                    self.entries.insert(summary.code.clone(), Entry { summary, since: now, seen: now });
                    true
                }
            },
            Report::Unlisted { code } => self.entries.remove(&code).is_some(),
        }
    }

    /// Drop entries not reported since `ENTRY_TTL_MS` before `now`. Returns whether the list
    /// changed.
    pub fn expire(&mut self, now: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|_, e| now.saturating_sub(e.seen) < ENTRY_TTL_MS);
        self.entries.len() != before
    }

    /// The rooms, newest first.
    pub fn list(&self) -> Vec<RoomSummary> {
        let mut entries: Vec<&Entry> = self.entries.values().collect();
        entries.sort_by(|a, b| b.since.cmp(&a.since).then_with(|| a.summary.code.cmp(&b.summary.code)));
        entries.into_iter().map(|e| e.summary.clone()).collect()
    }

    /// The earliest an entry could expire, if there are any (for an alarm).
    pub fn next_expiry(&self) -> Option<u64> {
        self.entries.values().map(|e| e.seen + ENTRY_TTL_MS).min()
    }
}
