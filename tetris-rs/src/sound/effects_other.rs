//! Sound effects in slots 0 (noise), 2 (DMC) and 3 (triangle).
//!
//! Stubs: each method is to be rewritten from the routine named in its doc comment.

use crate::shadow::Routine;
#[allow(unused_imports)]
use crate::{layout, shadow::Ret, Game};
#[allow(unused_imports)]
use super::{DATA_PAGE, NOISE, SQ1, SQ2, TRI};

impl Game {
    /// `soundEffectSlot0_makesNoSound` (not yet rewritten).
    pub fn sound_effect_slot0_makes_no_sound(&mut self) {}

    /// `soundEffectSlot0_gameOverCurtainInit` (not yet rewritten).
    pub fn sound_effect_slot0_game_over_curtain_init(&mut self) {}

    /// `soundEffectSlot0_endingRocketInit` (not yet rewritten).
    pub fn sound_effect_slot0_ending_rocket_init(&mut self) {}

    /// `advanceSoundEffectSlot0WithoutUpdate` (not yet rewritten).
    pub fn advance_sound_effect_slot0_without_update(&mut self) {}

    /// `updateSoundEffectSlot0_apu` (not yet rewritten).
    pub fn update_sound_effect_slot0_apu(&mut self) {}

    /// `soundEffectSlot3_fallingAlien` (not yet rewritten).
    pub fn sound_effect_slot3_falling_alien(&mut self) {}

    /// `soundEffectSlot3_donk` (not yet rewritten).
    pub fn sound_effect_slot3_donk(&mut self) {}

    /// `updateSoundEffectSlot3_apu` (not yet rewritten).
    pub fn update_sound_effect_slot3_apu(&mut self) {}

    /// `soundEffectSlot3Playing_advance` (not yet rewritten).
    pub fn sound_effect_slot3_playing_advance(&mut self) {}

    /// `soundEffectSlot2_lowBuzz` (not yet rewritten).
    pub fn sound_effect_slot2_low_buzz(&mut self) {}

    /// `soundEffectSlot2_mediumBuzz` (not yet rewritten).
    pub fn sound_effect_slot2_medium_buzz(&mut self) {}
}

pub const SHADOW: &[Routine] = &[];
