//! Whole matches with no network: a room and its clients in one process, messages delivered after
//! each client's own latency. Every replica must match the game it copies (all checksums equal,
//! none missing), and the result must be confirmed by both players' reports.

use netplay::{Note, Phase, Session};
use protocol::{ConnId, Event, FromRoom, Outcome, Pieces, Room, Settings, ToRoom};
use std::sync::Arc;
use tetris_rs::Rom;

/// One frame, in microseconds.
const FRAME_US: u64 = 16_639;

fn rom() -> Arc<Rom> {
    let path = std::env::var_os("TETRIS_ROM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"));
    let file = std::fs::read(&path).unwrap_or_else(|e| panic!("ROM {}: {e}", path.display()));
    Arc::new(Rom::new(&file).unwrap())
}

struct Xorshift(u64);
impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

struct Client {
    conn: ConnId,
    session: Session,
    /// One-way latency, ms.
    latency: u64,
    rng: Xorshift,
    held: u8,
    readied: bool,
    notes: Vec<Note>,
}

impl Client {
    /// Random play: a new choice every 8 frames (a move, a turn, a soft drop, or nothing).
    fn buttons(&mut self, f: u64) -> u8 {
        if f.is_multiple_of(8) {
            let r = self.rng.next();
            self.held = [0, 0, 0x04, 0x02, 0x01, 0x80, 0x40, 0x02, 0x01][(r % 9) as usize];
        }
        self.held
    }
}

enum To {
    Room(ConnId, ToRoom),
    Client(ConnId, FromRoom),
}

/// Host, opponent and a watcher from the start; another watcher joins `late_join` frames into the
/// match, and the opponent's connection drops `drop_opp` frames in if given.
/// Runs until every client has the confirmed result. Returns the clients still connected.
fn play(settings: Settings, seed: u64, late_join: u64, drop_opp: Option<u64>) -> Vec<Client> {
    let rom = rom();
    let mut room = Room::new(seed);
    let mut clients: Vec<Client> = vec![];
    let add = |clients: &mut Vec<Client>, room: &mut Room, conn, name: &str, watch, latency| {
        room.handle(Event::Joined(conn));
        let mut session = Session::new(rom.clone(), None, name, watch);
        // Each player their own music (the replicas play music 1): the checksums leave it out.
        session.set_music(conn as u8 % 4);
        clients.push(Client {
            conn,
            session,
            latency,
            rng: Xorshift(seed * 31 + conn),
            held: 0,
            readied: false,
            notes: vec![],
        });
    };
    add(&mut clients, &mut room, 1, "host", false, 30);
    add(&mut clients, &mut room, 2, "opp", false, 80);
    add(&mut clients, &mut room, 3, "watch", true, 50);
    let mut queue: Vec<(u64, To)> = vec![];
    let mut set = false;
    let mut started = None;
    for f in 0..200_000u64 {
        let now = f * FRAME_US / 1000;
        if started.is_none() && *clients[0].session.phase() == Phase::Playing {
            started = Some(f);
        }
        if started.is_some_and(|s| f == s + late_join) {
            add(&mut clients, &mut room, 4, "late", true, 120);
        }
        let mut from_room = vec![];
        if let (Some(s), Some(d)) = (started, drop_opp) {
            if f == s + d {
                clients.retain(|c| c.conn != 2);
                from_room.extend(room.handle(Event::Left(2)));
            }
        }
        // Deliver what's due.
        let (due, later): (Vec<_>, Vec<_>) = queue.into_iter().partition(|(at, _)| *at <= now);
        queue = later;
        for (_, to) in due {
            match to {
                // A dropped connection's messages in flight are lost.
                To::Room(conn, _) if drop_opp.is_some() && conn == 2 && !clients.iter().any(|c| c.conn == 2) => {}
                To::Room(conn, msg) => from_room.extend(room.handle(Event::Message(conn, msg))),
                To::Client(conn, msg) => {
                    if let Some(c) = clients.iter_mut().find(|c| c.conn == conn) {
                        c.session.receive(now, msg);
                    }
                }
            }
        }
        for send in from_room {
            if let Some(c) = clients.iter().find(|c| c.conn == send.to) {
                queue.push((now + c.latency, To::Client(send.to, send.msg)));
            }
        }
        // Each client's frame.
        for c in clients.iter_mut() {
            let in_lobby = *c.session.phase() == Phase::Lobby || matches!(c.session.phase(), Phase::Over { .. });
            if c.session.seat() == Some(0) && !set && in_lobby {
                c.session.set_settings(settings);
                set = true;
            }
            let both_here = c.session.players().iter().all(|p| p.is_some());
            if c.session.seat().is_some() && !c.readied && in_lobby && both_here && set {
                c.session.set_ready(true);
                c.readied = true;
            }
            let b = c.buttons(f);
            c.session.tick(now, b);
            c.notes.extend(c.session.take_notes());
            for msg in c.session.take_outbox() {
                queue.push((now + c.latency, To::Room(c.conn, msg)));
            }
        }
        let expected = if drop_opp.is_some() { 3 } else { 4 };
        if clients.len() == expected && clients.iter().all(|c| matches!(c.session.phase(), Phase::Over { confirmed: Some(_), .. })) {
            return clients;
        }
    }
    panic!("no confirmed result");
}

fn check(clients: &[Client]) -> (Outcome, [u32; 2]) {
    let Phase::Over { outcome, scores, confirmed } = clients[0].session.phase().clone() else { panic!() };
    assert_eq!(confirmed, Some(true), "the reports agree with the scores");
    for c in clients {
        assert_eq!(c.session.phase(), &Phase::Over { outcome, scores, confirmed: Some(true) }, "client {}", c.conn);
        assert!(c.notes.iter().all(|n| !matches!(n, Note::Desync { .. } | Note::Error(_))), "client {}: {:?}", c.conn, c.notes);
    }
    // Every replica checked itself against the game it copies.
    let verified = |conn: usize| clients[conn].session.verified();
    assert!(verified(0)[1] > 0 && verified(1)[0] > 0, "players' replicas");
    for w in [2, 3] {
        assert!(verified(w)[0] > 0 && verified(w)[1] > 0, "watcher {w}'s replicas: {:?}", verified(w));
    }
    // CTWC: the winner has the higher score.
    match outcome {
        Outcome::Win { seat } => assert!(scores[seat as usize] > scores[1 - seat as usize]),
        Outcome::Draw => assert_eq!(scores[0], scores[1]),
        Outcome::Forfeit { .. } => panic!("nobody left"),
    }
    (outcome, scores)
}

#[test]
fn a_match_separate_pieces() {
    let settings = Settings { level: 5, pieces: Pieces::Separate, ..Settings::default() };
    let clients = play(settings, 5, 600, None);
    let (outcome, scores) = check(&clients);
    println!("separate: {outcome:?} {scores:?}, checks {:?}", clients.iter().map(|c| c.session.verified()).collect::<Vec<_>>());
}

#[test]
fn a_match_identical_pieces() {
    let settings = Settings { level: 0, pieces: Pieces::Identical, ..Settings::default() };
    let clients = play(settings, 11, 1500, None);
    let (outcome, scores) = check(&clients);
    println!("identical: {outcome:?} {scores:?}, checks {:?}", clients.iter().map(|c| c.session.verified()).collect::<Vec<_>>());
}

/// The opponent's connection drops 20 s in: they forfeit at once. Only the host reports, and the
/// watchers' replicas of the opponent's game checked out up to then.
#[test]
fn a_dropped_player_forfeits() {
    let settings = Settings { level: 0, ..Settings::default() };
    let clients = play(settings, 23, 300, Some(1200));
    let Phase::Over { outcome, confirmed, .. } = clients[0].session.phase().clone() else { panic!() };
    assert_eq!(outcome, Outcome::Forfeit { seat: 1 });
    assert_eq!(confirmed, Some(true));
    for c in &clients {
        assert!(c.notes.contains(&Note::Gone(1)), "client {} saw the opponent go", c.conn);
        assert!(c.notes.iter().all(|n| !matches!(n, Note::Desync { .. })));
    }
    assert!(clients[1].session.verified()[1] > 0);
}
