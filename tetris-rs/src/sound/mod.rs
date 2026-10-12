//! The sound engine: once a frame it runs each sound-effect slot, then the music.
//!
//! Game code requests an effect by writing its number to `sfx_init[slot]`. The engine looks the
//! number up in that slot's ROM table of routines and runs the effect's init routine; on later
//! frames it runs the effect's "playing" routine from a second table until the effect stops.
//! Slots: 0 noise (game over, rocket), 1 square 1 (moves, line clears, menus), 2 DMC buzzes and
//! "silence", 3 triangle (ending effects). Effects take a channel from the music by setting
//! `channel_inhibit`.

pub(crate) mod effects_other;
pub(crate) mod effects_slot1;
pub(crate) mod music;

use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

/// Effect channel data (4 APU register values per entry) lives on this ROM page; routines
/// address it by the low byte.
pub(crate) const DATA_PAGE: u16 = 0xE100;
/// APU channel register bases.
pub(crate) const SQ1: u16 = 0x4000;
pub(crate) const SQ2: u16 = 0x4004;
pub(crate) const TRI: u16 = 0x4008;
pub(crate) const NOISE: u16 = 0x400C;
const DMC_RAW: u16 = 0x4011;
const SND_CHN: u16 = 0x4015;
const APU_FRAME_COUNTER: u16 = 0x4017;
/// `musicStagingNoiseHi` holds this while the game is paused.
pub(crate) const PAUSED: u8 = 0x05;
/// What `computeSoundEffMethod` falls back to for an effect number past the table: an `rts`.
const NO_ROUTINE: u16 = 0xE091;

impl Game {
    /// `updateAudio`: once a frame. While paused, play the pause jingle instead.
    pub fn update_audio(&mut self) {
        // Disable the APU frame interrupt.
        self.apu_write(APU_FRAME_COUNTER, 0xC0);
        if self.vars.music_staging_noise_hi == PAUSED {
            self.update_audio_pause();
            return;
        }
        self.vars.audio_initialized = 0;
        self.vars.music_pause_sound_effect_counter = 0;
        self.update_sound_effect_slot2();
        self.update_sound_effect_slot0();
        self.update_sound_effect_slot3();
        self.update_sound_effect_slot1();
        self.update_music();
        // Requests are consumed. The original's loop clears $06F0..=$06F5, which takes the
        // music track request with it.
        self.vars.sfx_init = [0; 5];
        self.vars.music_track = 0;
    }

    /// `updateAudio_pause`: on the first paused frame silence everything; then alternate two
    /// square-wave blips every 4 frames until 18 frames have passed.
    fn update_audio_pause(&mut self) {
        if self.vars.audio_initialized == 0 {
            // initAudioAndMarkInited
            self.vars.audio_initialized = self.vars.audio_initialized.wrapping_add(1);
            self.mute_audio();
            self.vars.music_pause_sound_effect_length_counter = 0;
            return;
        }
        let length = self.vars.music_pause_sound_effect_length_counter;
        if length == 0x12 {
            return;
        }
        if length & 0x03 == 0x03 {
            self.vars.music_pause_sound_effect_counter = self.vars.music_pause_sound_effect_counter.wrapping_add(1);
            let blip = if self.vars.music_pause_sound_effect_counter & 1 != 0 {
                layout::MUSIC_PAUSE_SQ1_ODD
            } else {
                layout::MUSIC_PAUSE_SQ1_EVEN
            };
            self.copy_to_apu_channel(SQ1, blip as u8);
        }
        self.vars.music_pause_sound_effect_length_counter = length.wrapping_add(1);
    }

    /// `updateAudio2`: stop all sound (it's `soundEffectSlot2_makesNoSound`).
    pub fn update_audio2(&mut self) {
        self.sound_effect_slot2_makes_no_sound();
    }

    /// Unlabelled routine at $E1D8 (reached through `LE006`): enable the four tone channels,
    /// seed the sound RNG, and stop all sound.
    pub fn init_audio(&mut self) {
        self.apu_write(SND_CHN, 0x0F);
        // `sta soundRngSeed` writes the seed's first byte.
        self.vars.sound_rng_seed.0 = (self.vars.sound_rng_seed.0 & 0x00FF) | 0x5500;
        self.sound_effect_slot2_makes_no_sound();
    }

    fn update_sound_effect_slot0(&mut self) {
        self.update_sound_effect_slot(0, layout::SOUND_EFFECT_SLOT0_INIT_TABLE, layout::SOUND_EFFECT_SLOT0_PLAYING_TABLE);
    }

    /// Slot 1 yields while slot 4 (unused) plays.
    fn update_sound_effect_slot1(&mut self) {
        if self.vars.sfx_playing[4] != 0 {
            return;
        }
        self.update_sound_effect_slot(1, layout::SOUND_EFFECT_SLOT1_INIT_TABLE, layout::SOUND_EFFECT_SLOT1_PLAYING_TABLE);
    }

    /// Slot 2 has no "playing" routines: its init table serves for both.
    fn update_sound_effect_slot2(&mut self) {
        self.update_sound_effect_slot(2, layout::SOUND_EFFECT_SLOT2_INIT_TABLE, layout::SOUND_EFFECT_SLOT2_INIT_TABLE);
    }

    fn update_sound_effect_slot3(&mut self) {
        self.update_sound_effect_slot(3, layout::SOUND_EFFECT_SLOT3_INIT_TABLE, layout::SOUND_EFFECT_SLOT3_PLAYING_TABLE);
    }

    /// `updateSoundEffectSlotShared`: start the requested effect, or else continue the one
    /// playing.
    fn update_sound_effect_slot(&mut self, slot: u8, init_table: u16, playing_table: u16) {
        self.vars.current_sound_effect_slot = slot;
        let (table, effect) = match (self.vars.sfx_init[slot as usize], self.vars.sfx_playing[slot as usize]) {
            (0, 0) => return,
            (0, playing) => (playing_table, playing),
            (requested, _) => (init_table, requested),
        };
        let routine = self.compute_sound_eff_method(table, effect);
        self.call_sound_routine(routine);
    }

    /// `computeSoundEffMethod`: the routine for 1-based `effect` in a table of 17 at most.
    /// Sets `current_audio_slot` to the effect number for the routine to use.
    fn compute_sound_eff_method(&mut self, table: u16, effect: u8) -> u16 {
        self.vars.current_audio_slot = effect;
        let index = effect.wrapping_sub(1);
        if index < 17 {
            self.rom.word(table + index as u16 * 2)
        } else {
            NO_ROUTINE
        }
    }

    /// Run the sound routine at `addr` (an entry of one of the ROM's routine tables).
    fn call_sound_routine(&mut self, addr: u16) {
        use layout::*;
        match addr {
            SOUND_EFFECT_SLOT0_MAKES_NO_SOUND => self.sound_effect_slot0_makes_no_sound(),
            SOUND_EFFECT_SLOT0_GAME_OVER_CURTAIN_INIT => self.sound_effect_slot0_game_over_curtain_init(),
            SOUND_EFFECT_SLOT0_ENDING_ROCKET_INIT => self.sound_effect_slot0_ending_rocket_init(),
            ADVANCE_SOUND_EFFECT_SLOT0_WITHOUT_UPDATE => self.advance_sound_effect_slot0_without_update(),
            UPDATE_SOUND_EFFECT_SLOT0_APU => self.update_sound_effect_slot0_apu(),
            SOUND_EFFECT_SLOT1_MENU_OPTION_SELECT_INIT => self.sound_effect_slot1_menu_option_select_init(),
            SOUND_EFFECT_SLOT1_MENU_SCREEN_SELECT_INIT => self.sound_effect_slot1_menu_screen_select_init(),
            SOUND_EFFECT_SLOT1_SHIFT_TETRIMINO_INIT => self.sound_effect_slot1_shift_tetrimino_init(),
            SOUND_EFFECT_SLOT1_TETRIS_ACHIEVED_INIT => self.sound_effect_slot1_tetris_achieved_init(),
            SOUND_EFFECT_SLOT1_ROTATE_TETRIMINO_INIT => self.sound_effect_slot1_rotate_tetrimino_init(),
            SOUND_EFFECT_SLOT1_LEVEL_UP_INIT => self.sound_effect_slot1_level_up_init(),
            SOUND_EFFECT_SLOT1_LOCK_TETRIMINO_INIT => self.sound_effect_slot1_lock_tetrimino_init(),
            SOUND_EFFECT_SLOT1_CHIRP_CHIRP_INIT => self.sound_effect_slot1_chirp_chirp_init(),
            SOUND_EFFECT_SLOT1_LINE_CLEARING_INIT => self.sound_effect_slot1_line_clearing_init(),
            SOUND_EFFECT_SLOT1_LINE_COMPLETED_INIT => self.sound_effect_slot1_line_completed_init(),
            SOUND_EFFECT_SLOT1_MENU_OPTION_SELECT_PLAYING => self.sound_effect_slot1_menu_option_select_playing(),
            SOUND_EFFECT_SLOT1_MENU_SCREEN_SELECT_PLAYING => self.sound_effect_slot1_menu_screen_select_playing(),
            SOUND_EFFECT_SLOT1_PLAYING_ADVANCE => self.sound_effect_slot1_playing_advance(),
            SOUND_EFFECT_SLOT1_TETRIS_ACHIEVED_PLAYING => self.sound_effect_slot1_tetris_achieved_playing(),
            SOUND_EFFECT_SLOT1_ROTATE_TETRIMINO_PLAYING => self.sound_effect_slot1_rotate_tetrimino_playing(),
            SOUND_EFFECT_SLOT1_LEVEL_UP_PLAYING => self.sound_effect_slot1_level_up_playing(),
            SOUND_EFFECT_SLOT1_CHIRP_CHIRP_PLAYING => self.sound_effect_slot1_chirp_chirp_playing(),
            SOUND_EFFECT_SLOT1_LINE_CLEARING_PLAYING => self.sound_effect_slot1_line_clearing_playing(),
            SOUND_EFFECT_SLOT1_LINE_COMPLETED_PLAYING => self.sound_effect_slot1_line_completed_playing(),
            SOUND_EFFECT_SLOT3_FALLING_ALIEN => self.sound_effect_slot3_falling_alien(),
            SOUND_EFFECT_SLOT3_DONK => self.sound_effect_slot3_donk(),
            UPDATE_SOUND_EFFECT_SLOT3_APU => self.update_sound_effect_slot3_apu(),
            SOUND_EFFECT_SLOT3_PLAYING_ADVANCE => self.sound_effect_slot3_playing_advance(),
            SOUND_EFFECT_SLOT2_MAKES_NO_SOUND => self.sound_effect_slot2_makes_no_sound(),
            SOUND_EFFECT_SLOT2_LOW_BUZZ => self.sound_effect_slot2_low_buzz(),
            SOUND_EFFECT_SLOT2_MEDIUM_BUZZ => self.sound_effect_slot2_medium_buzz(),
            NO_ROUTINE => {}
            _ => panic!("no sound routine at {addr:04X}"),
        }
    }

    // ---- helpers shared by the effects ----

    /// `copyToApuChannel`: write 4 bytes of effect data (`DATA_PAGE` + `data`) to a channel's
    /// registers.
    pub(crate) fn copy_to_apu_channel(&mut self, channel: u16, data: u8) {
        for i in 0..4 {
            let v = self.rom.byte(DATA_PAGE + data as u16 + i);
            self.apu_write(channel + i, v);
        }
    }

    /// `advanceAudioSlotFrame`: count a frame of the current slot's effect. True once every
    /// `sfx_frame_count` frames (the counter wraps to 0), which is when effects take their
    /// next step; the original returns this as Z set.
    pub(crate) fn advance_audio_slot_frame(&mut self) -> bool {
        let slot = self.vars.current_sound_effect_slot as usize;
        self.vars.sfx_frame_counter[slot] = self.vars.sfx_frame_counter[slot].wrapping_add(1);
        if self.vars.sfx_frame_counter[slot] != self.vars.sfx_frame_count[slot] {
            return false;
        }
        self.vars.sfx_frame_counter[slot] = 0;
        true
    }

    /// `initSoundEffectShared`: start the effect numbered `current_audio_slot` in the current
    /// slot: `frames` per step, the slot's channel taken from the music, and (if `data` isn't 0)
    /// the channel's registers loaded from effect data.
    pub(crate) fn init_sound_effect_shared(&mut self, frames: u8, data: u8) {
        let slot = self.vars.current_sound_effect_slot as usize;
        self.vars.sfx_frame_count[slot] = frames;
        self.vars.channel_inhibit[slot] = slot as u8;
        if data != 0 {
            match slot {
                0 => self.copy_to_apu_channel(NOISE, data),
                1 => self.copy_to_apu_channel(SQ1, data),
                2 => self.copy_to_apu_channel(SQ2, data),
                3 => self.copy_to_apu_channel(TRI, data),
                // Slot 4 never has data; the original returns without starting it.
                _ => return,
            }
        }
        self.vars.sfx_playing[slot] = self.vars.current_audio_slot;
        self.vars.sfx_frame_counter[slot] = 0;
        self.vars.sfx_secondary_counter[slot] = 0;
        self.vars.sfx_tertiary_counter[slot] = 0;
        self.vars.sfx_tmp[slot] = 0;
        self.vars.reset_sq12_for_music = 0;
    }

    /// `muteAudio`: silence every channel (volume 0, triangle halted, DMC output 0).
    pub(crate) fn mute_audio(&mut self) {
        self.apu_write(DMC_RAW, 0);
        self.apu_write(SQ1, 0x10);
        self.apu_write(SQ2, 0x10);
        self.apu_write(NOISE, 0x10);
        self.apu_write(TRI, 0);
    }

    /// `soundEffectSlot2_makesNoSound`: stop every effect, give all channels back to the
    /// music, and silence the APU.
    pub fn sound_effect_slot2_makes_no_sound(&mut self) {
        // LE253
        self.vars.channel_inhibit[1..4].fill(0);
        self.vars.music_staging_noise_lo = 0;
        self.vars.reset_sq12_for_music = 0;
        // The original clears $06F8..=$06FD: every slot's effect and the playing music track.
        self.vars.sfx_playing = [0; 5];
        self.vars.currently_playing_music_track = 0;
        // muteAudioAndClearTriControl
        self.mute_audio();
        self.apu_write(DMC_RAW, 0);
        self.vars.music_chan_control[2] = 0;
    }
}

pub const SHADOW: &[Routine] = &[
    // The game reaches updateAudio with `jmp` from updateAudio_jmp, so that's the entry called.
    Routine {
        name: "updateAudio_jmp",
        addr: layout::UPDATE_AUDIO_JMP,
        run: |g, _| {
            g.update_audio();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "updateAudio2",
        addr: layout::UPDATE_AUDIO2,
        run: |g, _| {
            g.update_audio2();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "LE006",
        addr: layout::LE006,
        run: |g, _| {
            g.init_audio();
            Ret::Unit
        },
        scratch: &[],
    },
];
