//! Small hardware helpers: memory fills, mapper banking, OAM DMA, NMI control.

use crate::shadow::Routine;
#[allow(unused_imports)]
use crate::{buttons, layout, shadow::Ret, Game};

pub const SHADOW: &[Routine] = &[];
