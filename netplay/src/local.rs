//! A room server on this machine: the same `protocol::Room` as the Cloudflare Worker, over plain
//! websockets, one room per code, a thread per connection, and the list of public rooms
//! (`protocol::Directory`) at `/rooms`. For trying matches without Cloudflare, and for tests.
//! Pings are answered, but silence isn't timed: a connection gone here is seen closing at once.

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use protocol::directory::Report;
use protocol::limits::{self, MAX_MESSAGE_BYTES, MAX_WATCHERS};
use protocol::{ConnId, Directory, Event, FromDirectory, FromRoom, Room, ToRoom};
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
    /// The list, and the connections watching it.
    directory: Arc<Mutex<(Directory, Vec<Sender<FromDirectory>>)>>,
}

impl Rooms {
    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn run(&self, code: &str, event: Event) {
        let mut rooms = self.rooms.lock().unwrap();
        let hosted = rooms.get_mut(code).expect("a room");
        for send in hosted.room.handle(self.now(), event) {
            if let Some(tx) = hosted.conns.get(&send.to) {
                let _ = tx.send(send.msg);
            }
        }
        let report = match hosted.room.summary(code) {
            Some(summary) if !hosted.room.empty() => Report::Listed { summary },
            _ => Report::Unlisted { code: code.to_string() },
        };
        if hosted.room.empty() {
            rooms.remove(code);
        }
        drop(rooms);
        let mut directory = self.directory.lock().unwrap();
        let (list, watchers) = &mut *directory;
        if list.report(report, self.now()) {
            let msg = FromDirectory::Rooms { rooms: list.list() };
            watchers.retain(|tx| tx.send(msg.clone()).is_ok());
        }
    }
}

/// Serve rooms on `listener` until the process ends.
pub fn serve(listener: TcpListener) {
    let rooms = Rooms {
        start: Instant::now(),
        rooms: Arc::new(Mutex::new(HashMap::new())),
        directory: Arc::new(Mutex::new((Directory::default(), vec![]))),
    };
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
    // Far longer messages than the limit aren't even read in.
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(16 * MAX_MESSAGE_BYTES))
        .max_frame_size(Some(16 * MAX_MESSAGE_BYTES));
    let Ok(mut ws) = tungstenite::accept_hdr_with_config(stream, callback, Some(config)) else { return };
    if code == "/ROOMS" {
        return watch_list(rooms, ws);
    }
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
        let mut ended = false;
        while let Ok(msg) = outgoing.try_recv() {
            ended |= msg.ends_connection();
            let _ = ws.send(Message::text(msg.to_json()));
            idle = false;
        }
        let _ = ws.flush();
        // Removed by the host, or refused: the room has already let them go.
        if ended {
            let _ = ws.close(None);
            let _ = ws.flush();
            break;
        }
        match ws.read() {
            Ok(Message::Text(text)) if text.as_str() == limits::PING => {
                let _ = ws.send(Message::text(limits::PONG));
                idle = false;
            }
            Ok(Message::Text(text)) => {
                // No limit on how many: tests play faster than real time.
                if text.len() > MAX_MESSAGE_BYTES {
                    let _ = ws.send(Message::text(FromRoom::Closed { reason: limits::TOO_LONG.into() }.to_json()));
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    break;
                }
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

/// A connection watching the list: the list now, then again whenever it changes.
fn watch_list(rooms: Rooms, mut ws: tungstenite::WebSocket<TcpStream>) {
    let (tx, updates) = channel();
    {
        let mut directory = rooms.directory.lock().unwrap();
        if directory.1.len() >= MAX_WATCHERS {
            // Watchers that have gone are only noticed on sending.
            let list = FromDirectory::Rooms { rooms: directory.0.list() };
            directory.1.retain(|tx| tx.send(list.clone()).is_ok());
        }
        if directory.1.len() >= MAX_WATCHERS {
            let _ = ws.close(None);
            let _ = ws.flush();
            return;
        }
        let _ = tx.send(FromDirectory::Rooms { rooms: directory.0.list() });
        directory.1.push(tx);
    }
    ws.get_mut().set_nonblocking(true).expect("non-blocking socket");
    loop {
        let mut idle = true;
        while let Ok(msg) = updates.try_recv() {
            let _ = ws.send(Message::text(msg.to_json()));
            idle = false;
        }
        let _ = ws.flush();
        match ws.read() {
            Ok(Message::Close(_)) => break,
            Ok(_) => idle = false,
            Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        if idle {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
