//! The websockets to a room and to the list of rooms: on their own threads natively, the
//! browser's `WebSocket` in WebAssembly. Both open without waiting and close when dropped.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::{Link, RoomList};
#[cfg(target_arch = "wasm32")]
pub use web::{Link, RoomList};

use protocol::{FromRoom, RoomSummary};

/// What comes in from a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkEvent {
    Message(FromRoom),
    /// The connection couldn't be made, or has ended; why.
    Closed(String),
}

/// The list of rooms as last heard, or why there's none.
pub type Listing = Result<Vec<RoomSummary>, String>;

/// A room run inside this program (`protocol::Room` has no networking), for a match against a
/// bot with no server: the same messages as a link, delivered at once.
pub struct LocalRoom {
    room: protocol::Room,
    inbox: std::collections::VecDeque<FromRoom>,
}

/// This program's connection to its own room.
const ME: protocol::ConnId = 1;

impl LocalRoom {
    pub fn new(seed: u64) -> LocalRoom {
        let mut room = protocol::Room::new(seed);
        room.handle(0, protocol::Event::Joined(ME));
        LocalRoom { room, inbox: Default::default() }
    }

    pub fn send(&mut self, msg: protocol::ToRoom) {
        for send in self.room.handle(0, protocol::Event::Message(ME, msg)) {
            if send.to == ME {
                self.inbox.push_back(send.msg);
            }
        }
    }

    pub fn poll(&mut self) -> Option<LinkEvent> {
        self.inbox.pop_front().map(LinkEvent::Message)
    }
}
