//! Menu screens' per-frame work: level and height selection, and menu rendering.

use crate::shadow::{Ret, Routine};
use crate::{buttons, layout, Game};

/// PPUCTRL's two nametable-select bits, cleared on every menu frame.
const NAMETABLE_SELECT_MASK: u8 = 0xFC;
/// Sound effects requested in slot 1 by the menus.
const SFX_MENU_OPTION_SELECT: u8 = 0x01;
/// The level grid is 10 cells in rows of five: levels 0-9.
const LEVEL_MAX: u8 = 9;
const LEVEL_ROW: u8 = 5;
/// The height grid is 6 cells in rows of three: heights 0-5.
const HEIGHT_MAX: u8 = 5;
const HEIGHT_ROW: u8 = 3;
/// Added to the cursor's y when `active_player` is 1.
const SECOND_PLAYER_CURSOR_Y: u8 = 0x50;
/// The cursor blinks while the level cell is the one being chosen (and the height cell while it
/// is): `frameCounter & MENU_CURSOR_MASK` is zero on the off frames.
const MENU_CURSOR_MASK: u8 = 0x03;

impl Game {
    /// `render_mode_legal_and_title_screens`: nothing is drawn per frame on the legal and title
    /// screens. They clear the nametable-select bits in `current_ppu_ctrl` (without writing
    /// PPUCTRL) and reset the scroll.
    pub fn render_mode_legal_and_title_screens(&mut self) {
        self.vars.current_ppu_ctrl &= NAMETABLE_SELECT_MASK;
        self.vars.ppu_scroll_x = 0;
        self.vars.ppu_scroll_y = 0;
        self.ppu_scroll(0, 0);
    }

    /// `render_mode_menu_screens`: the same as the title screens, but the cleared PPUCTRL value
    /// is also written to the PPU.
    pub fn render_mode_menu_screens(&mut self) {
        self.vars.current_ppu_ctrl &= NAMETABLE_SELECT_MASK;
        let ctrl = self.vars.current_ppu_ctrl;
        self.ppu_ctrl(ctrl);
        self.vars.ppu_scroll_x = 0;
        self.vars.ppu_scroll_y = 0;
        self.ppu_scroll(0, 0);
    }

    /// `gameMode_levelMenu_handleLevelHeightNavigation`: move the level or height cursor with the
    /// d-pad, switch between the two with A (type B only), and stage the cursor sprites.
    ///
    /// Level is a grid of rows of five (Left/Right step, Up/Down jump a row); height is rows of
    /// three. Every step plays the menu sound. Each check below is separate, as in the original,
    /// but only one button counts as newly pressed at a time, so they never interact.
    pub fn game_mode_level_menu_handle_level_height_navigation(&mut self) {
        let mut pressed = self.vars.newly_pressed_buttons;
        // In the app (no A in menus), stepping off the level grid's right edge or the height
        // grid's left edge switches between them, as A does.
        if self.options.menus && self.vars.game_type != 0 {
            let on_height = self.vars.selecting_level_or_height != 0;
            let crossing = (pressed == buttons::RIGHT && !on_height && self.vars.active.start_level % LEVEL_ROW == LEVEL_ROW - 1)
                || (pressed == buttons::LEFT && on_height && self.vars.active.start_height % HEIGHT_ROW == 0);
            if crossing {
                pressed = buttons::A;
            }
        }
        if pressed == buttons::RIGHT {
            self.vars.sfx_init[1] = SFX_MENU_OPTION_SELECT;
            if self.vars.selecting_level_or_height != 0 {
                if self.vars.active.start_height != HEIGHT_MAX {
                    self.vars.active.start_height = self.vars.active.start_height.wrapping_add(1);
                }
            } else if self.vars.active.start_level != LEVEL_MAX {
                self.vars.active.start_level = self.vars.active.start_level.wrapping_add(1);
            }
        }
        if pressed == buttons::LEFT {
            self.vars.sfx_init[1] = SFX_MENU_OPTION_SELECT;
            if self.vars.selecting_level_or_height != 0 {
                if self.vars.active.start_height != 0 {
                    self.vars.active.start_height = self.vars.active.start_height.wrapping_sub(1);
                }
            } else if self.vars.active.start_level != 0 {
                self.vars.active.start_level = self.vars.active.start_level.wrapping_sub(1);
            }
        }
        if pressed == buttons::DOWN {
            self.vars.sfx_init[1] = SFX_MENU_OPTION_SELECT;
            if self.vars.selecting_level_or_height != 0 {
                // Signed compare, as the original's `cmp / bpl`.
                if ((self.vars.active.start_height.wrapping_sub(HEIGHT_ROW)) as i8) < 0 {
                    self.vars.active.start_height = self.vars.active.start_height.wrapping_add(HEIGHT_ROW);
                }
            } else if ((self.vars.active.start_level.wrapping_sub(LEVEL_ROW)) as i8) < 0 {
                self.vars.active.start_level = self.vars.active.start_level.wrapping_add(LEVEL_ROW);
            }
        }
        if pressed == buttons::UP {
            self.vars.sfx_init[1] = SFX_MENU_OPTION_SELECT;
            if self.vars.selecting_level_or_height != 0 {
                // Signed compare, as the original's `cmp / bmi`.
                if ((self.vars.active.start_height.wrapping_sub(HEIGHT_ROW)) as i8) >= 0 {
                    self.vars.active.start_height = self.vars.active.start_height.wrapping_sub(HEIGHT_ROW);
                }
            } else if ((self.vars.active.start_level.wrapping_sub(LEVEL_ROW)) as i8) >= 0 {
                self.vars.active.start_level = self.vars.active.start_level.wrapping_sub(LEVEL_ROW);
            }
        }
        // A toggles the level/height choice, but only in type B (type A has no height).
        if self.vars.game_type != 0 && pressed == buttons::A {
            self.vars.sfx_init[1] = SFX_MENU_OPTION_SELECT;
            self.vars.selecting_level_or_height ^= 1;
        }

        // The level cursor is solid while the height is chosen, and blinks while it is chosen.
        let blink_on = self.vars.frame_counter.0 as u8 & MENU_CURSOR_MASK != 0;
        if self.vars.selecting_level_or_height != 0 || blink_on {
            self.stage_level_cursor();
        }
        if self.vars.game_type == 0 {
            return;
        }
        // The height cursor is the mirror image.
        if self.vars.selecting_level_or_height == 0 || blink_on {
            self.stage_height_cursor();
        }
    }

    /// Stage the cursor sprite on the chosen start level's cell.
    fn stage_level_cursor(&mut self) {
        let level = self.vars.active.start_level as usize;
        self.vars.sprite_y_offset = self.rom.table(layout::LEVEL_TO_SPRITE_YOFFSET, level);
        self.vars.sprite_index_in_oam_content_lookup = 0;
        self.vars.sprite_x_offset = self.rom.table(layout::LEVEL_TO_SPRITE_XOFFSET, level);
        self.stage_menu_cursor();
    }

    /// Stage the cursor sprite on the chosen start height's cell.
    fn stage_height_cursor(&mut self) {
        let height = self.vars.active.start_height as usize;
        self.vars.sprite_y_offset = self.rom.table(layout::HEIGHT_TO_PPU_HIGH_ADDR, height);
        self.vars.sprite_index_in_oam_content_lookup = 0;
        self.vars.sprite_x_offset = self.rom.table(layout::HEIGHT_TO_PPU_LOW_ADDR, height);
        self.stage_menu_cursor();
    }

    /// Shift the cursor for the second player's board, then stage it.
    fn stage_menu_cursor(&mut self) {
        if self.vars.active_player == 1 {
            self.vars.sprite_y_offset = self.vars.sprite_y_offset.wrapping_add(SECOND_PLAYER_CURSOR_Y);
        }
        self.load_sprite_into_oam_staging();
    }
}

/// The level/height cursor staging calls `load_sprite_into_oam_staging`, which leaves its pointer
/// (`general_counter`, `general_counter2`) behind as temporaries; this routine never reads them.
const NAVIGATION_SCRATCH: &[&str] = &["general_counter", "general_counter2"];

pub const SHADOW: &[Routine] = &[
    Routine {
        name: "render_mode_legal_and_title_screens",
        addr: layout::RENDER_MODE_LEGAL_AND_TITLE_SCREENS,
        run: |g, _| {
            g.render_mode_legal_and_title_screens();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "render_mode_menu_screens",
        addr: layout::RENDER_MODE_MENU_SCREENS,
        run: |g, _| {
            g.render_mode_menu_screens();
            Ret::Unit
        },
        scratch: &[],
    },
    Routine {
        name: "gameMode_levelMenu_handleLevelHeightNavigation",
        addr: layout::GAME_MODE_LEVEL_MENU_HANDLE_LEVEL_HEIGHT_NAVIGATION,
        run: |g, _| {
            g.game_mode_level_menu_handle_level_height_navigation();
            Ret::Unit
        },
        scratch: NAVIGATION_SCRATCH,
    },
];
