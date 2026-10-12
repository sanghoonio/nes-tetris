//! The rooms, as a Cloudflare Worker. `GET /new` makes up a room code; a websocket to
//! `/room/<CODE>` joins that room. Each room is a Durable Object holding a `protocol::Room`,
//! which does all the deciding; this passes websocket traffic through it. A websocket to `/rooms`
//! watches the list of public rooms, kept by one more Durable Object (`DirectoryObject`, holding
//! a `protocol::Directory`), which each room tells whenever its summary changes, and every
//! `REFRESH_MS` while it has people in it.
//!
//! Websockets are accepted with the hibernation API, so a room nobody is sending to can be put to
//! sleep without dropping its connections, and isn't billed for the time. Each connection's id
//! rides along as the socket's attachment. The room is saved to storage whenever no match is on
//! and reloaded on waking; a match's input log stays in memory (messages every 250 ms keep the
//! room awake), so a room restarted mid-match loses that match.

use std::cell::RefCell;

use protocol::directory::{Report, REFRESH_MS};
use protocol::{room_code, valid_code, ConnId, Directory, Event, FromDirectory, FromRoom, Room, ToRoom};
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
        ["ROOMS"] => directory(&env)?.fetch_with_request(req).await,
        _ => Response::error("not found", 404),
    }
}

/// The one directory.
fn directory(env: &Env) -> Result<Stub> {
    env.durable_object("DIRECTORY")?.id_from_name("list")?.get_stub()
}

fn now() -> u64 {
    Date::now().as_millis()
}

#[durable_object]
pub struct RoomObject {
    state: State,
    env: Env,
    /// Loaded on the first event after the object starts (or wakes).
    room: RefCell<Option<Room>>,
    /// The room's code (the object's name, which it can't ask for), saved on first contact.
    code: RefCell<Option<String>>,
    /// What this room last told the directory, since it woke.
    reported: RefCell<Option<Report>>,
}

impl RoomObject {
    /// Feed `event` to the room, send what it says, and save it between matches.
    async fn run(&self, event: Event) -> Result<()> {
        if self.room.borrow().is_none() {
            let saved: Option<Room> = self.state.storage().get("room").await?;
            let room = saved.unwrap_or_else(|| Room::new((random() * 9e15) as u64));
            *self.room.borrow_mut() = Some(room);
        }
        let (sends, save) = {
            let mut room = self.room.borrow_mut();
            let room = room.as_mut().unwrap();
            let sends = room.handle(event);
            (sends, (!room.playing()).then(|| room.clone()))
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
        self.report(false).await
    }

    /// Tell the directory about the room if that's changed (or `again`), and while anyone is in
    /// it, come back to tell it again in `REFRESH_MS`.
    async fn report(&self, again: bool) -> Result<()> {
        if self.code.borrow().is_none() {
            *self.code.borrow_mut() = self.state.storage().get("code").await?;
        }
        let Some(code) = self.code.borrow().clone() else { return Ok(()) };
        let (report, occupied) = {
            let room = self.room.borrow();
            let Some(room) = room.as_ref() else { return Ok(()) };
            let report = match room.summary(&code) {
                Some(summary) if !room.empty() => Report::Listed { summary },
                _ => Report::Unlisted { code },
            };
            (report, !room.empty())
        };
        if !again && self.reported.borrow().as_ref() == Some(&report) {
            return Ok(());
        }
        let mut init = RequestInit::new();
        init.with_method(Method::Post).with_body(Some(report.to_json().into()));
        directory(&self.env)?.fetch_with_request(Request::new_with_init("https://directory/report", &init)?).await?;
        *self.reported.borrow_mut() = Some(report);
        if occupied {
            self.state.storage().set_alarm(std::time::Duration::from_millis(REFRESH_MS)).await?;
        }
        Ok(())
    }

    fn conn(ws: &WebSocket) -> Option<ConnId> {
        ws.deserialize_attachment::<ConnId>().ok().flatten()
    }
}

impl DurableObject for RoomObject {
    fn new(state: State, env: Env) -> Self {
        RoomObject { state, env, room: RefCell::new(None), code: RefCell::new(None), reported: RefCell::new(None) }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        if req.headers().get("Upgrade")?.as_deref() != Some("websocket") {
            return Response::error("websocket only", 426);
        }
        if self.code.borrow().is_none() {
            let code = req.url()?.path().trim_matches('/').rsplit('/').next().unwrap_or("").to_uppercase();
            self.state.storage().put("code", &code).await?;
            *self.code.borrow_mut() = Some(code);
        }
        let pair = WebSocketPair::new()?;
        let conn = (random() * 9e15) as ConnId;
        self.state.accept_web_socket(&pair.server);
        pair.server.serialize_attachment(conn)?;
        self.run(Event::Joined(conn)).await?;
        Ok(ResponseBuilder::new().with_status(101).with_websocket(pair.client).empty())
    }

    /// The refresh: tell the directory again (the room is loaded first, if it slept).
    async fn alarm(&self) -> Result<Response> {
        if self.room.borrow().is_none() {
            let saved: Option<Room> = self.state.storage().get("room").await?;
            *self.room.borrow_mut() = saved;
        }
        self.report(true).await?;
        Response::ok("")
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

}

/// The list of public rooms. Rooms post their reports to `/report`; websockets to it watch the
/// list, sent whole on connecting and on every change. Entries that stop being reported expire
/// (an alarm at the next one due).
#[durable_object]
pub struct DirectoryObject {
    state: State,
    directory: RefCell<Option<Directory>>,
}

impl DirectoryObject {
    async fn load(&self) -> Result<()> {
        if self.directory.borrow().is_none() {
            let saved: Option<Directory> = self.state.storage().get("directory").await?;
            *self.directory.borrow_mut() = Some(saved.unwrap_or_default());
        }
        Ok(())
    }

    /// Apply `change` to the directory; if the list changed, save it and send it to everyone
    /// watching. Then set the alarm for the next expiry.
    async fn update(&self, change: impl FnOnce(&mut Directory) -> bool) -> Result<()> {
        self.load().await?;
        let (changed, list, next) = {
            let mut directory = self.directory.borrow_mut();
            let directory = directory.as_mut().unwrap();
            let changed = change(directory) | directory.expire(now());
            (changed, directory.clone(), directory.next_expiry())
        };
        if changed {
            self.state.storage().put("directory", &list).await?;
            let msg = FromDirectory::Rooms { rooms: list.list() }.to_json();
            for ws in self.state.get_websockets() {
                let _ = ws.send_with_str(&msg);
            }
        }
        if let Some(at) = next {
            let wait = at.saturating_sub(now()).max(1000);
            self.state.storage().set_alarm(std::time::Duration::from_millis(wait)).await?;
        }
        Ok(())
    }
}

impl DurableObject for DirectoryObject {
    fn new(state: State, _env: Env) -> Self {
        DirectoryObject { state, directory: RefCell::new(None) }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        if req.method() == Method::Post && req.path() == "/report" {
            let Some(report) = Report::from_json(&req.text().await?) else { return Response::error("unreadable report", 400) };
            self.update(|d| d.report(report, now())).await?;
            return Response::ok("");
        }
        if req.headers().get("Upgrade")?.as_deref() != Some("websocket") {
            return Response::error("websocket only", 426);
        }
        self.load().await?;
        let pair = WebSocketPair::new()?;
        self.state.accept_web_socket(&pair.server);
        let rooms = self.directory.borrow().as_ref().map(Directory::list).unwrap_or_default();
        pair.server.send_with_str(FromDirectory::Rooms { rooms }.to_json())?;
        Ok(ResponseBuilder::new().with_status(101).with_websocket(pair.client).empty())
    }

    async fn alarm(&self) -> Result<Response> {
        self.update(|_| false).await?;
        Response::ok("")
    }

    /// Watchers send nothing.
    async fn websocket_message(&self, _ws: WebSocket, _message: WebSocketIncomingMessage) -> Result<()> {
        Ok(())
    }

    async fn websocket_close(&self, ws: WebSocket, _code: usize, _reason: String, _was_clean: bool) -> Result<()> {
        let _ = ws.close(Some(1000), Some("closed"));
        Ok(())
    }
}
