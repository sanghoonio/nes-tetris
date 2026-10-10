//! Switching between the active player and the per-player copies, and small play states.

use crate::shadow::{Ret, Routine};
use crate::vars::Le16;
use crate::{buttons, layout, Game};

/// Page high bytes of the two playfields (`playfield` and `playfieldForSecondPlayer`).
const PLAYFIELD_PAGE: u16 = 0x0400;
const PLAYFIELD_PAGE_SECOND_PLAYER: u16 = 0x0500;
/// `activePlayer` values.
const ACTIVE_PLAYER_1: u8 = 0x01;
const ACTIVE_PLAYER_2: u8 = 0x02;
/// Character bank holding the game graphics.
const CHR_GAME: u8 = 0x03;
/// `hidden`: the orientation id meaning "no piece on the board".
const HIDDEN_PIECE: u8 = 0x13;

impl Game {
    /// `makePlayer1Active`: make player 1 the active player. Copies player 1's piece and
    /// progress into `active`, and selects player 1's playfield and button state.
    pub fn make_player1_active(&mut self) {
        self.vars.active_player = ACTIVE_PLAYER_1;
        self.set_playfield_page(PLAYFIELD_PAGE);
        self.vars.newly_pressed_buttons = self.vars.newly_pressed_buttons_player1;
        self.vars.held_buttons = self.vars.held_buttons_player1;
        self.vars.active = self.vars.player1.clone();
    }

    /// `makePlayer2Active`: make player 2 the active player. As `make_player1_active`, for
    /// player 2's copy, playfield and buttons.
    pub fn make_player2_active(&mut self) {
        self.vars.active_player = ACTIVE_PLAYER_2;
        self.set_playfield_page(PLAYFIELD_PAGE_SECOND_PLAYER);
        self.vars.newly_pressed_buttons = self.vars.newly_pressed_buttons_player2;
        self.vars.held_buttons = self.vars.held_buttons_player2;
        self.vars.active = self.vars.player2.clone();
    }

    /// Point `playfieldAddr` at a playfield page. Only the high byte changes; the low byte
    /// (the row offset) is kept.
    fn set_playfield_page(&mut self, page: u16) {
        let low = self.vars.playfield_addr.0 & 0x00FF;
        self.vars.playfield_addr = Le16(page | low);
    }

    /// `savePlayer1State`: store the active piece and progress back into player 1's copy. With
    /// two players, the pending garbage counts are swapped with the inactive player's.
    pub fn save_player1_state(&mut self) {
        self.vars.player1 = self.vars.active.clone();
        if self.vars.number_of_players != 1 {
            self.swap_pending_garbage();
        }
    }

    /// `savePlayer2State`: store the active piece and progress into player 2's copy, and swap
    /// the pending garbage counts (no check on the player count, unlike player 1).
    pub fn save_player2_state(&mut self) {
        self.vars.player2 = self.vars.active.clone();
        self.swap_pending_garbage();
    }

    /// Exchange `pendingGarbage` with `pendingGarbageInactivePlayer`.
    fn swap_pending_garbage(&mut self) {
        let v = &mut self.vars;
        std::mem::swap(&mut v.pending_garbage, &mut v.pending_garbage_inactive_player);
    }

    /// `playState_unassignOrientationId`: the piece is no longer on the board.
    pub fn play_state_unassign_orientation_id(&mut self) {
        self.vars.active.current_piece = HIDDEN_PIECE;
    }

    /// `playState_incrementPlayState`: advance the active piece's play state by one.
    pub fn play_state_increment_play_state(&mut self) {
        let p = &mut self.vars.active;
        p.play_state = p.play_state.wrapping_add(1);
    }

    /// `playState_noop`: a play state that does nothing.
    pub fn play_state_noop(&mut self) {}

    /// `gameModeState_updateCountersAndNonPlayerState`: once per frame in the game. Selects the
    /// game graphics, clears the sprite staging length, ages both players' fall timers, runs the
    /// two-player piece delay, and toggles the next-piece display on Select (player 1). Then
    /// advances the game mode state.
    pub fn game_mode_state_update_counters_and_non_player_state(&mut self) {
        self.set_chr_bank0(CHR_GAME);
        self.set_chr_bank1(CHR_GAME);
        self.vars.oam_staging_length = 0;
        // Both fall timers count up, whichever player is active.
        self.vars.player1.fall_timer = self.vars.player1.fall_timer.wrapping_add(1);
        self.vars.player2.fall_timer = self.vars.player2.fall_timer.wrapping_add(1);
        if self.vars.two_player_piece_delay_counter != 0 {
            self.vars.two_player_piece_delay_counter = self.vars.two_player_piece_delay_counter.wrapping_add(1);
        }
        if self.vars.newly_pressed_buttons_player1 & buttons::SELECT != 0 {
            self.vars.display_next_piece ^= 1;
        }
        self.vars.game_mode_state = self.vars.game_mode_state.wrapping_add(1);
    }
}

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "makePlayer1Active",
        addr: layout::MAKE_PLAYER1_ACTIVE,
        run: |g, _| {
            g.make_player1_active();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "makePlayer2Active",
        addr: layout::MAKE_PLAYER2_ACTIVE,
        run: |g, _| {
            g.make_player2_active();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "savePlayer1State",
        addr: layout::SAVE_PLAYER1_STATE,
        run: |g, _| {
            g.save_player1_state();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "savePlayer2State",
        addr: layout::SAVE_PLAYER2_STATE,
        run: |g, _| {
            g.save_player2_state();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "playState_unassignOrientationId",
        addr: layout::PLAY_STATE_UNASSIGN_ORIENTATION_ID,
        run: |g, _| {
            g.play_state_unassign_orientation_id();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "playState_incrementPlayState",
        addr: layout::PLAY_STATE_INCREMENT_PLAY_STATE,
        run: |g, _| {
            g.play_state_increment_play_state();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "playState_noop",
        addr: layout::PLAY_STATE_NOOP,
        run: |g, _| {
            g.play_state_noop();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "gameModeState_updateCountersAndNonPlayerState",
        addr: layout::GAME_MODE_STATE_UPDATE_COUNTERS_AND_NON_PLAYER_STATE,
        run: |g, _| {
            g.game_mode_state_update_counters_and_non_player_state();
            Ret::Unit
        },
        scratch: &[],
    },
];
