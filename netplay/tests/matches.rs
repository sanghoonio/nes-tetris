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
    /// The longest a batch of this client's waited for the room to acknowledge it.
    max_unacked: u64,
    /// Whether this client was ever waiting for each seat's inputs.
    waited: [bool; 2],
    /// Frames its own game had played, 10 frames after the opponent's trouble ended.
    frames_after: Option<u32>,
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

fn play(settings: Settings, seed: u64, late_join: u64, drop_opp: Option<u64>) -> Vec<Client> {
    play_with(settings, seed, Trouble { late_join, drop_opp, ..Trouble::default() })
}

/// What goes wrong in a match, in frames from its start.
#[derive(Clone, Copy, Default)]
struct Trouble {
    /// Another watcher joins.
    late_join: u64,
    /// The opponent's connection drops.
    drop_opp: Option<u64>,
    /// The opponent's program stops running (a hidden tab) from the first, for the second.
    hide_opp: Option<(u64, u64)>,
    /// Nothing gets to or from the opponent from the first, for the second, then it all arrives
    /// (a network that stalls and comes back).
    cut_opp: Option<(u64, u64)>,
}

/// Host, opponent and a watcher from the start, with `trouble`. The room runs on the same clock as
/// the clients. Runs until every client has the confirmed result. Returns the clients still
/// connected.
fn play_with(settings: Settings, seed: u64, trouble: Trouble) -> Vec<Client> {
    let Trouble { late_join, drop_opp, hide_opp, cut_opp } = trouble;
    let rom = rom();
    let mut room = Room::new(seed);
    let mut clients: Vec<Client> = vec![];
    let add = |clients: &mut Vec<Client>, room: &mut Room, conn, name: &str, watch, latency| {
        room.handle(0, Event::Joined(conn));
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
            max_unacked: 0,
            waited: [false; 2],
            frames_after: None,
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
                from_room.extend(room.handle(now, Event::Left(2)));
            }
        }
        let since = |w: Option<(u64, u64)>| match (started, w) {
            (Some(s), Some((from, len))) => Some((s + from, s + from + len)),
            _ => None,
        };
        let cut = since(cut_opp).is_some_and(|(a, b)| (a..b).contains(&f));
        let hidden = since(hide_opp).is_some_and(|(a, b)| (a..b).contains(&f));
        let trouble_over = since(hide_opp).or(since(cut_opp)).map(|(_, b)| b + 10);
        // Deliver what's due (nothing to or from the opponent while cut off).
        let held = |to: &To| cut && matches!(to, To::Room(2, _) | To::Client(2, _));
        let (due, later): (Vec<_>, Vec<_>) = queue.into_iter().partition(|(at, to)| *at <= now && !held(to));
        queue = later;
        for (_, to) in due {
            match to {
                // A dropped connection's messages in flight are lost.
                To::Room(conn, _) if drop_opp.is_some() && conn == 2 && !clients.iter().any(|c| c.conn == 2) => {}
                To::Room(conn, msg) => from_room.extend(room.handle(now, Event::Message(conn, msg))),
                To::Client(conn, msg) => {
                    if let Some(c) = clients.iter_mut().find(|c| c.conn == conn) {
                        c.session.receive(now, msg);
                    }
                }
            }
        }
        from_room.extend(room.handle(now, Event::Tick));
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
            if hidden && c.conn == 2 {
                continue;
            }
            c.session.tick(now, b);
            c.notes.extend(c.session.take_notes());
            for msg in c.session.take_outbox() {
                queue.push((now + c.latency, To::Room(c.conn, msg)));
            }
            c.max_unacked = c.max_unacked.max(c.session.unacknowledged_ms(now));
            let waiting = c.session.waiting(now);
            c.waited = [0, 1].map(|s| c.waited[s] || waiting[s]);
            if trouble_over == Some(f) {
                c.frames_after = c.session.frames_played();
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

/// The host plays a bot it seated: its session runs the bot's game beside its own and sends the
/// bot's inputs; a watcher's replica of the bot's game checks out against the bot's checksums;
/// the result is confirmed by the host's report alone.
#[test]
fn a_match_against_a_bot() {
    let rom = rom();
    let mut room = Room::new(41);
    let mut host = Session::new(rom.clone(), None, "ana", false);
    let mut watcher = Session::new(rom.clone(), None, "cy", true);
    let (host_conn, watcher_conn) = (1, 2);
    room.handle(0, Event::Joined(host_conn));
    room.handle(0, Event::Joined(watcher_conn));
    let mut rng = Xorshift(5);
    let mut held = 0u8;
    let mut added = false;
    let mut readied = false;
    let mut notes = vec![];
    for f in 0..200_000u64 {
        let now = f * FRAME_US / 1000;
        // Messages go straight through (no latency): the room answers each in turn.
        let mut to_room: Vec<(ConnId, ToRoom)> = vec![];
        if f.is_multiple_of(8) {
            held = [0, 0, 0x04, 0x02, 0x01, 0x80, 0x40][(rng.next() % 7) as usize];
        }
        host.tick(now, held);
        watcher.tick(now, 0);
        if host.seat() == Some(0) && !added && *host.phase() == Phase::Lobby {
            host.add_bot("JEROMY");
            added = true;
        }
        if added && !readied && host.players()[1].as_ref().is_some_and(|p| p.bot) {
            host.set_ready(true);
            readied = true;
        }
        to_room.extend(host.take_outbox().into_iter().map(|m| (host_conn, m)));
        to_room.extend(watcher.take_outbox().into_iter().map(|m| (watcher_conn, m)));
        for (conn, msg) in to_room {
            for send in room.handle(0, Event::Message(conn, msg)) {
                match send.to {
                    c if c == host_conn => host.receive(now, send.msg),
                    _ => watcher.receive(now, send.msg),
                }
            }
        }
        notes.extend(host.take_notes());
        notes.extend(watcher.take_notes());
        if let Phase::Over { outcome, scores, confirmed: Some(agreed) } = watcher.phase().clone() {
            println!("against JEROMY: {outcome:?} {scores:?}, the watcher's checks {:?}", watcher.verified());
            assert!(agreed, "the host's report agrees");
            assert!(notes.iter().all(|n| !matches!(n, Note::Desync { .. } | Note::Error(_))), "{notes:?}");
            assert!(watcher.verified()[1] > 0, "the bot's game, replicated, checked out");
            assert!(scores[1] > 0, "the bot scored");
            return;
        }
    }
    panic!("no confirmed result");
}

fn forfeited(clients: &[Client], seat: u8) {
    for c in clients {
        let Phase::Over { outcome, confirmed, .. } = c.session.phase().clone() else { panic!("client {}", c.conn) };
        assert_eq!(outcome, Outcome::Forfeit { seat }, "client {}", c.conn);
        assert_eq!(confirmed, Some(true), "client {}", c.conn);
    }
}

/// The opponent's tab is hidden for 5 s: their game stops, then plays the frames it owes with no
/// buttons and is back on the clock. The host waited for them meanwhile; nobody forfeits, and
/// every replica checks out.
#[test]
fn a_hidden_tab_catches_up_with_the_clock() {
    let settings = Settings { level: 0, ..Settings::default() };
    let clients = play_with(settings, 31, Trouble { late_join: 300, hide_opp: Some((600, 300)), ..Trouble::default() });
    check(&clients);
    let [host, opp] = [0, 1].map(|i| clients[i].frames_after.expect("playing"));
    println!("10 frames after: host {host}, opponent {opp}");
    assert!(host.abs_diff(opp) <= 8, "back on the clock: host {host}, opponent {opp}");
    assert!(clients[0].waited[1], "the host waited for the opponent");
}

/// Hidden for 15 s: the room forfeits the opponent once they're 10 s behind, and the result
/// doesn't wait on their report.
#[test]
fn a_tab_hidden_too_long_forfeits() {
    let settings = Settings { level: 0, ..Settings::default() };
    let clients = play_with(settings, 37, Trouble { late_join: 300, hide_opp: Some((300, 900)), ..Trouble::default() });
    forfeited(&clients, 1);
}

/// Nothing gets through to or from the opponent for 4 s, then it all does: their batches go
/// unacknowledged for that long but not for the grace period, the host waits for them, and the
/// match carries on with every replica in step.
#[test]
fn a_network_stall_within_the_grace_period_is_waited_out() {
    let settings = Settings { level: 0, ..Settings::default() };
    let clients = play_with(settings, 43, Trouble { late_join: 300, cut_opp: Some((600, 240)), ..Trouble::default() });
    check(&clients);
    let unacked = clients[1].max_unacked;
    println!("longest unacknowledged: {unacked} ms");
    assert!((3_000..protocol::GRACE_MS).contains(&unacked), "a stall, not a lost connection: {unacked} ms");
    assert!(clients[0].waited[1], "the host waited for the opponent");
}

/// Cut off for 12 s: the room forfeits the opponent at 10 s behind, and their batches went
/// unacknowledged long enough for the app to give the connection up.
#[test]
fn a_network_stall_past_the_grace_period_forfeits() {
    let settings = Settings { level: 0, ..Settings::default() };
    let clients = play_with(settings, 47, Trouble { late_join: 300, cut_opp: Some((300, 720)), ..Trouble::default() });
    forfeited(&clients, 1);
    assert!(clients[1].max_unacked >= protocol::GRACE_MS, "{} ms", clients[1].max_unacked);
}
