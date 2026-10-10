//! Sound effects in slot 1 (square 1): piece moves, line clears, level up, menus.
//!
//! Stubs: each method is to be rewritten from the routine named in its doc comment.

use crate::shadow::Routine;
#[allow(unused_imports)]
use crate::{layout, shadow::Ret, Game};
#[allow(unused_imports)]
use super::{DATA_PAGE, NOISE, SQ1, SQ2, TRI};

impl Game {
    /// `soundEffectSlot1_menuOptionSelectInit` (not yet rewritten).
    pub fn sound_effect_slot1_menu_option_select_init(&mut self) {}

    /// `soundEffectSlot1_menuScreenSelectInit` (not yet rewritten).
    pub fn sound_effect_slot1_menu_screen_select_init(&mut self) {}

    /// `soundEffectSlot1_shiftTetriminoInit` (not yet rewritten).
    pub fn sound_effect_slot1_shift_tetrimino_init(&mut self) {}

    /// `soundEffectSlot1_tetrisAchievedInit` (not yet rewritten).
    pub fn sound_effect_slot1_tetris_achieved_init(&mut self) {}

    /// `soundEffectSlot1_rotateTetriminoInit` (not yet rewritten).
    pub fn sound_effect_slot1_rotate_tetrimino_init(&mut self) {}

    /// `soundEffectSlot1_levelUpInit` (not yet rewritten).
    pub fn sound_effect_slot1_level_up_init(&mut self) {}

    /// `soundEffectSlot1_lockTetriminoInit` (not yet rewritten).
    pub fn sound_effect_slot1_lock_tetrimino_init(&mut self) {}

    /// `soundEffectSlot1_chirpChirpInit` (not yet rewritten).
    pub fn sound_effect_slot1_chirp_chirp_init(&mut self) {}

    /// `soundEffectSlot1_lineClearingInit` (not yet rewritten).
    pub fn sound_effect_slot1_line_clearing_init(&mut self) {}

    /// `soundEffectSlot1_lineCompletedInit` (not yet rewritten).
    pub fn sound_effect_slot1_line_completed_init(&mut self) {}

    /// `soundEffectSlot1_menuOptionSelectPlaying` (not yet rewritten).
    pub fn sound_effect_slot1_menu_option_select_playing(&mut self) {}

    /// `soundEffectSlot1_menuScreenSelectPlaying` (not yet rewritten).
    pub fn sound_effect_slot1_menu_screen_select_playing(&mut self) {}

    /// `soundEffectSlot1Playing_advance` (not yet rewritten).
    pub fn sound_effect_slot1_playing_advance(&mut self) {}

    /// `soundEffectSlot1_tetrisAchievedPlaying` (not yet rewritten).
    pub fn sound_effect_slot1_tetris_achieved_playing(&mut self) {}

    /// `soundEffectSlot1_rotateTetriminoPlaying` (not yet rewritten).
    pub fn sound_effect_slot1_rotate_tetrimino_playing(&mut self) {}

    /// `soundEffectSlot1_levelUpPlaying` (not yet rewritten).
    pub fn sound_effect_slot1_level_up_playing(&mut self) {}

    /// `soundEffectSlot1_chirpChirpPlaying` (not yet rewritten).
    pub fn sound_effect_slot1_chirp_chirp_playing(&mut self) {}

    /// `soundEffectSlot1_lineClearingPlaying` (not yet rewritten).
    pub fn sound_effect_slot1_line_clearing_playing(&mut self) {}

    /// `soundEffectSlot1_lineCompletedPlaying` (not yet rewritten).
    pub fn sound_effect_slot1_line_completed_playing(&mut self) {}
}

pub const SHADOW: &[Routine] = &[];
