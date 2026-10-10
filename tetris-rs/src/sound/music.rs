//! The music engine: runs the current track's channel scripts once a frame.
//!
//! Stubs: each method is to be rewritten from the routine named in its doc comment.

use crate::shadow::Routine;
#[allow(unused_imports)]
use crate::{layout, shadow::Ret, Game};
#[allow(unused_imports)]
use super::{DATA_PAGE, NOISE, SQ1, SQ2, TRI};

impl Game {
    /// `updateMusic` (not yet rewritten).
    pub fn update_music(&mut self) {}
}

pub const SHADOW: &[Routine] = &[];
