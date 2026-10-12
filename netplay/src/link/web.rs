//! The websockets in a browser: the page's `WebSocket`, whose events queue up here until polled.
//! Messages sent before the connection opens wait for it. A room link pings on a timer of its
//! own (the game loop stops in a hidden tab).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use protocol::limits::{PING, PING_MS, PONG};
use protocol::{FromDirectory, FromRoom, ToRoom};
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, MessageEvent, WebSocket};

use super::{LinkEvent, Listing};

/// A browser websocket and what's come in on it, as text.
struct Socket {
    ws: Option<WebSocket>,
    inbox: Rc<RefCell<VecDeque<Result<String, String>>>>,
    /// Waiting for the connection to open.
    outbox: Rc<RefCell<Vec<String>>>,
    _handlers: Vec<Closure<dyn FnMut(JsValue)>>,
    /// The ping timer, if this socket pings.
    pinging: Option<(i32, Closure<dyn FnMut(JsValue)>)>,
}

impl Socket {
    fn open(url: &str) -> Socket {
        let inbox: Rc<RefCell<VecDeque<Result<String, String>>>> = Rc::default();
        let outbox: Rc<RefCell<Vec<String>>> = Rc::default();
        let ws = match WebSocket::new(url) {
            Ok(ws) => ws,
            Err(e) => {
                inbox.borrow_mut().push_back(Err(format!("can't reach {url}: {e:?}")));
                return Socket { ws: None, inbox, outbox, _handlers: vec![], pinging: None };
            }
        };
        let on_message = {
            let inbox = inbox.clone();
            Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
                if let Some(text) = e.unchecked_into::<MessageEvent>().data().as_string() {
                    inbox.borrow_mut().push_back(Ok(text));
                }
            })
        };
        let on_close = {
            let inbox = inbox.clone();
            Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
                let e = e.unchecked_into::<CloseEvent>();
                let why = if e.reason().is_empty() { format!("closed ({})", e.code()) } else { e.reason() };
                inbox.borrow_mut().push_back(Err(why));
            })
        };
        let on_open = {
            let (outbox, ws) = (outbox.clone(), ws.clone());
            Closure::<dyn FnMut(JsValue)>::new(move |_| {
                for text in outbox.borrow_mut().drain(..) {
                    let _ = ws.send_with_str(&text);
                }
            })
        };
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        Socket { ws: Some(ws), inbox, outbox, _handlers: vec![on_message, on_close, on_open], pinging: None }
    }

    /// Ping every `PING_MS` while open, starting with the first sent (the outbox waits for the
    /// connection).
    fn ping(&mut self) {
        let (Some(ws), Some(window)) = (self.ws.clone(), web_sys::window()) else { return };
        self.send(PING.into());
        let tick = Closure::<dyn FnMut(JsValue)>::new(move |_| {
            if ws.ready_state() == WebSocket::OPEN {
                let _ = ws.send_with_str(PING);
            }
        });
        let ms = PING_MS as i32;
        if let Ok(id) = window.set_interval_with_callback_and_timeout_and_arguments_0(tick.as_ref().unchecked_ref(), ms) {
            self.pinging = Some((id, tick));
        }
    }

    fn send(&self, text: String) {
        match &self.ws {
            Some(ws) if ws.ready_state() == WebSocket::OPEN => {
                let _ = ws.send_with_str(&text);
            }
            Some(ws) if ws.ready_state() == WebSocket::CONNECTING => self.outbox.borrow_mut().push(text),
            _ => {}
        }
    }

    fn poll(&self) -> Option<Result<String, String>> {
        self.inbox.borrow_mut().pop_front()
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        if let (Some((id, _)), Some(window)) = (&self.pinging, web_sys::window()) {
            window.clear_interval_with_handle(*id);
        }
        if let Some(ws) = &self.ws {
            ws.set_onmessage(None);
            ws.set_onclose(None);
            ws.set_onopen(None);
            let _ = ws.close();
        }
    }
}

/// The websocket to a room.
pub struct Link(Socket);

impl Link {
    pub fn open(url: &str) -> Link {
        let mut socket = Socket::open(url);
        socket.ping();
        Link(socket)
    }

    pub fn send(&self, msg: ToRoom) {
        self.0.send(msg.to_json());
    }

    /// The next thing that came in, if any.
    pub fn poll(&mut self) -> Option<LinkEvent> {
        loop {
            return match self.0.poll()? {
                Ok(text) if text == PONG => continue,
                Ok(text) => match FromRoom::from_json(&text) {
                    Some(msg) => Some(LinkEvent::Message(msg)),
                    None => continue,
                },
                Err(why) => Some(LinkEvent::Closed(why)),
            };
        }
    }
}

/// The list of rooms, from a websocket to the server's `/rooms`.
pub struct RoomList {
    socket: Socket,
    latest: Option<Listing>,
}

impl RoomList {
    pub fn open(server: &str) -> RoomList {
        RoomList { socket: Socket::open(&format!("{}/rooms", server.trim_end_matches('/'))), latest: None }
    }

    /// The latest list: none until it first arrives; an error if the connection failed or ended.
    pub fn latest(&mut self) -> Option<&Listing> {
        while let Some(update) = self.socket.poll() {
            match update {
                Ok(text) => {
                    if let Some(FromDirectory::Rooms { rooms }) = FromDirectory::from_json(&text) {
                        self.latest = Some(Ok(rooms));
                    }
                }
                Err(why) => self.latest = Some(Err(why)),
            }
        }
        self.latest.as_ref()
    }
}
