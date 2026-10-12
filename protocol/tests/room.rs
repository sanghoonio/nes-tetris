//! The room's rules: seats and spectators, the host's settings, readying and starting, passing on
//! inputs, the CTWC ending, disconnects and forfeits, late spectators, and confirming results.

use protocol::limits::{self, MAX_PENDING, MAX_SPECTATORS};
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
    msg(room, conn, ToRoom::Inputs { frame, buttons: vec![0; n], score, over, seat: None })
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
fn spectators_are_limited() {
    let mut room = Room::new(1);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, OPP, "bo", false);
    for c in 0..MAX_SPECTATORS as ConnId {
        assert_eq!(to(&hello(&mut room, 100 + c, "cy", true), 100 + c)[0], FromRoom::Welcome { seat: None });
    }
    let out = hello(&mut room, 99, "dee", true);
    assert_eq!(to(&out, 99), [FromRoom::Closed { reason: limits::FULL.into() }]);
    assert!(to(&out, HOST).is_empty());
    // One leaves: room again.
    room.handle(Event::Left(100));
    assert_eq!(to(&hello(&mut room, 98, "eve", true), 98)[0], FromRoom::Welcome { seat: None });
}

#[test]
fn connections_that_never_say_hello_make_way() {
    let mut room = Room::new(1);
    for c in 0..MAX_PENDING as ConnId {
        assert!(room.handle(Event::Joined(10 + c)).is_empty());
    }
    // The next closes the oldest, which can no longer say Hello.
    let out = room.handle(Event::Joined(20));
    assert_eq!(to(&out, 10), [FromRoom::Closed { reason: limits::NO_HELLO.into() }]);
    let out = msg(&mut room, 10, ToRoom::Hello { name: "ana".into(), watch: false });
    assert!(matches!(to(&out, 10)[0], FromRoom::Error { .. }));
    assert_eq!(to(&msg(&mut room, 20, ToRoom::Hello { name: "bo".into(), watch: false }), 20)[0], FromRoom::Welcome { seat: Some(0) });
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
    let out = msg(&mut room, OPP, ToRoom::Checksum { frame: 60, hash: 99, seat: None });
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
    // Passing decides it: everyone's told, and the winner plays on.
    let out = inputs(&mut room, OPP, 15, 15, 1300, false);
    assert_eq!(result(&out, WATCHER), None, "the winner plays on");
    assert!(to(&out, WATCHER).contains(&FromRoom::Decided { match_id: 1, outcome: Outcome::Win { seat: 1 } }));
    assert!(room.playing());
    inputs(&mut room, OPP, 30, 15, 2500, false);
    // Over when the winner tops out, with their final score; the outcome stands.
    let out = inputs(&mut room, OPP, 45, 5, 2600, true);
    let (outcome, scores, frames) = result(&out, WATCHER).expect("over");
    assert_eq!(outcome, Outcome::Win { seat: 1 });
    assert_eq!(scores, [1200, 2600]);
    assert_eq!(frames, [25, 50]);
    assert!(!room.playing());
}

/// Playing on after winning, the winner leaving or resigning ends it, and isn't a forfeit.
#[test]
fn the_winner_stops_playing_on() {
    for stop in [Event::Left(OPP), Event::Message(OPP, ToRoom::Resign)] {
        let (mut room, _) = playing(Pieces::Separate);
        inputs(&mut room, HOST, 0, 15, 100, true);
        inputs(&mut room, OPP, 0, 15, 200, false);
        let out = room.handle(stop);
        assert_eq!(result(&out, WATCHER).expect("over").0, Outcome::Win { seat: 1 });
    }
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
    let out = inputs(&mut room, HOST, 0, 15, 5100, true);
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
        inputs(&mut room, OPP, 0, 15, 500, true);
        assert!(msg(&mut room, HOST, ToRoom::Report { match_id: 1, scores: [300, 500] }).is_empty());
        let out = msg(&mut room, OPP, ToRoom::Report { match_id: 1, scores: opp_report });
        assert_eq!(to(&out, WATCHER), vec![FromRoom::Confirmed { match_id: 1, agreed }]);
    }
}

#[test]
fn late_inputs_after_a_result_are_ignored() {
    let (mut room, _) = playing(Pieces::Separate);
    inputs(&mut room, HOST, 0, 15, 100, true);
    inputs(&mut room, OPP, 0, 15, 200, true);
    assert!(!room.playing());
    assert!(inputs(&mut room, OPP, 15, 15, 240, false).is_empty());
}

#[test]
fn rematch_after_a_result() {
    let (mut room, _) = playing(Pieces::Separate);
    inputs(&mut room, HOST, 0, 15, 100, true);
    inputs(&mut room, OPP, 0, 15, 200, true);
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
    let m = ToRoom::Inputs { frame: 15, buttons: vec![0, 4, 4], score: 120, over: false, seat: None };
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

fn lobby_players(out: &[Send], conn: ConnId) -> [Option<protocol::Player>; 2] {
    to(out, conn).into_iter().rev().find_map(|m| match m {
        FromRoom::Lobby { players, .. } => Some(players),
        _ => None,
    }).expect("a lobby")
}

/// The host adds a bot: it takes the free seat, ready; the match starts when the host readies;
/// the host sends its inputs, nobody else can; it wins or loses like anyone.
#[test]
fn a_bot_plays_for_the_host() {
    let mut room = Room::new(3);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, WATCHER, "cy", true);
    assert!(to(&msg(&mut room, WATCHER, ToRoom::AddBot { name: "pixel".into() }), WATCHER).iter().any(|m| matches!(m, FromRoom::Error { .. })), "only the host");
    let out = msg(&mut room, HOST, ToRoom::AddBot { name: "pixel".into() });
    let [_, bot] = lobby_players(&out, WATCHER);
    let bot = bot.expect("seated");
    assert!(bot.bot && bot.ready && bot.name == "PIXEL");
    // Nobody else can sit; a newcomer watches.
    assert_eq!(to(&hello(&mut room, OPP, "bo", false), OPP)[0], FromRoom::Welcome { seat: None });
    let out = msg(&mut room, HOST, ToRoom::Ready { ready: true });
    assert!(to(&out, WATCHER).iter().any(|m| matches!(m, FromRoom::Start { .. })));
    // The host plays both seats; the bot's seat only with `seat`. A watcher can't.
    let bot_inputs = |over| ToRoom::Inputs { frame: 0, buttons: vec![0; 15], score: 100, over, seat: Some(1) };
    assert!(to(&msg(&mut room, OPP, bot_inputs(false)), OPP).iter().any(|m| matches!(m, FromRoom::Error { .. })));
    let out = msg(&mut room, HOST, bot_inputs(true));
    assert!(to(&out, WATCHER).contains(&FromRoom::Inputs { seat: 1, frame: 0, buttons: vec![0; 15], score: 100, over: true }));
    assert!(to(&out, HOST).is_empty(), "not back to the host");
    // The host passes the bot's 100, and tops out after playing on: the host wins.
    let out = inputs(&mut room, HOST, 0, 15, 200, false);
    assert_eq!(result(&out, WATCHER), None, "playing on");
    let out = inputs(&mut room, HOST, 15, 15, 300, true);
    let (outcome, scores, _) = result(&out, WATCHER).expect("a result");
    assert_eq!((outcome, scores), (Outcome::Win { seat: 0 }, [300, 100]));
    // The bot stays seated and ready for a rematch.
    assert!(lobby_players(&out, WATCHER)[1].as_ref().unwrap().ready);
    // Only the host's report is waited for.
    let out = msg(&mut room, HOST, ToRoom::Report { match_id: 1, scores: [300, 100] });
    assert!(to(&out, WATCHER).contains(&FromRoom::Confirmed { match_id: 1, agreed: true }));
    // The host kicks it.
    let out = msg(&mut room, HOST, ToRoom::Kick);
    assert!(lobby_players(&out, WATCHER)[1].is_none());
    assert!(to(&out, HOST).iter().all(|m| !matches!(m, FromRoom::Kicked)));
}

/// The bot leaves with its host: between matches the room empties of players; during a match
/// both seats are gone and the match is abandoned.
#[test]
fn a_bot_leaves_with_its_host() {
    let mut room = Room::new(3);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, WATCHER, "cy", true);
    msg(&mut room, HOST, ToRoom::AddBot { name: "byte".into() });
    let out = room.handle(Event::Left(HOST));
    assert_eq!(lobby_players(&out, WATCHER), [None, None]);

    let mut room = Room::new(3);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, WATCHER, "cy", true);
    msg(&mut room, HOST, ToRoom::AddBot { name: "byte".into() });
    msg(&mut room, HOST, ToRoom::Ready { ready: true });
    assert!(room.playing());
    let out = room.handle(Event::Left(HOST));
    assert!(to(&out, WATCHER).iter().any(|m| matches!(m, FromRoom::Abandoned { .. })));
    assert!(!room.playing());
}

/// Messages go to each connection once, though the host also plays the bot.
#[test]
fn the_host_hears_once() {
    let mut room = Room::new(3);
    hello(&mut room, HOST, "ana", false);
    let out = msg(&mut room, HOST, ToRoom::AddBot { name: "byte".into() });
    assert_eq!(to(&out, HOST).len(), 1);
}

/// Resigning: a forfeit, and the resigner stays seated.
#[test]
fn resigning_stays() {
    let mut room = Room::new(3);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, WATCHER, "cy", true);
    msg(&mut room, HOST, ToRoom::AddBot { name: "sleve".into() });
    msg(&mut room, HOST, ToRoom::Ready { ready: true });
    let out = msg(&mut room, HOST, ToRoom::Resign);
    let (outcome, _, _) = result(&out, WATCHER).expect("a result");
    assert_eq!(outcome, Outcome::Forfeit { seat: 0 });
    let [host, bot] = lobby_players(&out, WATCHER);
    assert!(host.is_some() && bot.is_some(), "both still seated");
    assert!(!room.playing());
}

/// A bot plays on after winning, like anyone; the host can end it by resigning (the bot's win
/// stands).
#[test]
fn a_bot_plays_on() {
    let mut room = Room::new(3);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, WATCHER, "cy", true);
    msg(&mut room, HOST, ToRoom::AddBot { name: "sleve".into() });
    msg(&mut room, HOST, ToRoom::Ready { ready: true });
    inputs(&mut room, HOST, 0, 15, 100, true);
    let out = msg(&mut room, HOST, ToRoom::Inputs { frame: 0, buttons: vec![0; 15], score: 200, over: false, seat: Some(1) });
    assert_eq!(result(&out, WATCHER), None, "the bot plays on");
    assert!(to(&out, WATCHER).contains(&FromRoom::Decided { match_id: 1, outcome: Outcome::Win { seat: 1 } }));
    let out = msg(&mut room, HOST, ToRoom::Resign);
    assert_eq!(result(&out, WATCHER).expect("over").0, Outcome::Win { seat: 1 });
}

/// Pausing: only the host playing its own bot, during the match; spectators are told.
#[test]
fn pausing_a_bot_match() {
    let (mut room, _) = playing(Pieces::Separate);
    let out = msg(&mut room, HOST, ToRoom::Pause { paused: true });
    assert!(to(&out, HOST).iter().any(|m| matches!(m, FromRoom::Error { .. })), "not against a person");
    assert!(to(&out, WATCHER).is_empty());

    let mut room = Room::new(3);
    hello(&mut room, HOST, "ana", false);
    hello(&mut room, WATCHER, "cy", true);
    msg(&mut room, HOST, ToRoom::AddBot { name: "kevin".into() });
    msg(&mut room, HOST, ToRoom::Ready { ready: true });
    let out = msg(&mut room, HOST, ToRoom::Pause { paused: true });
    assert_eq!(to(&out, WATCHER), vec![FromRoom::Paused { paused: true }]);
    assert!(to(&out, HOST).is_empty());
}
