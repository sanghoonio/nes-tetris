//! The local room server's limits: a message too long ends the connection with the reason
//! (`protocol::limits`). (How many may be sent is limited by the Worker only.)

use std::net::TcpStream;

use protocol::limits;
use protocol::{FromRoom, ToRoom};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

fn server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || netplay::local::serve(listener));
    format!("ws://127.0.0.1:{port}")
}

fn join(server: &str, code: &str) -> WebSocket<MaybeTlsStream<TcpStream>> {
    let (mut ws, _) = tungstenite::connect(format!("{server}/room/{code}")).unwrap();
    ws.send(Message::text(ToRoom::Hello { name: "ana".into(), watch: false }.to_json())).unwrap();
    ws
}

/// What the server says until it closes the connection.
fn until_closed(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> Vec<FromRoom> {
    let mut said = vec![];
    while let Ok(msg) = ws.read() {
        if let Message::Text(text) = msg {
            said.extend(FromRoom::from_json(&text));
        }
    }
    said
}

fn closed_for(said: &[FromRoom]) -> Option<&str> {
    said.iter().find_map(|m| match m {
        FromRoom::Closed { reason } => Some(reason.as_str()),
        _ => None,
    })
}

#[test]
fn a_message_too_long_ends_the_connection() {
    let server = server();
    let mut ws = join(&server, "AAAA");
    ws.send(Message::text(format!("{{\"Hello\":{{\"name\":\"{}\"}}}}", "A".repeat(5000)))).unwrap();
    assert_eq!(closed_for(&until_closed(&mut ws)), Some(limits::TOO_LONG));
}
