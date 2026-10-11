//! The websocket to a room, on its own thread, for native clients.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use protocol::{FromRoom, ToRoom};

/// The websocket, on its own thread: messages from the room come in on `rx`, messages to it go
/// out on `tx`. A closed connection arrives as `None`.
pub struct Link {
    pub tx: Sender<ToRoom>,
    pub rx: Receiver<Option<FromRoom>>,
}

impl Link {
    pub fn connect(url: &str) -> Link {
        let (mut ws, _) = tungstenite::connect(url).unwrap_or_else(|e| panic!("connecting to {url}: {e}"));
        match ws.get_mut() {
            tungstenite::stream::MaybeTlsStream::Plain(s) => s.set_nonblocking(true),
            tungstenite::stream::MaybeTlsStream::Rustls(s) => s.get_mut().set_nonblocking(true),
            _ => Ok(()),
        }
        .expect("non-blocking socket");
        let (to_room, outgoing) = channel::<ToRoom>();
        let (incoming, from_room) = channel::<Option<FromRoom>>();
        std::thread::spawn(move || loop {
            let mut idle = true;
            while let Ok(msg) = outgoing.try_recv() {
                let _ = ws.send(tungstenite::Message::text(msg.to_json()));
                idle = false;
            }
            let _ = ws.flush();
            match ws.read() {
                Ok(tungstenite::Message::Text(text)) => {
                    match FromRoom::from_json(&text) {
                        Some(msg) => {
                            let _ = incoming.send(Some(msg));
                        }
                        None => eprintln!("unreadable message: {text}"),
                    }
                    idle = false;
                }
                Ok(tungstenite::Message::Close(_)) => {
                    let _ = incoming.send(None);
                    return;
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    eprintln!("connection: {e}");
                    let _ = incoming.send(None);
                    return;
                }
            }
            if idle {
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Link { tx: to_room, rx: from_room }
    }
}

