//! The ending animations' per-frame work: the type-B concert and cathedral sprites.

use crate::shadow::{Ret, Routine};
use crate::{layout, Game};

/// Nametable addresses of the ending's score panels.
const ENDING_SCORE: u16 = 0x208E;
const ENDING_COUNTER4: u16 = 0x20B0;
const ENDING_COUNTER5: u16 = 0x20D0;
const ENDING_TOTAL_SCORE: u16 = 0x212E;

/// `patchToPpu` markers: $FE starts the next address, $FD ends the list.
const PATCH_NEXT_ADDR: u8 = 0xFE;
const PATCH_END: u8 = 0xFD;

/// Level whose ending is the concert (every other level's ending is the cathedral).
const CONCERT_LEVEL: u8 = 9;
/// `ending >> 3` value at which Luigi's frame wraps, starting another cycle.
const LUIGI_CYCLE_STEPS: u8 = 0x0A;
/// Mario's y offset while he is off the screen, which keeps his sprite index at $27.
const MARIO_HIDDEN_Y: u8 = 0x97;
/// Sound effect requested in slot 0 while a dome's light is still moving.
const SFX_SLOT0_DOME: u8 = 0x03;
/// Ending A thresholds on player 1's top score digit: the ending is the first index whose
/// threshold the score is below, with index 4 (120,000 and up) having its own patch.
const ENDING_A_THRESHOLDS: [u8; 4] = [0x05, 0x07, 0x10, 0x12];
/// `ending_typeBConcertPatchToPpuForHeight` runs these lists, starting at the one for `ending`.
const CONCERT_PATCH_LISTS: [u16; 6] = [
    layout::ENDING_PATCH_TO_PPU_TYPE_BCONCERT_HEIGHT_UNUSED,
    layout::ENDING_PATCH_TO_PPU_TYPE_BCONCERT_HEIGHT0,
    layout::ENDING_PATCH_TO_PPU_TYPE_BCONCERT_HEIGHT1,
    layout::ENDING_PATCH_TO_PPU_TYPE_BCONCERT_HEIGHT2,
    layout::ENDING_PATCH_TO_PPU_TYPE_BCONCERT_HEIGHT3,
    layout::ENDING_PATCH_TO_PPU_TYPE_BCONCERT_HEIGHT4,
];

impl Game {
    /// `render_mode_ending_animation`: draw the score panels for the ending screen. Type B also
    /// shows the two counters and the total score.
    pub fn render_mode_ending_animation(&mut self) {
        self.ppu_addr(ENDING_SCORE);
        let score = self.vars.player1.score.0;
        for digits in score.iter().rev() {
            self.two_digs_to_ppu(*digits);
        }
        if self.vars.game_type != 0 {
            self.ppu_addr(ENDING_COUNTER4);
            self.two_digs_to_ppu(self.vars.general_counter4);
            self.ppu_addr(ENDING_COUNTER5);
            self.two_digs_to_ppu(self.vars.general_counter5);
            self.ppu_addr(ENDING_TOTAL_SCORE);
            let total = self.vars.total_score.0;
            for digits in total.iter().rev() {
                self.two_digs_to_ppu(*digits);
            }
        }
        self.ppu_scroll(0, 0);
    }

    /// `ending_initTypeBVars`: set up the ending's variables for a type-B ending. The concert
    /// (level 9) patches its nametable for the player's height and takes its sprite timings from
    /// the tables; the cathedral takes its per-level values.
    pub fn ending_init_type_b_vars(&mut self) {
        self.vars.ending = 0;
        self.vars.ending_custom_vars[0] = 0;
        self.vars.ending_type_b_cathedral_frame_delay_counter = 0;
        self.vars.sprite_index_in_oam_content_lookup = 2;
        let level = self.vars.active.level_number;
        if level == CONCERT_LEVEL {
            self.vars.ending = self.vars.player1.start_height.wrapping_add(1);
            self.ending_type_bconcert_patch_to_ppu_for_height();
            self.vars.ending = 0;
            self.vars.ending_custom_vars[2] = 0;
            self.vars.ending_custom_vars[3] = self.rom.byte(layout::LA73_D);
            self.vars.ending_custom_vars[4] = self.rom.byte(layout::LA73_E);
            self.vars.ending_custom_vars[5] = self.rom.byte(layout::LA73_F);
            self.vars.ending_custom_vars[6] = self.rom.byte(layout::LA740);
        } else {
            let value = self.rom.table(layout::LA767, level as usize);
            for slot in &mut self.vars.ending_custom_vars[2..] {
                *slot = value;
            }
            self.vars.ending_custom_vars[1] = self.rom.table(layout::LA75_D, level as usize);
        }
    }

    /// `selectEndingScreen`: choose the ending from player 1's score. Ending A is 0..=4 by
    /// score (4 is 120,000 and up, which also copies its own nametable patch). The chosen ending
    /// sets `ending_custom_vars[0]` from a table and clears `ending_custom_vars[1]`.
    pub fn select_ending_screen(&mut self) {
        let top_digit = self.vars.player1.score.0[2];
        let ending = match ENDING_A_THRESHOLDS.iter().position(|&threshold| top_digit < threshold) {
            Some(index) => index as u8,
            None => {
                self.patch_to_ppu_list(layout::ENDING_PATCH_TO_PPU_TYPE_AOVER120K);
                4
            }
        };
        self.vars.ending = ending;
        self.vars.ending_custom_vars[0] = self.rom.table(layout::LAA2_A, ending as usize);
        self.vars.ending_custom_vars[1] = 0;
    }

    /// `ending_typeBConcertPatchToPpuForHeight`: write the concert's nametable patches for the
    /// player's height. The original is a jump table on `ending` whose cases fall through, so
    /// each case runs its own list and every list after it.
    pub fn ending_type_bconcert_patch_to_ppu_for_height(&mut self) {
        for &list in CONCERT_PATCH_LISTS.iter().skip(self.vars.ending as usize) {
            self.patch_to_ppu_list(list);
        }
    }

    /// Point `patch_to_ppu_addr` at `list` and play it.
    pub fn patch_to_ppu_list(&mut self, list: u16) {
        self.vars.patch_to_ppu_addr.0 = list;
        self.patch_to_ppu();
    }

    /// `patchToPpu`: copy the patch list at `patch_to_ppu_addr` from ROM to VRAM. The list is an
    /// address (high, low) then bytes to write; $FE starts the next address, $FD ends.
    pub fn patch_to_ppu(&mut self) {
        let list = self.vars.patch_to_ppu_addr.0;
        let mut y: u8 = 0;
        'address: loop {
            let hi = self.rom.byte(list.wrapping_add(y as u16));
            let lo = self.rom.byte(list.wrapping_add(y.wrapping_add(1) as u16));
            self.ppu_addr(u16::from_be_bytes([hi, lo]));
            y = y.wrapping_add(2);
            loop {
                let value = self.rom.byte(list.wrapping_add(y as u16));
                y = y.wrapping_add(1);
                match value {
                    PATCH_NEXT_ADDR => continue 'address,
                    PATCH_END => return,
                    _ => self.ppu_data(value),
                }
            }
        }
    }

    /// `render_ending`: stage one frame of the ending: the rocket for type A, the concert for
    /// type B at level 9, the cathedral for the other type-B levels.
    pub fn render_ending(&mut self) {
        if self.vars.game_type == 0 {
            self.ending_type_a();
        } else if self.vars.active.level_number == CONCERT_LEVEL {
            self.ending_type_bconcert();
        } else {
            self.ending_type_b_cathedral();
        }
    }

    /// `ending_typeBConcert`: stage the concert's sprites for this frame. The original's cases
    /// fall through, so a player at height `h` gets the characters from `h` down to Kid Icarus.
    /// Then the domes' lights are updated.
    pub fn ending_type_bconcert(&mut self) {
        let characters: [fn(&mut Game); 6] = [
            Self::concert_kid_icarus,
            Self::concert_link,
            Self::concert_samus,
            Self::concert_donkey_kong,
            Self::concert_bowser,
            Self::concert_mario_luigi_peach,
        ];
        let height = self.vars.player1.start_height as usize;
        for character in characters.iter().take(height + 1).rev() {
            character(self);
        }
        self.la6bc();
    }

    /// Stage one concert sprite at a screen position.
    fn stage_concert_sprite(&mut self, x: u8, y: u8, index: u8) {
        self.vars.sprite_x_offset = x;
        self.vars.sprite_y_offset = y;
        self.vars.sprite_index_in_oam_content_lookup = index;
        self.load_sprite_into_oam_staging();
    }

    /// Bit `bit` of the low frame counter byte: a character's two-frame animation toggles on it.
    fn frame_bit(&self, bit: u32) -> u8 {
        ((self.vars.frame_counter.0 as u8) >> bit) & 1
    }

    /// The Mario/Luigi/Peach block: Mario's two sprites, then Luigi's frame, which advances every
    /// 8 frames over ten steps, with a cycle count in `ending_custom_vars[0]`.
    fn concert_mario_luigi_peach(&mut self) {
        let frame = self.vars.frame_counter.0 as u8;
        self.stage_concert_sprite(0xC8, 0x47, 0x21 + ((frame >> 3) & 1));

        let mario_y = self.rom.table(layout::MARIO_FRAME_TO_YOFFSET_TABLE, ((frame & 0x18) >> 3) as usize);
        let index = if mario_y == MARIO_HIDDEN_Y { 0x27 } else { 0x28 };
        self.stage_concert_sprite(0xA0, mario_y, index);

        // The original re-runs this check after wrapping; once reset, `ending >> 3` is 0, so one
        // `if` is the same.
        if self.vars.ending >> 3 == LUIGI_CYCLE_STEPS {
            self.vars.ending = 0;
            self.vars.ending_custom_vars[0] = self.vars.ending_custom_vars[0].wrapping_add(1);
        }
        let step = (self.vars.ending >> 3) as usize;
        let y = self.rom.table(layout::LUIGI_FRAME_TO_YOFFSET_TABLE, step);
        let index = self.rom.table(layout::LUIGI_FRAME_TO_SPRITE_TABLE, step);
        self.stage_concert_sprite(0xC0, y, index);
        self.vars.ending = self.vars.ending.wrapping_add(1);
    }

    /// Bowser's sprite, animated on frame bit 4.
    fn concert_bowser(&mut self) {
        let phase = self.frame_bit(4);
        self.stage_concert_sprite(0x30, 0xA7, 0x1F + phase);
    }

    /// Donkey Kong's sprite, animated on frame bit 4.
    fn concert_donkey_kong(&mut self) {
        let phase = self.frame_bit(4);
        self.stage_concert_sprite(0x40, 0x77, 0x1D + phase);
    }

    /// Samus's sprite, animated on frame bit 4.
    fn concert_samus(&mut self) {
        let phase = self.frame_bit(4);
        self.stage_concert_sprite(0xA8, 0xD7, 0x1A + phase);
    }

    /// Link's sprite, animated on frame bit 4.
    fn concert_link(&mut self) {
        let phase = self.frame_bit(4);
        self.stage_concert_sprite(0xC8, 0xD7, 0x18 + phase);
    }

    /// Kid Icarus's sprite, animated on frame bit 4.
    fn concert_kid_icarus(&mut self) {
        let phase = self.frame_bit(4);
        self.stage_concert_sprite(0x28, 0x77, 0x16 + phase);
    }

    /// `ending_typeBCathedralSetSprite`: count the cathedral's animation and pick the level's
    /// sprite, offset by the animation frame (`ending_custom_vars[0]`, which toggles).
    pub fn ending_type_bcathedral_set_sprite(&mut self) {
        self.vars.ending = self.vars.ending.wrapping_add(1);
        let level = self.vars.active.level_number as usize;
        if self.rom.table(layout::ENDING_TYPE_BCATHEDRAL_ANIM_SPEED, level) == self.vars.ending {
            self.vars.ending_custom_vars[0] ^= 1;
            self.vars.ending = 0;
        }
        let base = self.rom.table(layout::ENDING_TYPE_BCATHEDRAL_SPRITE_TABLE, level);
        self.vars.sprite_index_in_oam_content_lookup = base.wrapping_add(self.vars.ending_custom_vars[0]);
    }

    /// `ending_computeTypeBCathedralYTableIndex`: index into the cathedral's per-sprite tables,
    /// `level * 6 + ending_current_sprite`. The original returns it in X; the caller gets it as
    /// the return value.
    pub fn ending_compute_type_bcathedral_ytable_index(&mut self) -> u8 {
        let level = self.vars.active.level_number;
        let doubled = level.wrapping_shl(1);
        self.vars.general_counter = doubled;
        doubled
            .wrapping_shl(1)
            .wrapping_add(doubled)
            .wrapping_add(self.vars.ending_current_sprite)
    }

    /// `LA6BC`: the concert's dome lights. Each of the four domes whose trigger value matches
    /// the ending counter steps its light down and bumps the counter. Then every dome with a
    /// nonzero light is staged, plus a glow sprite while its light is still moving.
    pub fn la6bc(&mut self) {
        for x in 0..4 {
            let trigger = self.rom.table(layout::LA735, x);
            let custom = &mut self.vars.ending_custom_vars;
            if trigger == custom[0] && custom[3 + x] != 0 {
                custom[3 + x] -= 1;
                custom[0] = custom[0].wrapping_add(1);
            }
        }
        self.vars.ending_current_sprite = 0;
        while self.vars.ending_current_sprite != 4 {
            self.stage_dome(self.vars.ending_current_sprite as usize);
            self.vars.ending_current_sprite += 1;
        }
    }

    /// One dome's light for `LA6BC`: the light counts down (twice per step while it is below
    /// $A0), is staged, and while it is not at its resting value a glow sprite is staged too.
    fn stage_dome(&mut self, x: usize) {
        let counter = self.vars.ending_custom_vars[3 + x];
        if counter == 0 {
            return;
        }
        self.vars.general_counter = counter;
        if self.rom.table(layout::LA73_D, x) != self.vars.general_counter {
            self.vars.sfx_init[0] = SFX_SLOT0_DOME;
            self.vars.general_counter = self.vars.general_counter.wrapping_sub(1);
            // The second step is skipped at $A0 and above (unsigned compare).
            if self.vars.general_counter < 0xA0 {
                self.vars.general_counter = self.vars.general_counter.wrapping_sub(1);
            }
        }
        let light = self.vars.general_counter;
        self.vars.ending_custom_vars[3 + x] = light;
        let dome_x = self.rom.table(layout::DOME_NUMBER_TO_XOFFSET_TABLE, x);
        let dome_sprite = self.rom.table(layout::DOME_NUMBER_TO_SPRITE_TABLE, x);
        self.stage_concert_sprite(dome_x, light, dome_sprite);

        self.vars.general_counter = self.vars.ending_custom_vars[3 + x];
        if self.rom.table(layout::LA73_D, x) == self.vars.general_counter {
            return;
        }
        let glow_x = self.rom.table(layout::LA745, x).wrapping_add(dome_x);
        let glow_index = ((self.vars.frame_counter.0 as u8 & 0x02) >> 1) + 0x51;
        self.stage_concert_sprite(glow_x, light, glow_index);
    }
}

/// `twoDigsToPPU` leaves each byte it prints in `generalCounter`, which this routine never reads
/// back.
const RENDER_SCRATCH: &[&str] = &["general_counter"];
/// `loadSpriteIntoOamStaging` uses `generalCounter` and `generalCounter2` as the pointer and
/// loop counter for a sprite's entries. Each caller here writes its own values before reading.
const LOAD_SPRITE_SCRATCH: &[&str] = &["general_counter", "general_counter2"];

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "render_mode_ending_animation",
        addr: layout::RENDER_MODE_ENDING_ANIMATION,
        run: |g, _| {
            g.render_mode_ending_animation();
            Ret::Unit
        },
        scratch: RENDER_SCRATCH,
    },
    Routine {
        name: "ending_initTypeBVars",
        addr: layout::ENDING_INIT_TYPE_BVARS,
        run: |g, _| {
            g.ending_init_type_b_vars();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "selectEndingScreen",
        addr: layout::SELECT_ENDING_SCREEN,
        run: |g, _| {
            g.select_ending_screen();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "ending_typeBConcertPatchToPpuForHeight",
        addr: layout::ENDING_TYPE_BCONCERT_PATCH_TO_PPU_FOR_HEIGHT,
        run: |g, _| {
            g.ending_type_bconcert_patch_to_ppu_for_height();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "render_ending",
        addr: layout::RENDER_ENDING,
        run: |g, _| {
            g.render_ending();
            Ret::Unit
        },
        scratch: LOAD_SPRITE_SCRATCH,
    },
    Routine {
        name: "ending_typeBConcert",
        addr: layout::ENDING_TYPE_BCONCERT,
        run: |g, _| {
            g.ending_type_bconcert();
            Ret::Unit
        },
        scratch: LOAD_SPRITE_SCRATCH,
    },
    Routine {
        name: "ending_typeBCathedralSetSprite",
        addr: layout::ENDING_TYPE_BCATHEDRAL_SET_SPRITE,
        run: |g, _| {
            g.ending_type_bcathedral_set_sprite();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "ending_computeTypeBCathedralYTableIndex",
        addr: layout::ENDING_COMPUTE_TYPE_BCATHEDRAL_YTABLE_INDEX,
        run: |g, _| {
            g.ending_compute_type_bcathedral_ytable_index();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "LA6BC",
        addr: layout::LA6_BC,
        run: |g, _| {
            g.la6bc();
            Ret::Unit
        },
        scratch: LOAD_SPRITE_SCRATCH,
    },
    Routine {
        name: "patchToPpu",
        addr: layout::PATCH_TO_PPU,
        run: |g, _| {
            g.patch_to_ppu();
            Ret::Unit
        },
        scratch: &[],
    },
];
