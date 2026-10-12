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
