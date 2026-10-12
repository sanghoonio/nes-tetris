//! The list of rooms from a local room server: public rooms are listed, a private one isn't, a
//! room's state follows its match, and an emptied room leaves the list.

use std::time::{Duration, Instant};

use netplay::{Link, RoomList};
use protocol::{RoomState, RoomSummary, Settings, ToRoom};

fn server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || netplay::local::serve(listener));
    format!("ws://127.0.0.1:{port}")
}

fn join(server: &str, code: &str, name: &str) -> Link {
    let link = Link::open(&format!("{server}/room/{code}"));
    link.send(ToRoom::Hello { name: name.into(), watch: false });
    link
}

/// Wait for the list to satisfy `done`.
fn until(list: &mut RoomList, what: &str, done: impl Fn(&[RoomSummary]) -> bool) -> Vec<RoomSummary> {
    let start = Instant::now();
    loop {
        if let Some(Ok(rooms)) = list.latest() {
            if done(rooms) {
                return rooms.clone();
            }
        }
        assert!(start.elapsed() < Duration::from_secs(5), "waiting for {what}: {:?}", list.latest());
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_list_follows_the_rooms() {
    let server = server();
    let mut list = RoomList::open(&server);
    until(&mut list, "an empty list", |r| r.is_empty());

    let ana = join(&server, "AAAA", "ana");
    until(&mut list, "AAAA, open", |r| r.len() == 1 && r[0].state == RoomState::Open);
    let bo = join(&server, "AAAA", "bo");
    let rooms = until(&mut list, "AAAA, full", |r| r.len() == 1 && r[0].state == RoomState::Full);
    assert_eq!((rooms[0].host.as_str(), rooms[0].players), ("ANA", 2));

    let cy = join(&server, "BBBB", "cy");
    until(&mut list, "BBBB, newest first", |r| r.len() == 2 && r[0].code == "BBBB");

    // A private room isn't listed: DEE hosts CCCC and makes it private straight away.
    let dee = join(&server, "CCCC", "dee");
    dee.send(ToRoom::Settings { settings: Settings { public: false, ..Settings::default() } });
    std::thread::sleep(Duration::from_millis(200));
    until(&mut list, "still two", |r| r.len() == 2 && r.iter().all(|s| s.code != "CCCC"));

    // AAAA's match starts.
    ana.send(ToRoom::Ready { ready: true });
    bo.send(ToRoom::Ready { ready: true });
    until(&mut list, "AAAA playing", |r| r.iter().any(|s| s.code == "AAAA" && s.state == RoomState::Playing));

    // BBBB empties.
    drop(cy);
    until(&mut list, "BBBB gone", |r| r.len() == 1 && r[0].code == "AAAA");
    drop((ana, bo, dee));
}
