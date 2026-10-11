//! A whole match over real websockets: the local room server on a free port, and a host, an
//! opponent and a watcher, each a `Session` on the probe's `Link`, headless and faster than real
//! time. The result must be confirmed and every replica must have matched its checksums.

use std::sync::Arc;
use std::time::{Duration, Instant};

use netplay::{Note, Phase, Session};
use probe::Link;
use protocol::{Pieces, Settings};
use tetris_rs::Rom;

fn rom() -> Arc<Rom> {
    let path = std::env::var_os("TETRIS_ROM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"));
    Arc::new(Rom::new(&std::fs::read(path).expect("ROM")).unwrap())
}

struct Client {
    session: Session,
    link: Link,
    rng: u64,
    held: u8,
    readied: bool,
    notes: Vec<Note>,
}

#[test]
fn a_match_over_websockets() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || probe::local::serve(listener));
    let url = format!("ws://127.0.0.1:{port}/room/TEST");
    let rom = rom();
    let mut clients: Vec<Client> = [("ana", false), ("bo", false), ("cy", true)]
        .into_iter()
        .enumerate()
        .map(|(i, (name, watch))| {
            let c = Client {
                session: Session::new(rom.clone(), None, name, watch),
                link: Link::connect(&url),
                rng: 0x2545_F491 + i as u64,
                held: 0,
                readied: false,
                notes: vec![],
            };
            // Seat order is arrival order.
            std::thread::sleep(Duration::from_millis(50));
            c
        })
        .collect();
    let settings = Settings { level: 9, pieces: Pieces::Identical, ..Settings::default() };
    let start = Instant::now();
    let mut frame = 0u64;
    let mut set = false;
    loop {
        let now = start.elapsed().as_millis() as u64;
        assert!(now < 120_000, "no confirmed result in time: {:?}", clients.iter().map(|c| c.session.phase().clone()).collect::<Vec<_>>());
        for c in clients.iter_mut() {
            while let Ok(msg) = c.link.rx.try_recv() {
                c.session.receive(now, msg.expect("connection stays open"));
            }
            if c.session.seat() == Some(0) && !set && *c.session.phase() == Phase::Lobby {
                c.session.set_settings(settings);
                set = true;
            }
            let both = c.session.players().iter().all(|p| p.is_some());
            if c.session.seat().is_some() && set && both && !c.readied && c.session.settings() == settings {
                c.session.set_ready(true);
                c.readied = true;
            }
            if frame.is_multiple_of(8) {
                c.rng ^= c.rng << 13;
                c.rng ^= c.rng >> 7;
                c.rng ^= c.rng << 17;
                c.held = [0, 0, 0x04, 0x02, 0x01, 0x80, 0x40, 0x02, 0x01][(c.rng % 9) as usize];
            }
            c.session.tick(now, c.held);
            c.notes.extend(c.session.take_notes());
            for msg in c.session.take_outbox() {
                c.link.tx.send(msg).unwrap();
            }
        }
        frame += 1;
        if clients.iter().all(|c| matches!(c.session.phase(), Phase::Over { confirmed: Some(_), .. })) {
            break;
        }
        // Faster than real time, but leave the network threads room.
        std::thread::sleep(Duration::from_micros(500));
    }
    let Phase::Over { outcome, scores, confirmed } = clients[0].session.phase().clone() else { unreachable!() };
    println!("{outcome:?} {scores:?} after {frame} frames, {:?}", start.elapsed());
    assert_eq!(confirmed, Some(true));
    for c in &clients {
        assert!(c.notes.iter().all(|n| !matches!(n, Note::Desync { .. } | Note::Error(_))), "{:?}", c.notes);
        assert_eq!(c.session.phase(), &Phase::Over { outcome, scores, confirmed: Some(true) });
    }
    // Each player's replica of the other seat, and the watcher's of both. (Seats go by whose
    // hello reaches the room first.)
    for c in &clients[..2] {
        let other = 1 - c.session.seat().expect("a seat") as usize;
        assert!(c.session.verified()[other] > 0, "{:?}", c.session.verified());
    }
    assert!(clients[2].session.verified().iter().all(|&n| n > 0));
}
