//! The rooms, as a Cloudflare Worker. `GET /new` makes up a room code; a websocket to
//! `/room/<CODE>` joins that room. Each room is a Durable Object holding a `protocol::Room`,
//! which does all the deciding; this passes websocket traffic through it.
//!
//! Websockets are accepted with the hibernation API, so a room nobody is sending to can be put to
//! sleep without dropping its connections, and isn't billed for the time. Each connection's id
//! rides along as the socket's attachment. The room is saved to storage whenever no match is on
//! and reloaded on waking; a match's input log stays in memory (messages every 250 ms keep the
//! room awake), so a room restarted mid-match loses that match.

use std::cell::RefCell;

use protocol::{room_code, valid_code, ConnId, Event, FromRoom, Room, ToRoom};
use serde::Serialize;
use worker::*;

fn random() -> f64 {
    js_sys::Math::random()
}

#[derive(Serialize)]
struct NewRoom {
    code: String,
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let path: Vec<String> = url.path().trim_matches('/').split('/').map(str::to_uppercase).collect();
    match path.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["NEW"] => {
            let code = room_code((random() * 9e15) as u64);
            let mut response = Response::from_json(&NewRoom { code })?;
            response.headers_mut().set("Access-Control-Allow-Origin", "*")?;
            Ok(response)
        }
        ["ROOM", code] if valid_code(code) => {
            env.durable_object("ROOMS")?.id_from_name(code)?.get_stub()?.fetch_with_request(req).await
        }
        _ => Response::error("not found", 404),
    }
}

#[durable_object]
pub struct RoomObject {
    state: State,
    /// Loaded on the first event after the object starts (or wakes).
    room: RefCell<Option<Room>>,
}

impl RoomObject {
    /// Feed `event` to the room, send what it says, save it between matches, and set the alarm.
    async fn run(&self, event: Event) -> Result<()> {
        if self.room.borrow().is_none() {
            let saved: Option<Room> = self.state.storage().get("room").await?;
            let room = saved.unwrap_or_else(|| Room::new((random() * 9e15) as u64));
            *self.room.borrow_mut() = Some(room);
        }
        let now = Date::now().as_millis();
        let (sends, save, alarm) = {
            let mut room = self.room.borrow_mut();
            let room = room.as_mut().unwrap();
            let sends = room.handle(now, event);
            (sends, (!room.playing()).then(|| room.clone()), room.next_alarm())
        };
        let sockets: Vec<(ConnId, WebSocket)> = self
            .state
            .get_websockets()
            .into_iter()
            .filter_map(|ws| ws.deserialize_attachment::<ConnId>().ok().flatten().map(|c| (c, ws)))
            .collect();
        for send in sends {
            if let Some((_, ws)) = sockets.iter().find(|(c, _)| *c == send.to) {
                let _ = ws.send_with_str(send.msg.to_json());
            }
        }
        if let Some(room) = save {
            self.state.storage().put("room", &room).await?;
        }
        if let Some(at) = alarm {
            self.state.storage().set_alarm(at.saturating_sub(now) as i64).await?;
        }
        Ok(())
    }

    fn conn(ws: &WebSocket) -> Option<ConnId> {
        ws.deserialize_attachment::<ConnId>().ok().flatten()
    }
}

impl DurableObject for RoomObject {
    fn new(state: State, _env: Env) -> Self {
        RoomObject { state, room: RefCell::new(None) }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        if req.headers().get("Upgrade")?.as_deref() != Some("websocket") {
            return Response::error("websocket only", 426);
        }
        let pair = WebSocketPair::new()?;
        let conn = (random() * 9e15) as ConnId;
        self.state.accept_web_socket(&pair.server);
        pair.server.serialize_attachment(conn)?;
        self.run(Event::Joined(conn)).await?;
        Ok(ResponseBuilder::new().with_status(101).with_websocket(pair.client).empty())
    }

    async fn websocket_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        let Some(conn) = Self::conn(&ws) else { return Ok(()) };
        let parsed = match message {
            WebSocketIncomingMessage::String(s) => ToRoom::from_json(&s),
            WebSocketIncomingMessage::Binary(_) => None,
        };
        match parsed {
            Some(msg) => self.run(Event::Message(conn, msg)).await,
            None => {
                let _ = ws.send_with_str(FromRoom::Error { message: "unreadable message".into() }.to_json());
                Ok(())
            }
        }
    }

    async fn websocket_close(&self, ws: WebSocket, _code: usize, _reason: String, _was_clean: bool) -> Result<()> {
        let _ = ws.close(Some(1000), Some("closed"));
        match Self::conn(&ws) {
            Some(conn) => self.run(Event::Left(conn)).await,
            None => Ok(()),
        }
    }

    async fn websocket_error(&self, ws: WebSocket, _error: Error) -> Result<()> {
        match Self::conn(&ws) {
            Some(conn) => self.run(Event::Left(conn)).await,
            None => Ok(()),
        }
    }

    async fn alarm(&self) -> Result<Response> {
        self.run(Event::Timer).await?;
        Response::ok("")
    }
}
