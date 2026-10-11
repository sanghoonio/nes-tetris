//! A room server on this machine: the same `protocol::Room` as the Cloudflare Worker, over plain
//! websockets, one room per code, a thread per connection. For trying matches without Cloudflare,
//! and for tests.

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use protocol::{ConnId, Event, FromRoom, Room, ToRoom};
use tungstenite::Message;

/// A room and the outgoing queue of each connection in it.
struct Hosted {
    room: Room,
    conns: HashMap<ConnId, Sender<FromRoom>>,
}

#[derive(Clone)]
struct Rooms {
    start: Instant,
    rooms: Arc<Mutex<HashMap<String, Hosted>>>,
}

impl Rooms {
    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn run(&self, code: &str, event: Event) {
        let mut rooms = self.rooms.lock().unwrap();
        let hosted = rooms.get_mut(code).expect("a room");
        for send in hosted.room.handle(event) {
            if let Some(tx) = hosted.conns.get(&send.to) {
                let _ = tx.send(send.msg);
            }
        }
        if hosted.room.empty() {
            rooms.remove(code);
        }
    }
}

/// Serve rooms on `listener` until the process ends.
pub fn serve(listener: TcpListener) {
    let rooms = Rooms { start: Instant::now(), rooms: Arc::new(Mutex::new(HashMap::new())) };
    let mut next_conn: ConnId = 1;
    for stream in listener.incoming().flatten() {
        let rooms = rooms.clone();
        let conn = next_conn;
        next_conn += 1;
        std::thread::spawn(move || connection(rooms, conn, stream));
    }
}

fn connection(rooms: Rooms, conn: ConnId, stream: TcpStream) {
    let mut code = String::new();
    let callback = |req: &tungstenite::handshake::server::Request, res| {
        code = req.uri().path().trim_start_matches("/room/").to_uppercase();
        Ok(res)
    };
    let Ok(mut ws) = tungstenite::accept_hdr(stream, callback) else { return };
    if !protocol::valid_code(&code) {
        let _ = ws.close(None);
        return;
    }
    let (tx, outgoing) = channel();
    {
        let mut all = rooms.rooms.lock().unwrap();
        let hosted = all.entry(code.clone()).or_insert_with(|| Hosted { room: Room::new(rooms.now() ^ 0x9E37_79B9), conns: HashMap::new() });
        hosted.conns.insert(conn, tx);
    }
    rooms.run(&code, Event::Joined(conn));
    ws.get_mut().set_nonblocking(true).expect("non-blocking socket");
    loop {
        let mut idle = true;
        while let Ok(msg) = outgoing.try_recv() {
            let _ = ws.send(Message::text(msg.to_json()));
            idle = false;
        }
        let _ = ws.flush();
        match ws.read() {
            Ok(Message::Text(text)) => {
                match ToRoom::from_json(&text) {
                    Some(msg) => rooms.run(&code, Event::Message(conn, msg)),
                    None => {
                        let _ = ws.send(Message::text(FromRoom::Error { message: "unreadable message".into() }.to_json()));
                    }
                }
                idle = false;
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        if idle {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    if let Some(hosted) = rooms.rooms.lock().unwrap().get_mut(&code) {
        hosted.conns.remove(&conn);
    }
    rooms.run(&code, Event::Left(conn));
}
