//! The end of a game: high-score name entry and the ending animations' flow.
//!
//! The screens block (they wait for vblank), so the whole-game oracle checks them; the two
//! per-frame sprite routines (`ending_typeA`, `ending_typeBCathedral`) are checked call by call.

use crate::shadow::{Ret, Routine};
use crate::{buttons, layout, Game};

const MMC1_4KCHR_32KPRG_H_MIRROR: u8 = 0x10;
const CHR_TITLE_MENU: u8 = 0x00;
const CHR_TYPEB_ENDING: u8 = 0x01;
const CHR_TYPEA_ENDING: u8 = 0x02;
const CHR_GAME: u8 = 0x03;

const RENDER_CONGRATULATIONS: u8 = 0x02;
const RENDER_ENDING_ANIMATION: u8 = 0x04;
/// `outOfDateRenderFlags`: the letter being entered needs drawing.
const RENDER_HIGH_SCORE_LETTER: u8 = 0x80;

const MUSIC_HIGH_SCORE: u8 = 0x09;
const MUSIC_ENDING: u8 = 0x0A;
const SFX_SLOT0_ROCKET: u8 = 0x03;
const SFX_MENU_OPTION: u8 = 0x01;
const SFX_MENU_SCREEN: u8 = 0x02;

const ENDING_SLEEP_TIME_1: u8 = 0x80;
const ENDING_SLEEP_TIME_2: u8 = 0x40;
const ENDING_SLEEP_TIME_3: u8 = 0x02;

/// Name entry: the cursor's sprite (and its flicker), 6 letters per name, the alphabet size.
const CURSOR_SPRITE: u8 = 0x0E;
const CURSOR_SPRITE_FLICKER: u8 = 0x02;
const CURSOR_FLICKER_MASK: u8 = 0x03;
const NAME_LETTERS: u8 = 6;
const ALPHABET: u8 = 0x2C;
/// Holding Up/Down changes the letter every 8 frames.
const LETTER_REPEAT_MASK: u8 = 0x07;

/// The B-type ending's concert is the level whose display digit is 9.
const CONCERT_LEVEL: u8 = 9;
const PENGUIN_LEVEL: u8 = 2;
const OSTRICH_LEVEL: u8 = 6;

/// The rocket starts climbing once `ending_custom_vars[1]` has counted up to this.
const ROCKET_JET_READY: u8 = 0xF0;
/// At and above this y the rocket climbs on even frames only; below `ROCKET_FAST_Y` it climbs
/// two pixels at a time.
const ROCKET_SLOW_Y: u8 = 0xB0;
const ROCKET_FAST_Y: u8 = 0x80;

impl Game {
    /// `highScoreEntryScreen`: the name-entry screen. Left/right (or A/B) move the cursor along
    /// the six letters, held Up/Down changes the letter under it every 8 frames, Start finishes.
    pub fn high_score_entry_screen(&mut self) {
        self.reset_mmc1_serial_port();
        self.set_mmc1_control_value(MMC1_4KCHR_32KPRG_H_MIRROR);
        self.set_music_track(MUSIC_HIGH_SCORE);
        self.vars.render_mode = RENDER_CONGRATULATIONS;
        self.update_audio_wait_for_nmi_and_disable_ppu_rendering();
        self.disable_nmi();
        self.change_chr_bank0(CHR_TITLE_MENU);
        self.change_chr_bank1(CHR_TITLE_MENU);
        self.bulk_copy_to_ppu(layout::HIGH_SCORE_ENTRY_SCREEN_COPY_MENU_PALETTE);
        self.bulk_copy_to_ppu(layout::HIGH_SCORE_ENTRY_SCREEN_COPY_ENTER_HIGH_SCORE_NAMETABLE);
        // "A-TYPE" / "B-TYPE" in the heading.
        self.ppu_addr(0x206D);
        self.ppu_data(0x0A + self.vars.game_type);
        self.show_high_scores();
        self.vars.render_mode = RENDER_CONGRATULATIONS;
        self.wait_for_vblank_and_enable_nmi();
        self.update_audio_wait_for_nmi_and_reset_oam_staging();
        self.update_audio_wait_for_nmi_and_enable_ppu_rendering();
        self.update_audio_wait_for_nmi_and_reset_oam_staging();

        // The name's first byte in `highScoreNames` is row * 6 (computed as 2r + 4r).
        let raw_pos = self.vars.high_score_entry_raw_pos;
        let doubled = raw_pos.wrapping_shl(1);
        self.vars.general_counter = doubled;
        self.vars.high_score_entry_name_offset_for_row = doubled.wrapping_shl(1).wrapping_add(doubled);
        self.vars.high_score_entry_name_offset_for_letter = 0;
        self.vars.oam_staging[0] = 0;
        self.vars.sprite_y_offset = self.rom.table(layout::HIGH_SCORE_POS_TO_Y, (raw_pos & 0x03) as usize);

        loop {
            self.vars.oam_staging[0] = 0;
            let cursor = self.vars.high_score_entry_name_offset_for_letter;
            self.vars.sprite_x_offset = self.rom.table(layout::HIGH_SCORE_NAME_POS_TO_X, cursor as usize);
            self.vars.sprite_index_in_oam_content_lookup =
                if self.vars.frame_counter.0 as u8 & CURSOR_FLICKER_MASK == 0 { CURSOR_SPRITE_FLICKER } else { CURSOR_SPRITE };
            self.load_sprite_into_oam_staging();

            let pressed = self.vars.newly_pressed_buttons_player1;
            if pressed & buttons::START != 0 {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                break;
            }
            if pressed & (buttons::A | buttons::RIGHT) != 0 {
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
                let next = self.vars.high_score_entry_name_offset_for_letter.wrapping_add(1);
                // Signed compare against 6: past the last letter wraps to the first.
                self.vars.high_score_entry_name_offset_for_letter =
                    if (next.wrapping_sub(NAME_LETTERS) as i8) < 0 { next } else { 0 };
            }
            if pressed & (buttons::B | buttons::LEFT) != 0 {
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
                let prev = self.vars.high_score_entry_name_offset_for_letter.wrapping_sub(1);
                self.vars.high_score_entry_name_offset_for_letter = if (prev as i8) >= 0 { prev } else { NAME_LETTERS - 1 };
            }

            let held = self.vars.held_buttons_player1;
            let repeat = self.vars.frame_counter.0 as u8 & LETTER_REPEAT_MASK == 0;
            if held & buttons::DOWN != 0 && repeat {
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
                let at = self.entered_letter_index();
                let mut letter = self.vars.high_score_names[at].wrapping_sub(1);
                if (letter as i8) < 0 {
                    letter = letter.wrapping_add(ALPHABET);
                }
                self.vars.general_counter = letter;
                self.vars.high_score_names[at] = letter;
            }
            if held & buttons::UP != 0 && repeat {
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
                let at = self.entered_letter_index();
                let mut letter = self.vars.high_score_names[at].wrapping_add(1);
                if (letter.wrapping_sub(ALPHABET) as i8) >= 0 {
                    letter = letter.wrapping_sub(ALPHABET);
                }
                self.vars.general_counter = letter;
                self.vars.high_score_names[at] = letter;
            }

            // Have the NMI redraw the letter under the cursor.
            let at = self.entered_letter_index();
            self.vars.high_score_entry_current_letter = self.vars.high_score_names[at];
            self.vars.out_of_date_render_flags = RENDER_HIGH_SCORE_LETTER;
            self.update_audio_wait_for_nmi_and_reset_oam_staging();
        }
        self.update_audio_wait_for_nmi_and_reset_oam_staging();
    }

    /// Index into `highScoreNames` of the letter under the cursor: the row's offset plus the
    /// letter's offset (an 8-bit sum).
    fn entered_letter_index(&self) -> usize {
        self.vars
            .high_score_entry_name_offset_for_row
            .wrapping_add(self.vars.high_score_entry_name_offset_for_letter) as usize
    }

    /// `inc initRam`: the increment's write to ROM resets the MMC1's serial port.
    fn reset_mmc1_serial_port(&mut self) {
        let value = self.rom.byte(layout::INIT_RAM).wrapping_add(1);
        self.mapper_write(layout::INIT_RAM, value);
    }

    /// `endingAnimation`: the ending after a game. Type A shows the rocket for the score; type B
    /// counts up the level and height bonuses first. Returns when Start is pressed.
    pub fn ending_animation(&mut self) {
        self.vars.sprite_index_in_oam_content_lookup = 2;
        self.vars.render_mode = RENDER_ENDING_ANIMATION;
        if self.vars.game_type != 0 {
            self.ending_animation_b();
        } else {
            self.ending_animation_a();
        }
    }

    /// `endingAnimationA`: the rocket ending. The rocket lifts off (`ending_custom_vars[0]` is
    /// its height, 0 when it has left); then wait for Start.
    fn ending_animation_a(&mut self) {
        self.update_audio_wait_for_nmi_and_disable_ppu_rendering();
        self.disable_nmi();
        self.change_chr_bank0(CHR_TYPEA_ENDING);
        self.change_chr_bank1(CHR_TYPEA_ENDING);
        self.bulk_copy_to_ppu(layout::ENDING_ANIMATION_A_COPY_TYPE_A_ENDING_NAMETABLE);
        self.bulk_copy_to_ppu(layout::ENDING_ANIMATION_A_COPY_ENDING_PALETTE);
        self.select_ending_screen();
        self.start_ending_screen();
        self.render_ending_unskippable(ENDING_SLEEP_TIME_1);
        loop {
            self.render_ending();
            self.update_audio_wait_for_nmi_and_reset_oam_staging();
            if self.vars.ending_custom_vars[0] != 0 {
                continue;
            }
            if self.vars.newly_pressed_buttons_player1 == buttons::START {
                return;
            }
        }
    }

    /// `endingAnimationB`: the B-type ending: the scene for the level, then the level and height
    /// bonuses added to the score 100 at a time (the bonuses live in `general_counter4` and
    /// `general_counter5`, which the ending's render mode draws).
    fn ending_animation_b(&mut self) {
        let display = self.rom.table(layout::LEVEL_DISPLAY_TABLE, self.vars.player1.level_number as usize);
        let level = display & 0x0F;
        self.vars.active.level_number = level;
        self.vars.total_score.0 = [0; 3];
        // The bonuses are BCD counts of hundreds: level * $10 and height * $10.
        self.vars.general_counter4 = level.wrapping_shl(4);
        self.vars.general_counter5 = self.vars.player1.start_height.wrapping_shl(4);
        self.update_audio_wait_for_nmi_and_disable_ppu_rendering();
        self.disable_nmi();
        if level == CONCERT_LEVEL {
            self.change_chr_bank0(CHR_TYPEB_ENDING);
            self.change_chr_bank1(CHR_TYPEB_ENDING);
            self.bulk_copy_to_ppu(layout::ENDING_ANIMATION_B_COPY_TYPE_B_LVL9_ENDING_NAMETABLE);
        } else {
            let bank0 = if level == PENGUIN_LEVEL || level == OSTRICH_LEVEL { CHR_GAME } else { CHR_TYPEA_ENDING };
            self.change_chr_bank0(bank0);
            self.change_chr_bank1(CHR_TYPEA_ENDING);
            self.bulk_copy_to_ppu(layout::ENDING_ANIMATION_B_COPY_TYPE_B_ENDING_NAMETABLE);
        }
        self.bulk_copy_to_ppu(layout::ENDING_ANIMATION_B_COPY_ENDING_PALETTE);
        self.ending_init_type_b_vars();
        self.start_ending_screen();
        self.render_ending_unskippable(ENDING_SLEEP_TIME_1);

        // The score moves to the total, and the panel's score reads zero.
        self.vars.total_score.0 = self.vars.player1.score.0;
        self.vars.sfx_init[1] = SFX_MENU_SCREEN;
        self.vars.player1.score.0 = [0; 3];
        self.render_ending_unskippable(ENDING_SLEEP_TIME_2);

        if self.vars.general_counter4 != 0 {
            loop {
                self.vars.general_counter4 = Self::bonus_step(self.vars.general_counter4);
                self.add_100_points();
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
                self.render_ending_unskippable(ENDING_SLEEP_TIME_3);
                if self.vars.general_counter4 == 0 {
                    break;
                }
            }
            self.render_ending_unskippable(ENDING_SLEEP_TIME_2);
        }
        if self.vars.general_counter5 != 0 {
            loop {
                self.vars.general_counter5 = Self::bonus_step(self.vars.general_counter5);
                self.add_100_points();
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
                self.render_ending_unskippable(ENDING_SLEEP_TIME_3);
                if self.vars.general_counter5 == 0 {
                    break;
                }
            }
            self.vars.sfx_init[1] = SFX_MENU_SCREEN;
            self.render_ending_unskippable(ENDING_SLEEP_TIME_2);
        }
        loop {
            self.render_ending();
            self.update_audio_wait_for_nmi_and_reset_oam_staging();
            if self.vars.newly_pressed_buttons_player1 == buttons::START {
                break;
            }
        }
        self.vars.active.level_number = self.vars.player1.level_number;
        self.vars.active.score.0 = self.vars.total_score.0;
    }

    /// Count a BCD bonus down by one: decrement, and a low nibble that underflowed to $F becomes 9.
    fn bonus_step(bonus: u8) -> u8 {
        let bonus = bonus.wrapping_sub(1);
        if bonus & 0x0F == 0x0F { (bonus & 0xF0) | 0x09 } else { bonus }
    }

    /// The end of an ending screen's setup: turn the display on over three frames, switch to
    /// the ending's render mode and start its music.
    fn start_ending_screen(&mut self) {
        self.wait_for_vblank_and_enable_nmi();
        self.update_audio_wait_for_nmi_and_reset_oam_staging();
        self.update_audio_wait_for_nmi_and_enable_ppu_rendering();
        self.update_audio_wait_for_nmi_and_reset_oam_staging();
        self.vars.render_mode = RENDER_ENDING_ANIMATION;
        self.set_music_track(MUSIC_ENDING);
    }

    /// `render_endingUnskippable`: draw the ending for `frames` frames (Start doesn't cut it
    /// short).
    fn render_ending_unskippable(&mut self, frames: u8) {
        self.vars.sleep_counter = frames;
        loop {
            self.render_ending();
            self.update_audio_wait_for_nmi_and_reset_oam_staging();
            if self.vars.sleep_counter == 0 {
                break;
            }
        }
    }

    /// `ending_typeA`: stage the rocket and its jet at the height in `ending_custom_vars[0]`.
    /// Once `ending_custom_vars[1]` has counted up to $F0 the rocket climbs: a pixel per frame
    /// (every other frame while low on the screen, i.e. y >= $B0), two pixels a frame once it's
    /// above $80, with the rocket sound effect.
    pub fn ending_type_a(&mut self) {
        let height = self.vars.ending_custom_vars[0];
        if height == 0 {
            return;
        }
        self.vars.sprite_y_offset = height;
        let rocket = self.vars.ending as usize;
        self.vars.sprite_x_offset = self.rom.table(layout::ROCKET_TO_XOFFSET_TABLE, rocket);
        self.vars.sprite_index_in_oam_content_lookup = self.rom.table(layout::ROCKET_TO_SPRITE_TABLE, rocket);
        self.load_sprite_into_oam_staging();

        // The jet flickers between its two sprites every 2 frames.
        let doubled = self.vars.ending.wrapping_shl(1);
        self.vars.general_counter = doubled;
        let jet = ((self.vars.frame_counter.0 as u8 & 0x02) >> 1).wrapping_add(doubled);
        self.vars.sprite_index_in_oam_content_lookup = self.rom.table(layout::ROCKET_TO_JET_SPRITE_TABLE, jet as usize);
        let jet_x = self.rom.table(layout::ROCKET_TO_JET_XOFFSET_TABLE, rocket);
        self.vars.sprite_x_offset = jet_x.wrapping_add(self.vars.sprite_x_offset);
        self.load_sprite_into_oam_staging();

        if self.vars.ending_custom_vars[1] != ROCKET_JET_READY {
            self.vars.ending_custom_vars[1] = self.vars.ending_custom_vars[1].wrapping_add(1);
            return;
        }
        if self.vars.ending_custom_vars[0] >= ROCKET_SLOW_Y && self.vars.frame_counter.0 as u8 & 1 != 0 {
            return;
        }
        self.vars.sfx_init[0] = SFX_SLOT0_ROCKET;
        self.vars.ending_custom_vars[0] = self.vars.ending_custom_vars[0].wrapping_sub(1);
        if self.vars.ending_custom_vars[0] < ROCKET_FAST_Y {
            self.vars.ending_custom_vars[0] = self.vars.ending_custom_vars[0].wrapping_sub(1);
        }
    }

    /// `ending_typeBCathedral`: stage the cathedral ending's animated sprites for the level, one
    /// per entry up to the starting height. Each sprite steps along its path (by the level's
    /// vector) every `FrameDelay` frames and, when it reaches the end of its path, releases the
    /// next sprite.
    pub fn ending_type_b_cathedral(&mut self) {
        self.ending_type_bcathedral_set_sprite();
        self.vars.ending_type_b_cathedral_frame_delay_counter = self.vars.ending_type_b_cathedral_frame_delay_counter.wrapping_add(1);
        self.vars.ending_current_sprite = 0;
        let level = self.vars.active.level_number as usize;
        loop {
            let sprite = self.vars.ending_current_sprite as usize;
            let rest = self.rom.table(layout::LA767, level);
            self.vars.general_counter = rest;
            let x = self.ending_byte(1 + sprite);
            if x != rest {
                self.vars.sprite_x_offset = x;
                let index = self.ending_compute_type_bcathedral_ytable_index();
                self.vars.sprite_y_offset = self.rom.table(layout::ENDING_TYPE_BCATHEDRAL_YTABLE, index as usize);
                self.load_sprite_into_oam_staging();
                let delay = self.rom.table(layout::ENDING_TYPE_BCATHEDRAL_FRAME_DELAY_TABLE, level);
                if delay == self.vars.ending_type_b_cathedral_frame_delay_counter {
                    let vector = self.rom.table(layout::ENDING_TYPE_BCATHEDRAL_VECTOR_TABLE, level);
                    let new_x = vector.wrapping_add(self.vars.sprite_x_offset);
                    self.vars.sprite_x_offset = new_x;
                    self.set_ending_byte(1 + sprite, new_x);
                    let index = self.ending_compute_type_bcathedral_ytable_index();
                    // At the end of its path the next sprite is released.
                    if self.rom.table(layout::ENDING_TYPE_BCATHEDRAL_XTABLE, index as usize) == new_x {
                        let release = self.rom.table(layout::LA75_D, level);
                        self.set_ending_byte(2 + sprite, release);
                    }
                }
            }
            self.vars.general_counter = self.vars.ending_current_sprite;
            if self.vars.ending_current_sprite == self.vars.active.start_height {
                break;
            }
            self.vars.ending_current_sprite = self.vars.ending_current_sprite.wrapping_add(1);
        }
        let delay = self.rom.table(layout::ENDING_TYPE_BCATHEDRAL_FRAME_DELAY_TABLE, level);
        if delay == self.vars.ending_type_b_cathedral_frame_delay_counter {
            self.vars.ending_type_b_cathedral_frame_delay_counter = 0;
        }
    }

    /// Byte `i` of `ending_customVars` ($C5..). Index 7 is the next byte of RAM,
    /// `ending_currentSprite`.
    fn ending_byte(&self, i: usize) -> u8 {
        match i {
            0..=6 => self.vars.ending_custom_vars[i],
            7 => self.vars.ending_current_sprite,
            _ => panic!("ending_customVars+{i}"),
        }
    }

    /// Write byte `i` of `ending_customVars`. Releasing the sprite after the last of six lands on
    /// `ending_currentSprite` (index 7), changing the loop's own counter, as in the original.
    fn set_ending_byte(&mut self, i: usize, value: u8) {
        match i {
            0..=6 => self.vars.ending_custom_vars[i] = value,
            7 => self.vars.ending_current_sprite = value,
            _ => panic!("ending_customVars+{i}"),
        }
    }
}

/// The sprite loader's `generalCounter`/`generalCounter2` pointer and counter; the callers
/// write them before they read them.
const LOAD_SPRITE_SCRATCH: &[&str] = &["general_counter", "general_counter2"];

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "ending_typeA",
        addr: layout::ENDING_TYPE_A,
        run: |g, _| {
            g.ending_type_a();
            Ret::Unit
        },
        scratch: LOAD_SPRITE_SCRATCH,
    },
    Routine {
        name: "ending_typeBCathedral",
        addr: layout::ENDING_TYPE_BCATHEDRAL,
        run: |g, _| {
            g.ending_type_b_cathedral();
            Ret::Unit
        },
        scratch: LOAD_SPRITE_SCRATCH,
    },
];
