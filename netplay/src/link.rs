//! The websocket to a room, on its own thread, for native clients.

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::time::Duration;

use protocol::{FromRoom, ToRoom};

/// What comes in from a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkEvent {
    Message(FromRoom),
    /// The connection couldn't be made, or has ended; why.
    Closed(String),
}

/// The websocket, on its own thread: what the room says comes in on `rx`, messages to it go out
/// on `tx`. Opening doesn't wait for the connection; a failure arrives as `LinkEvent::Closed`.
/// Dropping the link closes the connection.
pub struct Link {
    pub tx: Sender<ToRoom>,
    pub rx: Receiver<LinkEvent>,
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
            loop {
                let mut idle = true;
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
                    Ok(tungstenite::Message::Text(text)) => {
                        match FromRoom::from_json(&text) {
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
}
