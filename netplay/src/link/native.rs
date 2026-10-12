//! The websockets, each on its own thread, for native clients.

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use protocol::limits::{PING, PING_MS, PONG};
use protocol::ToRoom;

use super::{LinkEvent, Listing};

/// The websocket to a room, on its own thread. Opening doesn't wait for the connection; a
/// failure arrives as `LinkEvent::Closed`. Dropping the link closes the connection. It pings the
/// room every `PING_MS`, from opening.
pub struct Link {
    tx: Sender<ToRoom>,
    rx: Receiver<LinkEvent>,
}

impl Link {
    pub fn open(url: &str) -> Link {
        let (to_room, outgoing) = channel::<ToRoom>();
        let (incoming, from_room) = channel::<LinkEvent>();
        let url = url.to_string();
        std::thread::spawn(move || {
            let closed = |why: String| {
                let _ = incoming.send(LinkEvent::Closed(why));
            };
            let mut ws = match tungstenite::connect(url.as_str()) {
                Ok((ws, _)) => ws,
                Err(e) => return closed(format!("can't reach {url}: {e}")),
            };
            let nonblocking = match ws.get_mut() {
                tungstenite::stream::MaybeTlsStream::Plain(s) => s.set_nonblocking(true),
                tungstenite::stream::MaybeTlsStream::Rustls(s) => s.get_mut().set_nonblocking(true),
                _ => Ok(()),
            };
            if let Err(e) = nonblocking {
                return closed(e.to_string());
            }
            let mut pinged: Option<Instant> = None;
            loop {
                let mut idle = true;
                if pinged.is_none_or(|t| t.elapsed() >= Duration::from_millis(PING_MS)) {
                    let _ = ws.send(tungstenite::Message::text(PING));
                    pinged = Some(Instant::now());
                }
                loop {
                    match outgoing.try_recv() {
                        Ok(msg) => {
                            let _ = ws.send(tungstenite::Message::text(msg.to_json()));
                            idle = false;
                        }
                        Err(TryRecvError::Empty) => break,
                        // The link was dropped: leave.
                        Err(TryRecvError::Disconnected) => {
                            let _ = ws.close(None);
                            let _ = ws.flush();
                            return;
                        }
                    }
                }
                let _ = ws.flush();
                match ws.read() {
                    Ok(tungstenite::Message::Text(text)) if text.as_str() == PONG => {}
                    Ok(tungstenite::Message::Text(text)) => {
                        match protocol::FromRoom::from_json(&text) {
                            Some(msg) => {
                                let _ = incoming.send(LinkEvent::Message(msg));
                            }
                            None => eprintln!("unreadable message: {text}"),
                        }
                        idle = false;
                    }
                    Ok(tungstenite::Message::Close(_)) => return closed("the room closed the connection".into()),
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return closed(e.to_string()),
                }
                if idle {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        });
        Link { tx: to_room, rx: from_room }
    }

    pub fn send(&self, msg: ToRoom) {
        let _ = self.tx.send(msg);
    }

    /// The next thing that came in, if any.
    pub fn poll(&mut self) -> Option<LinkEvent> {
        self.rx.try_recv().ok()
    }
}

/// The list of rooms, kept up to date on its own thread from a websocket to the server's
/// `/rooms`. Opening doesn't wait; dropping it closes the connection.
pub struct RoomList {
    rx: Receiver<Listing>,
    /// Tells the thread to stop when dropped.
    _stop: Sender<()>,
    /// The latest list (none until the first arrives), or why there's none.
    latest: Option<Listing>,
}

impl RoomList {
    /// Watch the list at `server` (`ws://host:port` or `wss://...`).
    pub fn open(server: &str) -> RoomList {
        let (tx, rx) = channel();
        let (stop, stopped) = channel::<()>();
        let url = format!("{}/rooms", server.trim_end_matches('/'));
        std::thread::spawn(move || {
            let mut ws = match tungstenite::connect(url.as_str()) {
                Ok((ws, _)) => ws,
                Err(e) => return drop(tx.send(Err(format!("can't reach {url}: {e}")))),
            };
            let nonblocking = match ws.get_mut() {
                tungstenite::stream::MaybeTlsStream::Plain(s) => s.set_nonblocking(true),
                tungstenite::stream::MaybeTlsStream::Rustls(s) => s.get_mut().set_nonblocking(true),
                _ => Ok(()),
            };
            if let Err(e) = nonblocking {
                return drop(tx.send(Err(e.to_string())));
            }
            loop {
                if let Err(TryRecvError::Disconnected) = stopped.try_recv() {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return;
                }
                match ws.read() {
                    Ok(tungstenite::Message::Text(text)) => {
                        if let Some(protocol::FromDirectory::Rooms { rooms }) = protocol::FromDirectory::from_json(&text) {
                            let _ = tx.send(Ok(rooms));
                        }
                    }
                    Ok(tungstenite::Message::Close(_)) => return drop(tx.send(Err("the server closed the list".into()))),
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        let _ = ws.flush();
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => return drop(tx.send(Err(e.to_string()))),
                }
            }
        });
        RoomList { rx, _stop: stop, latest: None }
    }

    /// The latest list: none until it first arrives; an error if the connection failed or ended.
    pub fn latest(&mut self) -> Option<&Listing> {
        while let Ok(update) = self.rx.try_recv() {
            self.latest = Some(update);
        }
        self.latest.as_ref()
    }
}
