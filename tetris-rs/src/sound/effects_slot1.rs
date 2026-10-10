//! Sound effects in slot 1 (square 1): piece moves, line clears, level up, menus.
//!
//! Each effect has an init routine, run when the game requests it, and a playing routine, run on
//! later frames until it stops. Slot 1's counters are `sfx_*[1]`.

use super::SQ1;
use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

/// Frames per step of the effects that start from a constant (NTSC values from
/// `disasm/constants.asm`).
const SFX_LEVELUP_INIT: u8 = 0x06;
const SFX_LINE_COMPLETE_INIT: u8 = 0x05;
const SFX_LINECLEAR_INIT: u8 = 0x04;
const SFX_TETRIS_INIT: u8 = 0x05;
/// Square 1 register offsets from `SQ1`.
const SQ1_LO: u16 = SQ1 + 2;
const SQ1_HI: u16 = SQ1 + 3;
/// Written to `SQ1` to silence the channel: volume 0, constant-volume bit set.
const SQ1_SILENT: u8 = 0x10;
/// Written to `SQ1_HI` by the level-up and menu effects: timer high 0, length counter index 5.
const SQ1_HI_LENGTH: u8 = 0x28;
/// Effect numbers (1-based, in the slot 1 init table) that `LE33B` blocks a restart of the init
/// routine: tetris achieved, level up, line clearing, line completed.
const BLOCKING_EFFECTS: [u8; 4] = [0x04, 0x06, 0x09, 0x0A];
/// The effect number that `lineClearingPlaying` restarts as line clearing (`currentAudioSlot`).
const LINE_CLEARING_RESTART: u8 = 0x09;
/// Effect number of tetris achieved: when its sweep ends, line clearing restarts.
const TETRIS_EFFECT: u8 = 0x04;

/// The low byte of a data address, which the effect routines pass as `ldy #<data`.
fn data_lo(addr: u16) -> u8 {
    (addr & 0xFF) as u8
}

impl Game {
    /// `soundEffectSlot1_menuOptionSelectInit`: start the menu option blip (3 frames per step).
    pub fn sound_effect_slot1_menu_option_select_init(&mut self) {
        self.init_sound_effect_shared(3, data_lo(layout::SOUND_EFFECT_SLOT1_MENU_OPTION_SELECT_INIT_DATA));
    }

    /// `soundEffectSlot1_menuScreenSelectInit`: start the menu screen blip. Its counter starts
    /// from the third byte of the init data, not from zero.
    pub fn sound_effect_slot1_menu_screen_select_init(&mut self) {
        let data = layout::SOUND_EFFECT_SLOT1_MENU_SCREEN_SELECT_INIT_DATA;
        self.init_sound_effect_shared(3, data_lo(data));
        self.vars.sfx_secondary_counter[1] = self.rom.byte(data + 2);
    }

    /// `soundEffectSlot1_shiftTetriminoInit`: start the shift blip, unless the effect playing
    /// now is one that blocks it.
    pub fn sound_effect_slot1_shift_tetrimino_init(&mut self) {
        if self.slot1_effect_blocks_start() {
            return;
        }
        self.init_sound_effect_shared(2, data_lo(layout::SOUND_EFFECT_SLOT1_SHIFT_TETRIMINO_INIT_DATA));
    }

    /// `soundEffectSlot1_tetrisAchievedInit`: start the tetris jingle. Its tertiary counter
    /// starts at 0x10, offsetting its pitch-table index.
    pub fn sound_effect_slot1_tetris_achieved_init(&mut self) {
        self.init_sound_effect_shared(SFX_TETRIS_INIT, data_lo(layout::SOUND_EFFECT_SLOT1_TETRIS_ACHIEVED_INIT_DATA));
        self.vars.sfx_tertiary_counter[1] = 0x10;
    }

    /// `soundEffectSlot1_rotateTetriminoInit`: start the rotate blip, unless blocked, then run
    /// its first playing frame in the same call (the original falls through).
    pub fn sound_effect_slot1_rotate_tetrimino_init(&mut self) {
        if self.slot1_effect_blocks_start() {
            return;
        }
        self.init_sound_effect_shared(4, data_lo(layout::SOUND_EFFECT_SLOT1_ROTATE_TETRIMINO_INIT_DATA));
        self.sound_effect_slot1_rotate_tetrimino_playing();
    }

    /// `soundEffectSlot1_levelUpInit`: start the level-up jingle.
    pub fn sound_effect_slot1_level_up_init(&mut self) {
        self.init_sound_effect_shared(SFX_LEVELUP_INIT, data_lo(layout::SOUND_EFFECT_SLOT1_LEVEL_UP_INIT_DATA));
    }

    /// `soundEffectSlot1_lockTetriminoInit`: start the lock thud, unless blocked.
    pub fn sound_effect_slot1_lock_tetrimino_init(&mut self) {
        if self.slot1_effect_blocks_start() {
            return;
        }
        self.init_sound_effect_shared(0x0F, data_lo(layout::SOUND_EFFECT_SLOT1_LOCK_TETRIMINO_INIT_DATA));
    }

    /// `soundEffectSlot1_chirpChirpInit`: unused by the game. The original never loads A here,
    /// so the frame count is whatever A holds on entry: the effect number, which
    /// `computeSoundEffMethod` leaves in A (and in `current_audio_slot`).
    pub fn sound_effect_slot1_chirp_chirp_init(&mut self) {
        let frames = self.vars.current_audio_slot;
        self.init_sound_effect_shared(frames, data_lo(layout::SOUND_EFFECT_SLOT1_CHIRP_CHIRP_INIT_DATA));
    }

    /// `soundEffectSlot1_lineClearingInit`: start the line-clearing sweep from its first step.
    pub fn sound_effect_slot1_line_clearing_init(&mut self) {
        self.init_sound_effect_shared(SFX_LINECLEAR_INIT, data_lo(layout::SOUND_EFFECT_SLOT1_LINE_CLEARING_INIT_DATA));
        self.vars.sfx_tertiary_counter[1] = 0;
    }

    /// `soundEffectSlot1_lineCompletedInit`: start the line-completed jingle. Its tertiary counter
    /// starts at 0x08, offsetting its pitch-table index.
    pub fn sound_effect_slot1_line_completed_init(&mut self) {
        self.init_sound_effect_shared(SFX_LINE_COMPLETE_INIT, data_lo(layout::SOUND_EFFECT_SLOT1_LINE_COMPLETED_INIT_DATA));
        self.vars.sfx_tertiary_counter[1] = 0x08;
    }

    /// `soundEffectSlot1_menuOptionSelectPlaying`: the menu blip's two steps, then stop.
    pub fn sound_effect_slot1_menu_option_select_playing(&mut self) {
        if !self.advance_audio_slot_frame() {
            return;
        }
        let step = self.vars.sfx_secondary_counter[1].wrapping_add(1);
        self.vars.sfx_secondary_counter[1] = step;
        if step == 2 {
            self.stop_sound_effect_slot1();
        } else {
            self.copy_to_apu_channel(SQ1, data_lo(layout::SOUND_EFFECT_SLOT1_PLAYING_MENU_OPTION_SELECT_STAGE2));
        }
    }

    /// `soundEffectSlot1_menuScreenSelectPlaying`: sweep the pitch down each step, four steps
    /// then stop. The pitch's low byte drops by its high nibble plus one: the original clears
    /// carry before `sbc`, so the subtraction borrows.
    pub fn sound_effect_slot1_menu_screen_select_playing(&mut self) {
        if !self.advance_audio_slot_frame() {
            return;
        }
        let steps = self.vars.sfx_tertiary_counter[1].wrapping_add(1);
        self.vars.sfx_tertiary_counter[1] = steps;
        if steps == 4 {
            self.stop_sound_effect_slot1();
            return;
        }
        let pitch = self.vars.sfx_secondary_counter[1];
        // Kept in `sfx_tmp[1]`, as the original does; the playing routine never reads it back.
        let high_nibble = pitch >> 4;
        self.vars.sfx_tmp[1] = high_nibble;
        let pitch = pitch.wrapping_sub(high_nibble).wrapping_sub(1);
        self.vars.sfx_secondary_counter[1] = pitch;
        self.apu_write(SQ1_LO, pitch);
        self.apu_write(SQ1_HI, SQ1_HI_LENGTH);
    }

    /// `soundEffectSlot1Playing_advance`: count a frame of an effect that has no steps of its own,
    /// and stop it when its frame count is reached.
    pub fn sound_effect_slot1_playing_advance(&mut self) {
        if !self.advance_audio_slot_frame() {
            return;
        }
        self.stop_sound_effect_slot1();
    }

    /// `soundEffectSlot1_tetrisAchievedPlaying`: the tetris jingle's steps (the shared line
    /// effect tail, with the tetris data).
    pub fn sound_effect_slot1_tetris_achieved_playing(&mut self) {
        if !self.advance_audio_slot_frame() {
            return;
        }
        self.line_effect_step(layout::SOUND_EFFECT_SLOT1_TETRIS_ACHIEVED_INIT_DATA);
    }

    /// `soundEffectSlot1_rotateTetriminoPlaying`: the rotate blip's four stages, then stop.
    pub fn sound_effect_slot1_rotate_tetrimino_playing(&mut self) {
        if !self.advance_audio_slot_frame() {
            return;
        }
        let stage = self.vars.sfx_secondary_counter[1];
        self.vars.sfx_secondary_counter[1] = stage.wrapping_add(1);
        match stage {
            // 0xFF wraps to 0 when incremented and takes the `beq` to stage 3, as the original does.
            0xFF | 2 => self.copy_to_apu_channel(SQ1, data_lo(layout::SOUND_EFFECT_SLOT1_PLAYING_ROTATE_TETRIMINO_STAGE3)),
            1 => self.copy_to_apu_channel(SQ1, data_lo(layout::SOUND_EFFECT_SLOT1_ROTATE_TETRIMINO_INIT_DATA)),
            3 => self.stop_sound_effect_slot1(),
            _ => {}
        }
    }

    /// `soundEffectSlot1_levelUpPlaying`: step through the level-up pitch table; a zero entry
    /// ends the effect.
    pub fn sound_effect_slot1_level_up_playing(&mut self) {
        if !self.advance_audio_slot_frame() {
            return;
        }
        let index = self.vars.sfx_secondary_counter[1];
        self.vars.sfx_secondary_counter[1] = index.wrapping_add(1);
        let pitch_lo = self.rom.byte(layout::SOUND_EFFECT_SLOT1_LEVEL_UP_LO + index as u16);
        if pitch_lo == 0 {
            self.stop_sound_effect_slot1();
            return;
        }
        self.apu_write(SQ1_LO, pitch_lo);
        self.apu_write(SQ1_HI, SQ1_HI_LENGTH);
    }

    /// `soundEffectSlot1_chirpChirpPlaying`: unused by the game. Runs every frame (no
    /// `advance_audio_slot_frame`). Eight frames of volume blips, then a second stage, then stop
    /// after 0x16 frames of the second stage.
    pub fn sound_effect_slot1_chirp_chirp_playing(&mut self) {
        if self.vars.sfx_tertiary_counter[1] != 0 {
            let count = self.vars.sfx_secondary_counter[1].wrapping_add(1);
            self.vars.sfx_secondary_counter[1] = count;
            if count == 0x16 {
                self.stop_sound_effect_slot1();
            }
            return;
        }
        let index = self.vars.sfx_secondary_counter[1] & 0x03;
        let volume = self.rom.byte(layout::SOUND_EFFECT_SLOT1_CHIRP_CHIRP_SQ1_VOL_TABLE + index as u16);
        self.apu_write(SQ1, volume);
        let count = self.vars.sfx_secondary_counter[1].wrapping_add(1);
        self.vars.sfx_secondary_counter[1] = count;
        if count == 8 {
            self.vars.sfx_tertiary_counter[1] = self.vars.sfx_tertiary_counter[1].wrapping_add(1);
            self.copy_to_apu_channel(SQ1, data_lo(layout::SOUND_EFFECT_SLOT1_PLAYING_CHIRP_CHIRP_STAGE2));
        }
    }

    /// `soundEffectSlot1_lineClearingPlaying`: the line-clearing sweep's steps.
    pub fn sound_effect_slot1_line_clearing_playing(&mut self) {
        if !self.advance_audio_slot_frame() {
            return;
        }
        self.line_effect_step(layout::SOUND_EFFECT_SLOT1_LINE_CLEARING_INIT_DATA);
    }

    /// `soundEffectSlot1_lineCompletedPlaying`: the line-completed jingle's steps.
    pub fn sound_effect_slot1_line_completed_playing(&mut self) {
        if !self.advance_audio_slot_frame() {
            return;
        }
        self.line_effect_step(layout::SOUND_EFFECT_SLOT1_LINE_COMPLETED_INIT_DATA);
    }

    // ---- helpers shared by the slot 1 effects ----

    /// `LE33B`: whether the effect playing now blocks starting another one.
    fn slot1_effect_blocks_start(&self) -> bool {
        BLOCKING_EFFECTS.contains(&self.vars.sfx_playing[1])
    }

    /// `soundEffectSlot1Playing_stop`: silence square 1, give it back to the music, and end the
    /// effect.
    fn stop_sound_effect_slot1(&mut self) {
        self.apu_write(SQ1, SQ1_SILENT);
        self.vars.channel_inhibit[1] = 0;
        self.vars.sfx_playing[1] = 0;
        // The music re-sets square 1 when this is nonzero; the original increments it.
        self.vars.reset_sq12_for_music = self.vars.reset_sq12_for_music.wrapping_add(1);
    }

    /// One step of the line-clearing sweep (`LE442`), with the effect's init data at `data`.
    /// The pitch index is the tertiary counter plus the secondary one, in 8 bits. A zero volume
    /// entry ends the sweep: the line-clearing effect restarts itself when the effect playing is
    /// number 4, otherwise it stops.
    fn line_effect_step(&mut self, data: u16) {
        self.copy_to_apu_channel(SQ1, data_lo(data));
        let index = self.vars.sfx_tertiary_counter[1].wrapping_add(self.vars.sfx_secondary_counter[1]);
        let pitch_lo = self.rom.byte(layout::SOUND_EFFECT_SLOT1_LINE_CLEARING_LO + index as u16);
        self.apu_write(SQ1_LO, pitch_lo);
        let step = self.vars.sfx_secondary_counter[1];
        let volume = self.rom.byte(layout::SOUND_EFFECT_SLOT1_LINE_CLEARING_VOL + step as u16);
        self.apu_write(SQ1, volume);
        if volume != 0 {
            self.vars.sfx_secondary_counter[1] = step.wrapping_add(1);
        } else if self.vars.sfx_playing[1] == TETRIS_EFFECT {
            self.vars.current_audio_slot = LINE_CLEARING_RESTART;
            self.sound_effect_slot1_line_clearing_init();
        } else {
            self.stop_sound_effect_slot1();
        }
    }
}

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "soundEffectSlot1_menuOptionSelectInit",
        addr: layout::SOUND_EFFECT_SLOT1_MENU_OPTION_SELECT_INIT,
        run: |g, _| {
            g.sound_effect_slot1_menu_option_select_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_menuScreenSelectInit",
        addr: layout::SOUND_EFFECT_SLOT1_MENU_SCREEN_SELECT_INIT,
        run: |g, _| {
            g.sound_effect_slot1_menu_screen_select_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_shiftTetriminoInit",
        addr: layout::SOUND_EFFECT_SLOT1_SHIFT_TETRIMINO_INIT,
        run: |g, _| {
            g.sound_effect_slot1_shift_tetrimino_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_tetrisAchievedInit",
        addr: layout::SOUND_EFFECT_SLOT1_TETRIS_ACHIEVED_INIT,
        run: |g, _| {
            g.sound_effect_slot1_tetris_achieved_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_rotateTetriminoInit",
        addr: layout::SOUND_EFFECT_SLOT1_ROTATE_TETRIMINO_INIT,
        run: |g, _| {
            g.sound_effect_slot1_rotate_tetrimino_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_levelUpInit",
        addr: layout::SOUND_EFFECT_SLOT1_LEVEL_UP_INIT,
        run: |g, _| {
            g.sound_effect_slot1_level_up_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_lockTetriminoInit",
        addr: layout::SOUND_EFFECT_SLOT1_LOCK_TETRIMINO_INIT,
        run: |g, _| {
            g.sound_effect_slot1_lock_tetrimino_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_chirpChirpInit",
        addr: layout::SOUND_EFFECT_SLOT1_CHIRP_CHIRP_INIT,
        run: |g, _| {
            g.sound_effect_slot1_chirp_chirp_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_lineClearingInit",
        addr: layout::SOUND_EFFECT_SLOT1_LINE_CLEARING_INIT,
        run: |g, _| {
            g.sound_effect_slot1_line_clearing_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_lineCompletedInit",
        addr: layout::SOUND_EFFECT_SLOT1_LINE_COMPLETED_INIT,
        run: |g, _| {
            g.sound_effect_slot1_line_completed_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_menuOptionSelectPlaying",
        addr: layout::SOUND_EFFECT_SLOT1_MENU_OPTION_SELECT_PLAYING,
        run: |g, _| {
            g.sound_effect_slot1_menu_option_select_playing();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_menuScreenSelectPlaying",
        addr: layout::SOUND_EFFECT_SLOT1_MENU_SCREEN_SELECT_PLAYING,
        run: |g, _| {
            g.sound_effect_slot1_menu_screen_select_playing();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1Playing_advance",
        addr: layout::SOUND_EFFECT_SLOT1_PLAYING_ADVANCE,
        run: |g, _| {
            g.sound_effect_slot1_playing_advance();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_tetrisAchievedPlaying",
        addr: layout::SOUND_EFFECT_SLOT1_TETRIS_ACHIEVED_PLAYING,
        run: |g, _| {
            g.sound_effect_slot1_tetris_achieved_playing();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_rotateTetriminoPlaying",
        addr: layout::SOUND_EFFECT_SLOT1_ROTATE_TETRIMINO_PLAYING,
        run: |g, _| {
            g.sound_effect_slot1_rotate_tetrimino_playing();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_levelUpPlaying",
        addr: layout::SOUND_EFFECT_SLOT1_LEVEL_UP_PLAYING,
        run: |g, _| {
            g.sound_effect_slot1_level_up_playing();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_chirpChirpPlaying",
        addr: layout::SOUND_EFFECT_SLOT1_CHIRP_CHIRP_PLAYING,
        run: |g, _| {
            g.sound_effect_slot1_chirp_chirp_playing();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_lineClearingPlaying",
        addr: layout::SOUND_EFFECT_SLOT1_LINE_CLEARING_PLAYING,
        run: |g, _| {
            g.sound_effect_slot1_line_clearing_playing();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot1_lineCompletedPlaying",
        addr: layout::SOUND_EFFECT_SLOT1_LINE_COMPLETED_PLAYING,
        run: |g, _| {
            g.sound_effect_slot1_line_completed_playing();
            Ret::Unit
        },
        scratch: &[],
    },
];
