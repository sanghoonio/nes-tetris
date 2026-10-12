//! The list of rooms: rooms' summaries for each state, the directory's reports, expiry and order,
//! and listing settings not unreadying anyone.

use protocol::directory::{Report, ENTRY_TTL_MS};
use protocol::{Directory, Event, FromRoom, Room, RoomState, RoomSummary, Settings, ToRoom};

fn hello(room: &mut Room, conn: u64, name: &str) {
    room.handle(Event::Joined(conn));
    room.handle(Event::Message(conn, ToRoom::Hello { name: name.into(), watch: false }));
}

fn summary(code: &str, state: RoomState) -> RoomSummary {
    RoomSummary { code: code.into(), host: "ANA".into(), players: 1, spectators: 0, level: 18, same_seed: true, state }
}

#[test]
fn a_room_summarises_itself() {
    let mut room = Room::new(1);
    assert_eq!(room.summary("KQXR"), None, "no host yet");
    hello(&mut room, 1, "ana");
    let s = room.summary("KQXR").unwrap();
    assert_eq!((s.host.as_str(), s.players, s.level, s.same_seed, s.state), ("ANA", 1, 0, true, RoomState::Open));
    hello(&mut room, 2, "bo");
    assert_eq!(room.summary("KQXR").unwrap().state, RoomState::Full);
    room.handle(Event::Message(1, ToRoom::Ready { ready: true }));
    room.handle(Event::Message(2, ToRoom::Ready { ready: true }));
    assert_eq!(room.summary("KQXR").unwrap().state, RoomState::Playing);
}

#[test]
fn a_private_room_isnt_listed_and_listing_doesnt_unready() {
    let mut room = Room::new(1);
    hello(&mut room, 1, "ana");
    hello(&mut room, 2, "bo");
    room.handle(Event::Message(2, ToRoom::Ready { ready: true }));
    let out = room.handle(Event::Message(1, ToRoom::Settings { settings: Settings { public: false, ..Settings::default() } }));
    assert_eq!(room.summary("KQXR"), None);
    let lobby = out.iter().find_map(|s| match &s.msg {
        FromRoom::Lobby { players, .. } => Some(players.clone()),
        _ => None,
    });
    assert!(lobby.unwrap()[1].as_ref().unwrap().ready, "still ready");
    // A change to the match does unready.
    let out = room.handle(Event::Message(1, ToRoom::Settings { settings: Settings { level: 5, public: false, ..Settings::default() } }));
    let lobby = out.iter().find_map(|s| match &s.msg {
        FromRoom::Lobby { players, .. } => Some(players.clone()),
        _ => None,
    });
    assert!(!lobby.unwrap()[1].as_ref().unwrap().ready);
}

#[test]
fn reports_expiry_and_order() {
    let mut d = Directory::default();
    assert!(d.report(Report::Listed { summary: summary("AAAA", RoomState::Open) }, 0));
    assert!(d.report(Report::Listed { summary: summary("BBBB", RoomState::Open) }, 10));
    assert!(!d.report(Report::Listed { summary: summary("AAAA", RoomState::Open) }, 20), "unchanged");
    assert!(d.report(Report::Listed { summary: summary("AAAA", RoomState::Playing) }, 30));
    let codes: Vec<String> = d.list().into_iter().map(|s| s.code).collect();
    assert_eq!(codes, ["BBBB", "AAAA"], "newest first");
    // BBBB was last heard at 10, AAAA at 30.
    assert!(!d.expire(ENTRY_TTL_MS + 9));
    assert!(d.expire(ENTRY_TTL_MS + 10));
    assert_eq!(d.list().len(), 1);
    assert!(d.report(Report::Unlisted { code: "AAAA".into() }, 40));
    assert!(!d.report(Report::Unlisted { code: "AAAA".into() }, 50));
    assert!(d.list().is_empty());
    assert_eq!(d.next_expiry(), None);
}

#[test]
fn settings_without_public_default_to_listed() {
    let json = r#"{"level":18,"pieces":"Identical","rules":{"das_delay":16,"das_repeat":6,"left_right":"Both","hard_drop":false,"ghost":false,"next_piece":true}}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert!(s.public);
}
