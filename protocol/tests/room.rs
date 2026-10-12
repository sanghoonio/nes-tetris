//! The room's rules: seats and spectators, the host's settings, readying and starting, passing on
//! inputs, the CTWC ending, disconnects and forfeits, late spectators, and confirming results.

use protocol::{ConnId, Event, FromRoom, Outcome, Pieces, Room, Send, Settings, ToRoom};

const HOST: ConnId = 1;
const OPP: ConnId = 2;
const WATCHER: ConnId = 3;

fn to(out: &[Send], conn: ConnId) -> Vec<FromRoom> {
    out.iter().filter(|s| s.to == conn).map(|s| s.msg.clone()).collect()
}

fn hello(room: &mut Room, conn: ConnId, name: &str, watch: bool) -> Vec<Send> {
    room.handle(Event::Joined(conn));
    room.handle(Event::Message(conn, ToRoom::Hello { name: name.into(), watch }))
}

fn msg(room: &mut Room, conn: ConnId, m: ToRoom) -> Vec<Send> {
    room.handle(Event::Message(conn, m))
}

fn inputs(room: &mut Room, conn: ConnId, frame: u32, n: usize, score: u32, over: bool) -> Vec<Send> {
    msg(room, conn, ToRoom::Inputs { frame, buttons: vec![0; n], score, over })
}

/// Host, opponent and a spectator, the match started.
fn playing(pieces: Pieces) -> (Room, [u32; 2]) {
    let mut room = Room::new(7);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, OPP, "bo", false);
    hello(&mut room, WATCHER, "cy", false);
    msg(&mut room, HOST, ToRoom::Settings { settings: Settings { pieces, ..Settings::default() } });
    msg(&mut room, HOST, ToRoom::Ready { ready: true });
    let out = msg(&mut room, OPP, ToRoom::Ready { ready: true });
    let start = to(&out, WATCHER).into_iter().find_map(|m| match m {
        FromRoom::Start { seeds, countdown_ms, .. } => Some((seeds, countdown_ms)),
        _ => None,
    });
    let (seeds, countdown) = start.expect("a start");
    assert_eq!(countdown, 3000);
    assert!(room.playing());
    (room, seeds)
}

fn result(out: &[Send], conn: ConnId) -> Option<(Outcome, [u32; 2], [u32; 2])> {
    to(out, conn).into_iter().find_map(|m| match m {
        FromRoom::Result { outcome, scores, frames, .. } => Some((outcome, scores, frames)),
        _ => None,
    })
}

#[test]
fn seats_then_spectators() {
    let mut room = Room::new(1);
    assert_eq!(to(&hello(&mut room, HOST, "ana", false), HOST)[0], FromRoom::Welcome { seat: Some(0) });
    assert_eq!(to(&hello(&mut room, 9, "eve", true), 9)[0], FromRoom::Welcome { seat: None });
    assert_eq!(to(&hello(&mut room, OPP, "bo", false), OPP)[0], FromRoom::Welcome { seat: Some(1) });
    let out = hello(&mut room, WATCHER, "cy", false);
    assert_eq!(to(&out, WATCHER)[0], FromRoom::Welcome { seat: None });
    let FromRoom::Lobby { players, spectators, .. } = &to(&out, HOST)[0] else { panic!() };
    assert_eq!(players[0].as_ref().unwrap().name, "ANA");
    assert_eq!(*spectators, 2);
}

#[test]
fn names_fit_the_font() {
    assert_eq!(protocol::clean_name("  zoë_99!!  longname"), "ZO99 LON");
    assert_eq!(protocol::clean_name("??"), "PLAYER");
}

#[test]
fn only_the_host_sets_and_a_change_unreadies() {
    let mut room = Room::new(1);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, OPP, "bo", false);
    msg(&mut room, OPP, ToRoom::Ready { ready: true });
    let settings = Settings { level: 12, ..Settings::default() };
    let out = msg(&mut room, OPP, ToRoom::Settings { settings });
    assert!(matches!(to(&out, OPP)[0], FromRoom::Error { .. }));
    let out = msg(&mut room, HOST, ToRoom::Settings { settings: Settings { level: 30, ..settings } });
    assert!(matches!(to(&out, HOST)[0], FromRoom::Error { .. }));
    let out = msg(&mut room, HOST, ToRoom::Settings { settings });
    let FromRoom::Lobby { players, settings: now, .. } = &to(&out, OPP)[0] else { panic!() };
    assert_eq!(now.level, 12);
    assert!(!players[1].as_ref().unwrap().ready, "a change has to be agreed to again");
}

#[test]
fn seeds_follow_the_piece_setting() {
    let (_, seeds) = playing(Pieces::Identical);
    assert_eq!(seeds[0], seeds[1]);
    let (_, seeds) = playing(Pieces::Separate);
    assert_ne!(seeds[0], seeds[1]);
}

#[test]
fn inputs_are_passed_on_in_order() {
    let (mut room, _) = playing(Pieces::Separate);
    let out = inputs(&mut room, HOST, 0, 15, 40, false);
    assert!(to(&out, HOST).is_empty());
    for c in [OPP, WATCHER] {
        assert_eq!(to(&out, c), vec![FromRoom::Inputs { seat: 0, frame: 0, buttons: vec![0; 15], score: 40, over: false }]);
    }
    // A gap, or a repeat, is refused.
    let out = inputs(&mut room, HOST, 30, 15, 40, false);
    assert!(matches!(to(&out, HOST)[0], FromRoom::Error { .. }));
    let out = inputs(&mut room, HOST, 0, 15, 40, false);
    assert!(matches!(to(&out, HOST)[0], FromRoom::Error { .. }));
    // Spectators can't play.
    let out = inputs(&mut room, WATCHER, 0, 15, 0, false);
    assert!(matches!(to(&out, WATCHER)[0], FromRoom::Error { .. }));
    // Checksums are passed on too.
    let out = msg(&mut room, OPP, ToRoom::Checksum { frame: 60, hash: 99 });
    assert_eq!(to(&out, HOST), vec![FromRoom::Checksum { seat: 1, frame: 60, hash: 99 }]);
}

#[test]
fn survivor_wins_on_passing_the_score() {
    let (mut room, _) = playing(Pieces::Separate);
    inputs(&mut room, HOST, 0, 15, 1000, false);
    let out = inputs(&mut room, HOST, 15, 10, 1200, true);
    assert_eq!(result(&out, HOST), None, "the opponent plays on");
    let out = inputs(&mut room, OPP, 0, 15, 1200, false);
    assert_eq!(result(&out, HOST), None, "level isn't passing");
    let out = inputs(&mut room, OPP, 15, 15, 1300, false);
    let (outcome, scores, frames) = result(&out, WATCHER).expect("decided");
    assert_eq!(outcome, Outcome::Win { seat: 1 });
    assert_eq!(scores, [1200, 1300]);
    assert_eq!(frames, [25, 30]);
    assert!(!room.playing());
}

#[test]
fn both_topped_out() {
    let (mut room, _) = playing(Pieces::Separate);
    inputs(&mut room, OPP, 0, 15, 800, true);
    let out = inputs(&mut room, HOST, 0, 15, 700, true);
    assert_eq!(result(&out, HOST).unwrap().0, Outcome::Win { seat: 1 });

    let (mut room, _) = playing(Pieces::Separate);
    inputs(&mut room, OPP, 0, 15, 800, true);
    let out = inputs(&mut room, HOST, 0, 15, 800, true);
    assert_eq!(result(&out, HOST).unwrap().0, Outcome::Draw);
}

#[test]
fn leaving_mid_match_forfeits() {
    let (mut room, _) = playing(Pieces::Separate);
    inputs(&mut room, OPP, 0, 15, 5000, false);
    let out = room.handle(Event::Left(OPP));
    assert_eq!(to(&out, HOST)[0], FromRoom::Gone { seat: 1 });
    assert_eq!(result(&out, HOST).unwrap().0, Outcome::Forfeit { seat: 1 });
    // The seat is free again, and only the host reports.
    let FromRoom::Lobby { players, .. } = to(&out, HOST).last().unwrap().clone() else { panic!() };
    assert!(players[1].is_none());
    let out = msg(&mut room, HOST, ToRoom::Report { match_id: 1, scores: [0, 5000] });
    assert_eq!(to(&out, HOST), vec![FromRoom::Confirmed { match_id: 1, agreed: true }]);
}

#[test]
fn leaving_after_topping_out_isnt_a_forfeit() {
    let (mut room, _) = playing(Pieces::Separate);
    inputs(&mut room, OPP, 0, 15, 5000, true);
    let out = room.handle(Event::Left(OPP));
    assert_eq!(result(&out, HOST), None, "the match goes on");
    let out = inputs(&mut room, HOST, 0, 15, 5100, false);
    assert_eq!(result(&out, HOST).unwrap().0, Outcome::Win { seat: 0 });
}

/// The first to leave had topped out (so the match went on); then the other leaves too.
#[test]
fn both_leaving_abandons() {
    let (mut room, _) = playing(Pieces::Separate);
    inputs(&mut room, HOST, 0, 15, 100, true);
    room.handle(Event::Left(HOST));
    let out = room.handle(Event::Left(OPP));
    assert_eq!(to(&out, WATCHER)[0], FromRoom::Abandoned { match_id: 1 });
    assert!(!room.playing());
}

#[test]
fn spectator_arriving_mid_match_catches_up() {
    let (mut room, seeds) = playing(Pieces::Separate);
    inputs(&mut room, HOST, 0, 15, 10, false);
    inputs(&mut room, HOST, 15, 15, 20, false);
    inputs(&mut room, OPP, 0, 15, 30, false);
    let out = hello(&mut room, 9, "dee", false);
    let got = to(&out, 9);
    assert_eq!(got[0], FromRoom::Welcome { seat: None }, "no seat during a match");
    assert!(got.iter().any(|m| matches!(m, FromRoom::Start { seeds: s, countdown_ms: 0, .. } if *s == seeds)));
    assert!(got.contains(&FromRoom::Inputs { seat: 0, frame: 0, buttons: vec![0; 30], score: 20, over: false }));
    assert!(got.contains(&FromRoom::Inputs { seat: 1, frame: 0, buttons: vec![0; 15], score: 30, over: false }));
}

#[test]
fn results_are_confirmed_by_the_reports() {
    for (opp_report, agreed) in [([300, 500], true), ([300, 400], false)] {
        let (mut room, _) = playing(Pieces::Separate);
        inputs(&mut room, HOST, 0, 15, 300, true);
        inputs(&mut room, OPP, 0, 15, 500, false);
        assert!(msg(&mut room, HOST, ToRoom::Report { match_id: 1, scores: [300, 500] }).is_empty());
        let out = msg(&mut room, OPP, ToRoom::Report { match_id: 1, scores: opp_report });
        assert_eq!(to(&out, WATCHER), vec![FromRoom::Confirmed { match_id: 1, agreed }]);
    }
}

#[test]
fn late_inputs_after_a_result_are_ignored() {
    let (mut room, _) = playing(Pieces::Separate);
    inputs(&mut room, HOST, 0, 15, 100, true);
    inputs(&mut room, OPP, 0, 15, 200, false);
    assert!(!room.playing());
    assert!(inputs(&mut room, OPP, 15, 15, 240, false).is_empty());
}

#[test]
fn rematch_after_a_result() {
    let (mut room, _) = playing(Pieces::Separate);
    inputs(&mut room, HOST, 0, 15, 100, true);
    inputs(&mut room, OPP, 0, 15, 200, false);
    msg(&mut room, HOST, ToRoom::Ready { ready: true });
    let out = msg(&mut room, OPP, ToRoom::Ready { ready: true });
    assert!(to(&out, HOST).iter().any(|m| matches!(m, FromRoom::Start { match_id: 2, .. })));
}

#[test]
fn host_leaving_the_lobby_hands_over() {
    let mut room = Room::new(1);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, OPP, "bo", false);
    let out = room.handle(Event::Left(HOST));
    assert_eq!(to(&out, OPP)[0], FromRoom::Welcome { seat: Some(0) });
    let out = msg(&mut room, OPP, ToRoom::Settings { settings: Settings::default() });
    assert!(matches!(to(&out, OPP)[0], FromRoom::Lobby { .. }));
}

#[test]
fn hello_first_and_once() {
    let mut room = Room::new(1);
    room.handle(Event::Joined(HOST));
    let out = msg(&mut room, HOST, ToRoom::Ready { ready: true });
    assert!(matches!(to(&out, HOST)[0], FromRoom::Error { .. }));
    hello(&mut room, OPP, "bo", false);
    let out = msg(&mut room, OPP, ToRoom::Hello { name: "x".into(), watch: false });
    assert!(matches!(to(&out, OPP)[0], FromRoom::Error { .. }));
}

#[test]
fn lobby_state_survives_saving() {
    let mut room = Room::new(1);
    hello(&mut room, HOST, "ana", false);
    msg(&mut room, HOST, ToRoom::Settings { settings: Settings { level: 9, ..Settings::default() } });
    let saved = serde_json::to_string(&room).unwrap();
    let mut back: Room = serde_json::from_str(&saved).unwrap();
    let out = hello(&mut back, OPP, "bo", false);
    let FromRoom::Lobby { players, settings, .. } = &to(&out, OPP)[1] else { panic!() };
    assert_eq!(settings.level, 9);
    assert_eq!(players[0].as_ref().unwrap().name, "ANA");
}

#[test]
fn messages_as_json() {
    let m = ToRoom::Inputs { frame: 15, buttons: vec![0, 4, 4], score: 120, over: false };
    assert_eq!(m.to_json(), r#"{"type":"Inputs","frame":15,"buttons":[0,4,4],"score":120,"over":false}"#);
    assert_eq!(ToRoom::from_json(&m.to_json()), Some(m));
    assert_eq!(ToRoom::from_json("{nonsense"), None);
}

#[test]
fn room_codes() {
    let codes: Vec<String> = (0..1000u64).map(|i| protocol::room_code(i.wrapping_mul(0x9E37_79B9_7F4A_7C15))).collect();
    assert!(codes.iter().all(|c| protocol::valid_code(c)));
    assert!(!protocol::valid_code("ABIO") && !protocol::valid_code("abcd") && !protocol::valid_code("ABCDE"));
}

#[test]
fn the_host_removes_the_opponent_between_matches() {
    let mut room = Room::new(1);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, OPP, "bo", false);
    hello(&mut room, WATCHER, "cy", true);
    assert!(to(&msg(&mut room, OPP, ToRoom::Kick), OPP).iter().any(|m| matches!(m, FromRoom::Error { .. })), "only the host");
    let out = msg(&mut room, HOST, ToRoom::Kick);
    assert_eq!(to(&out, OPP), vec![FromRoom::Kicked]);
    let lobby = to(&out, WATCHER).into_iter().find_map(|m| match m {
        FromRoom::Lobby { players, .. } => Some(players),
        _ => None,
    });
    assert!(lobby.unwrap()[1].is_none(), "the seat is free");
    // The kicked connection's close changes nothing more.
    room.handle(Event::Left(OPP));
    assert!(to(&msg(&mut room, HOST, ToRoom::Kick), HOST).iter().any(|m| matches!(m, FromRoom::Error { .. })), "nobody to remove");
}

#[test]
fn no_removing_during_a_match() {
    let (mut room, _) = playing(Pieces::Separate);
    let out = msg(&mut room, HOST, ToRoom::Kick);
    assert!(to(&out, OPP).is_empty());
    assert!(to(&out, HOST).iter().any(|m| matches!(m, FromRoom::Error { .. })));
}

#[test]
fn players_with_one_name_are_told_apart() {
    let mut room = Room::new(1);
    hello(&mut room, HOST, "player", false);
    let out = hello(&mut room, OPP, "player", false);
    let players = to(&out, HOST).into_iter().find_map(|m| match m {
        FromRoom::Lobby { players, .. } => Some(players),
        _ => None,
    });
    let [a, b] = players.unwrap().map(Option::unwrap);
    assert_eq!((a.name.as_str(), b.name.as_str()), ("PLAYER", "PLAYER"), "names as given");
    assert_ne!(a.id, b.id);
}
