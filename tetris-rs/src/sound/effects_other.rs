//! Sound effects in slots 0 (noise), 2 (DMC) and 3 (triangle).

use super::{DATA_PAGE, DMC_RAW, NOISE, SND_CHN, TRI};
use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

/// Noise channel registers: volume/envelope, then period.
const NOISE_VOL: u16 = NOISE;
const NOISE_LO: u16 = NOISE + 2;
/// Noise volume with the envelope's constant-volume bit set.
const NOISE_CONSTANT_VOLUME: u8 = 0x10;
/// Triangle channel registers: linear counter, period low, period high.
const TRI_LINEAR: u16 = TRI;
const TRI_LO: u16 = TRI + 2;
const TRI_HI: u16 = TRI + 3;
/// Written to `TRI_HI` when a triangle effect stops (a length-counter load).
const TRI_HI_STOPPED: u8 = 0x18;
/// DMC channel registers.
const DMC_FREQ: u16 = 0x4010;
const DMC_START: u16 = 0x4012;
const DMC_LEN: u16 = 0x4013;
/// `$4015` bit 4 enables the DMC: writing it clear stops the DMC, writing it set starts it.
const SND_CHN_DMC_OFF: u8 = 0x0F;
const SND_CHN_DMC_ON: u8 = 0x1F;

/// Frames per step of each effect, as passed to `init_sound_effect_shared`.
const NOISE_SILENT_FRAMES: u8 = 0x10;
const NOISE_CURTAIN_FRAMES: u8 = 0x40;
const NOISE_ROCKET_FRAMES: u8 = 0x20;
const TRI_ALIEN_FRAMES: u8 = 0x06;
const TRI_DONK_FRAMES: u8 = 0x02;

/// Offsets into the falling-alien init data: the tertiary counter's start value, and the
/// triangle high byte written on every step.
const ALIEN_TERTIARY_OFFSET: u16 = 2;
const ALIEN_HI_OFFSET: u16 = 3;

impl Game {
    /// `soundEffectSlot0_makesNoSound`: a silent noise effect for a few frames.
    pub fn sound_effect_slot0_makes_no_sound(&mut self) {
        self.init_sound_effect_shared(NOISE_SILENT_FRAMES, 0);
    }

    /// `soundEffectSlot0_gameOverCurtainInit`: start the game-over curtain noise.
    pub fn sound_effect_slot0_game_over_curtain_init(&mut self) {
        self.init_sound_effect_shared(NOISE_CURTAIN_FRAMES, (layout::SOUND_EFFECT_SLOT0_GAME_OVER_CURTAIN_INIT_DATA & 0xFF) as u8);
    }

    /// `soundEffectSlot0_endingRocketInit`: start the ending rocket noise.
    pub fn sound_effect_slot0_ending_rocket_init(&mut self) {
        self.init_sound_effect_shared(NOISE_ROCKET_FRAMES, (layout::SOUND_EFFECT_SLOT0_ENDING_ROCKET_INIT_DATA & 0xFF) as u8);
    }

    /// `advanceSoundEffectSlot0WithoutUpdate`: count a frame of the noise effect, and stop it when
    /// its last step falls on this frame. Otherwise the APU is left alone.
    pub fn advance_sound_effect_slot0_without_update(&mut self) {
        if self.advance_audio_slot_frame() {
            self.stop_sound_effect_slot0();
        }
    }

    /// `stopSoundEffectSlot0`: end the noise effect and silence the noise volume.
    fn stop_sound_effect_slot0(&mut self) {
        self.vars.sfx_playing[0] = 0;
        self.apu_write(NOISE_VOL, NOISE_CONSTANT_VOLUME);
    }

    /// `updateSoundEffectSlot0_apu`: play the next frame of the noise effect, or stop it when its
    /// steps run out.
    pub fn update_sound_effect_slot0_apu(&mut self) {
        if self.advance_audio_slot_frame() {
            self.stop_sound_effect_slot0();
        } else {
            self.update_sound_effect_noise_audio();
        }
    }

    /// `updateSoundEffectNoiseAudio`: set the noise period and volume for the current step, then
    /// move to the next step.
    fn update_sound_effect_noise_audio(&mut self) {
        let period = self.get_sound_effect_noise_nibble(layout::NOISELO_TABLE);
        self.apu_write(NOISE_LO, period);
        let volume = self.get_sound_effect_noise_nibble(layout::NOISEVOL_TABLE);
        self.apu_write(NOISE_VOL, volume | NOISE_CONSTANT_VOLUME);
        self.vars.sfx_secondary_counter[0] = self.vars.sfx_secondary_counter[0].wrapping_add(1);
    }

    /// `getSoundEffectNoiseNibble`: the nibble of `table` for the current step. Two steps share a
    /// byte: an even step takes the high nibble, an odd step the low one.
    ///
    /// The original builds the pointer from the table's low byte and always the `DATA_PAGE` high
    /// byte, so the byte read is at `DATA_PAGE + low + index`, with the index 8-bit.
    fn get_sound_effect_noise_nibble(&self, table: u16) -> u8 {
        let step = self.vars.sfx_secondary_counter[0];
        let address = DATA_PAGE + (table & 0xFF) + (step >> 1) as u16;
        let byte = self.rom.byte(address);
        if step & 1 == 0 {
            byte >> 4
        } else {
            byte & 0x0F
        }
    }

    /// `soundEffectSlot3_fallingAlien` (unused): start the falling-alien triangle effect, and load
    /// its tertiary counter from the data.
    pub fn sound_effect_slot3_falling_alien(&mut self) {
        self.init_sound_effect_shared(TRI_ALIEN_FRAMES, (layout::SOUND_EFFECT_SLOT3_FALLING_ALIEN_INIT_DATA & 0xFF) as u8);
        self.vars.sfx_tertiary_counter[3] = self.rom.byte(layout::SOUND_EFFECT_SLOT3_FALLING_ALIEN_INIT_DATA + ALIEN_TERTIARY_OFFSET);
    }

    /// `soundEffectSlot3_donk` (unused): start the donk triangle effect.
    pub fn sound_effect_slot3_donk(&mut self) {
        self.init_sound_effect_shared(TRI_DONK_FRAMES, (layout::SOUND_EFFECT_SLOT3_DONK_INIT_DATA & 0xFF) as u8);
    }

    /// `updateSoundEffectSlot3_apu`: play the next period of the falling-alien effect from
    /// `TRILO_TABLE`, or stop when the table's zero terminator is reached.
    pub fn update_sound_effect_slot3_apu(&mut self) {
        if !self.advance_audio_slot_frame() {
            return;
        }
        let step = self.vars.sfx_secondary_counter[3];
        self.vars.sfx_secondary_counter[3] = step.wrapping_add(1);
        let period = self.rom.byte(layout::TRILO_TABLE + step as u16);
        if period == 0 {
            self.stop_sound_effect_slot3();
            return;
        }
        self.apu_write(TRI_LO, period);
        self.vars.sfx_tertiary_counter[3] = period;
        self.apu_write(TRI_HI, self.rom.byte(layout::SOUND_EFFECT_SLOT3_FALLING_ALIEN_INIT_DATA + ALIEN_HI_OFFSET));
    }

    /// `soundEffectSlot3Playing_advance`: count a frame of the triangle effect, and stop it when
    /// its last step falls on this frame.
    pub fn sound_effect_slot3_playing_advance(&mut self) {
        if self.advance_audio_slot_frame() {
            self.stop_sound_effect_slot3();
        }
    }

    /// `soundEffectSlot3Playing_stop`: end the triangle effect, return the triangle to the music,
    /// and silence it.
    fn stop_sound_effect_slot3(&mut self) {
        self.apu_write(TRI_LINEAR, 0);
        self.vars.channel_inhibit[3] = 0;
        self.vars.sfx_playing[3] = 0;
        self.apu_write(TRI_HI, TRI_HI_STOPPED);
    }

    /// `soundEffectSlot2_lowBuzz` (unused): a DMC buzz at the lower rate.
    pub fn sound_effect_slot2_low_buzz(&mut self) {
        self.dmc_buzz(0x3F, 0x60, 0x0E);
    }

    /// `soundEffectSlot2_mediumBuzz` (unused): the same buzz at the next rate. Its three loads
    /// leave Z clear, so the `bne LE51B` is always taken and the flags at entry don't matter.
    pub fn sound_effect_slot2_medium_buzz(&mut self) {
        self.dmc_buzz(0x3F, 0x60, 0x0F);
    }

    /// `LE51B`: load the DMC's length, sample start and rate, then restart it.
    fn dmc_buzz(&mut self, length: u8, start: u8, rate: u8) {
        self.apu_write(DMC_LEN, length);
        self.apu_write(DMC_START, start);
        self.apu_write(DMC_FREQ, rate);
        self.apu_write(SND_CHN, SND_CHN_DMC_OFF);
        self.apu_write(DMC_RAW, 0);
        self.apu_write(SND_CHN, SND_CHN_DMC_ON);
    }
}

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "soundEffectSlot0_makesNoSound",
        addr: layout::SOUND_EFFECT_SLOT0_MAKES_NO_SOUND,
        run: |g, _| {
            g.sound_effect_slot0_makes_no_sound();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot0_gameOverCurtainInit",
        addr: layout::SOUND_EFFECT_SLOT0_GAME_OVER_CURTAIN_INIT,
        run: |g, _| {
            g.sound_effect_slot0_game_over_curtain_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot0_endingRocketInit",
        addr: layout::SOUND_EFFECT_SLOT0_ENDING_ROCKET_INIT,
        run: |g, _| {
            g.sound_effect_slot0_ending_rocket_init();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "advanceSoundEffectSlot0WithoutUpdate",
        addr: layout::ADVANCE_SOUND_EFFECT_SLOT0_WITHOUT_UPDATE,
        run: |g, _| {
            g.advance_sound_effect_slot0_without_update();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "updateSoundEffectSlot0_apu",
        addr: layout::UPDATE_SOUND_EFFECT_SLOT0_APU,
        run: |g, _| {
            g.update_sound_effect_slot0_apu();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot3_fallingAlien",
        addr: layout::SOUND_EFFECT_SLOT3_FALLING_ALIEN,
        run: |g, _| {
            g.sound_effect_slot3_falling_alien();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot3_donk",
        addr: layout::SOUND_EFFECT_SLOT3_DONK,
        run: |g, _| {
            g.sound_effect_slot3_donk();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "updateSoundEffectSlot3_apu",
        addr: layout::UPDATE_SOUND_EFFECT_SLOT3_APU,
        run: |g, _| {
            g.update_sound_effect_slot3_apu();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot3Playing_advance",
        addr: layout::SOUND_EFFECT_SLOT3_PLAYING_ADVANCE,
        run: |g, _| {
            g.sound_effect_slot3_playing_advance();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot2_lowBuzz",
        addr: layout::SOUND_EFFECT_SLOT2_LOW_BUZZ,
        run: |g, _| {
            g.sound_effect_slot2_low_buzz();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "soundEffectSlot2_mediumBuzz",
        addr: layout::SOUND_EFFECT_SLOT2_MEDIUM_BUZZ,
        run: |g, _| {
            g.sound_effect_slot2_medium_buzz();
            Ret::Unit
        },
        scratch: &[],
    },
];
