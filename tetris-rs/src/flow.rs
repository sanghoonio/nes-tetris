//! The game's flow: power-on, the main loop, the screens (legal, title, menus), and the play
//! mode's state machine with pausing, game over and B-type setup.
//!
//! These routines block: they wait for vblank (ending the frame) in the middle, so they can't be
//! checked call by call. The whole-game oracle (`tests/frames.rs`) checks them frame by frame.
//!
//! The main loop runs the current mode's routine and then compares the A register it returned
//! with `gameModeState`: only if they're equal does it wait for vblank. Play mode relies on this
//! (state 8 returns 2 after setting the state to 2, which ends the frame), and other routines can
//! trigger it by accident, so every routine here returns the A value the original leaves.

use crate::{buttons, layout, Game};

const MODE_LEGAL: u8 = 0;
const MODE_TITLE: u8 = 1;
const MODE_LEVEL_MENU: u8 = 3;
const MODE_DEMO: u8 = 5;
const MODE_START_DEMO: u8 = 6;

pub(crate) const RENDER_LEGAL_AND_TITLE: u8 = 0;
const RENDER_MENUS: u8 = 1;
const RENDER_PLAY: u8 = 3;

/// MMC1 control: 4 KB CHR banks, 32 KB PRG, horizontal mirroring.
const MMC1_4KCHR_32KPRG_H_MIRROR: u8 = 0x10;
const PRG_32K_BANK: u8 = 0x00;
pub(crate) const CHR_TITLE_MENU: u8 = 0x00;
const CHR_GAME: u8 = 0x03;

const LEGAL_SLEEP_TIME: u8 = 0xFF;
const ENDING_SLEEP_TIME_1: u8 = 0x80;
const INITIAL_AUTOREPEAT_Y: u8 = 0xA0;
/// Frames between flips of a menu cursor's flicker.
pub(crate) const MENU_CURSOR_MASK: u8 = 0x03;

pub(crate) const SFX_MENU_OPTION: u8 = 0x01;
pub(crate) const SFX_MENU_SCREEN: u8 = 0x02;
const TILE_EMPTY: u8 = 0xEF;
const HIDDEN: u8 = 0x13;
const TILE_CURTAIN: u8 = 0x4F;
const PAUSED: u8 = 0x05;

/// `outOfDateRenderFlags`: everything.
const RENDER_ALL: u8 = 0x40 | 0x04 | 0x02 | 0x01;
/// The marker the high score table is checked against at power-on (warm boots keep scores).
pub(crate) const INIT_MAGIC: [u8; 5] = [0x12, 0x34, 0x56, 0x78, 0x9A];

impl Game {
    /// `reset`: power-on. Silence the PPU, wait for it to warm up (two vblanks, which this
    /// model doesn't time), set up the mapper and run the game.
    pub async fn reset(&mut self) -> ! {
        self.ppu_ctrl(0);
        self.ppu_mask(0);
        self.ppu_status();
        self.ppu_status();
        self.reset_mmc1(layout::RESET);
        self.set_mmc1_control_value(MMC1_4KCHR_32KPRG_H_MIRROR);
        self.change_chr_bank0(CHR_TITLE_MENU);
        self.change_chr_bank1(CHR_TITLE_MENU);
        self.change_prg_bank(PRG_32K_BANK);
        self.init_ram().await
    }

    /// `inc initRam` / `inc reset`: an increment of a ROM byte, whose write (bit 7 set) resets
    /// the MMC1's serial port.
    fn reset_mmc1(&mut self, addr: u16) {
        let v = self.rom.byte(addr).wrapping_add(1);
        self.mapper_write(addr, v);
    }

    /// `initRam` / `initRamContinued`: clear RAM below the high score table, initialise the
    /// table on a cold boot, set up video and sound, then run the main loop.
    async fn init_ram(&mut self) -> ! {
        self.vars.with_ram(|ram| ram[..0x700].fill(0));
        // The loop's pointer ends at page $FF (tmp2) with tmp1 = 0.
        self.vars.tmp2 = 0xFF;
        if let Some(saved) = self.saved.take() {
            saved.store(&mut self.vars);
        }
        if self.vars.init_magic != INIT_MAGIC {
            let mut i = 0;
            loop {
                let b = self.rom.byte(layout::DEFAULT_HIGH_SCORES_TABLE + i);
                if b == 0xFF {
                    break;
                }
                self.vars.with_ram(|ram| ram[0x700 + i as usize] = b);
                i += 1;
            }
            self.vars.init_magic = INIT_MAGIC;
        }
        self.vars.rng_seed.0 = 0x8988;
        self.vars.ppu_scroll_x = 0;
        self.vars.ppu_scroll_y = 0;
        self.ppu_scroll(0, 0);
        self.vars.current_ppu_ctrl = 0x90;
        self.ppu_ctrl(0x90);
        self.ppu_mask(0x06);
        self.init_audio();
        self.update_audio2();
        self.update_audio_wait_for_nmi_and_disable_ppu_rendering().await;
        self.disable_nmi();
        for page in [0x20, 0x24, 0x28, 0x2C] {
            // LAA82: fill a nametable with blank tiles, attributes 0.
            self.memset_ppu_page_and_more(page, 0xFF, 0x00);
        }
        self.memset_page(TILE_EMPTY, 0x04, 0x05);
        self.wait_for_vblank_and_enable_nmi();
        self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        self.update_audio_wait_for_nmi_and_enable_ppu_rendering().await;
        self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        self.vars.unused_0e = 0x0E;
        self.vars.game_mode_state = 0;
        self.vars.game_mode = MODE_LEGAL;
        self.vars.number_of_players = 1;
        self.vars.frame_counter.0 &= 0x00FF;
        self.main_loop().await
    }

    /// The main loop: run the current mode; wait for vblank when it says the frame is done;
    /// when the demo's recorded input runs out, go back to the title screen.
    async fn main_loop(&mut self) -> ! {
        loop {
            let a = self.branch_on_game_mode().await;
            if a == self.vars.game_mode_state {
                self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            }
            let demo_page = (layout::DEMO_TETRIMINO_TYPE_TABLE >> 8) as u16;
            if self.vars.game_mode == MODE_DEMO && self.vars.demo_buttons_addr.0 >> 8 == demo_page {
                self.vars.demo_buttons_addr.0 = (layout::DEMO_BUTTONS_TABLE & 0xFF00) | (self.vars.demo_buttons_addr.0 & 0xFF);
                self.vars.frame_counter.0 &= 0x00FF;
                self.vars.game_mode = MODE_TITLE;
            }
        }
    }

    /// `switch_s_plus_2a`, the jump-table dispatch: the routine for `index` in the table of
    /// addresses that follows the call at `call_site`. The original leaves the target address in
    /// tmp1/tmp2, and those bytes are observable: the attract demo's first few frames read
    /// "recorded input" from zero page.
    pub(crate) fn switch_target(&mut self, table: u16, index: u8) -> u16 {
        let target = self.rom.word(table + index.wrapping_shl(1) as u16);
        self.vars.tmp1 = target as u8;
        self.vars.tmp2 = (target >> 8) as u8;
        target
    }

    /// `branchOnGameMode`. Returns A.
    async fn branch_on_game_mode(&mut self) -> u8 {
        // `lda gameMode / jsr switch_s_plus_2a`, then the table.
        self.switch_target(layout::BRANCH_ON_GAME_MODE + 5, self.vars.game_mode);
        match self.vars.game_mode {
            0 => self.game_mode_legal_screen().await,
            1 => self.game_mode_title_screen().await,
            2 => self.game_mode_game_type_menu().await,
            3 => self.game_mode_level_menu().await,
            4 | 5 => self.game_mode_play_and_ending_high_score().await,
            6 => self.game_mode_start_demo().await,
            mode => panic!("game mode {mode}"),
        }
    }

    /// Turn rendering off, then (NMI off) switch CHR banks and upload a screen (uploads named
    /// by call site; see `bulk_copy_to_ppu`).
    pub(crate) async fn load_screen(&mut self, chr: u8, palette: u16, nametable: u16) {
        self.update_audio_wait_for_nmi_and_disable_ppu_rendering().await;
        self.disable_nmi();
        self.change_chr_bank0(chr);
        self.change_chr_bank1(chr);
        self.bulk_copy_to_ppu(palette);
        self.bulk_copy_to_ppu(nametable);
    }

    /// Turn NMI and rendering back on over three frames, as every screen setup ends.
    pub(crate) async fn show_screen(&mut self) {
        self.wait_for_vblank_and_enable_nmi();
        self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        self.update_audio_wait_for_nmi_and_enable_ppu_rendering().await;
        self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
    }

    /// `gameMode_legalScreen`: the legal notice, for 255 frames or until Start (which can only
    /// cut it short after the first 255 frames). Returns A.
    async fn game_mode_legal_screen(&mut self) -> u8 {
        self.update_audio2();
        self.vars.render_mode = RENDER_LEGAL_AND_TITLE;
        self.load_screen(CHR_TITLE_MENU, layout::GAME_MODE_LEGAL_SCREEN_COPY_LEGAL_SCREEN_PALETTE, layout::GAME_MODE_LEGAL_SCREEN_COPY_LEGAL_SCREEN_NAMETABLE).await;
        self.show_screen().await;
        self.memset_page(0x00, 0x02, 0x02);
        if self.options.menus {
            self.skippable_legal_wait(LEGAL_SLEEP_TIME).await;
        } else {
            self.sleep_for_a_vblanks(LEGAL_SLEEP_TIME).await;
        }
        self.vars.general_counter = LEGAL_SLEEP_TIME;
        let mut a = 0xFF;
        loop {
            if self.vars.newly_pressed_buttons_player1 == buttons::START {
                a = buttons::START;
                break;
            }
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            self.vars.general_counter = self.vars.general_counter.wrapping_sub(1);
            if self.vars.general_counter == 0 {
                break;
            }
        }
        self.vars.game_mode = self.vars.game_mode.wrapping_add(1);
        a
    }

    /// `gameMode_titleScreen`: wait for Start (to the menus) or about 21 seconds (to the
    /// demo). Returns A.
    async fn game_mode_title_screen(&mut self) -> u8 {
        if self.options.menus {
            return self.title_menu().await;
        }
        self.update_audio2();
        self.vars.render_mode = RENDER_LEGAL_AND_TITLE;
        self.vars.demo_recording = 0;
        self.vars.display_next_piece = 0;
        self.load_screen(CHR_TITLE_MENU, layout::GAME_MODE_TITLE_SCREEN_COPY_MENU_PALETTE, layout::GAME_MODE_TITLE_SCREEN_COPY_TITLE_SCREEN_NAMETABLE).await;
        self.show_screen().await;
        self.memset_page(0x00, 0x02, 0x02);
        self.vars.frame_counter.0 &= 0x00FF;
        loop {
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            if self.vars.newly_pressed_buttons_player1 == buttons::START {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                self.vars.game_mode = self.vars.game_mode.wrapping_add(1);
                return SFX_MENU_SCREEN;
            }
            if self.vars.frame_counter.0 >> 8 == 5 {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                self.vars.game_mode = MODE_START_DEMO;
                return MODE_START_DEMO;
            }
        }
    }

    /// `gameMode_gameTypeMenu`: choose A/B type (left/right) and music (up/down); Start goes
    /// on, B goes back to the title. Returns A.
    async fn game_mode_game_type_menu(&mut self) -> u8 {
        self.reset_mmc1(layout::INIT_RAM);
        self.set_mmc1_control_value(MMC1_4KCHR_32KPRG_H_MIRROR);
        self.vars.render_mode = RENDER_MENUS;
        self.update_audio_wait_for_nmi_and_disable_ppu_rendering().await;
        self.disable_nmi();
        self.bulk_copy_to_ppu(layout::GAME_MODE_GAME_TYPE_MENU_COPY_MENU_PALETTE);
        self.bulk_copy_to_ppu(layout::GAME_MODE_GAME_TYPE_MENU_COPY_GAME_TYPE_MENU_NAMETABLE);
        self.change_chr_bank0(CHR_TITLE_MENU);
        self.change_chr_bank1(CHR_TITLE_MENU);
        self.show_screen().await;
        self.play_selected_music();
        loop {
            self.memset_page(0xFF, 0x02, 0x02);
            let pressed = self.vars.newly_pressed_buttons_player1;
            if pressed == buttons::RIGHT {
                self.vars.game_type = 1;
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
            } else if pressed == buttons::LEFT {
                self.vars.game_type = 0;
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
            }
            let mut check_up = true;
            if pressed == buttons::DOWN {
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
                if self.vars.music_type == 3 {
                    // At the last music option, Down also skips the Up check.
                    check_up = false;
                } else {
                    self.vars.music_type += 1;
                    self.play_selected_music();
                }
            }
            if check_up && pressed == buttons::UP {
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
                if self.vars.music_type != 0 {
                    self.vars.music_type -= 1;
                    self.play_selected_music();
                }
            }
            if pressed == buttons::START {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                self.vars.game_mode = self.vars.game_mode.wrapping_add(1);
                return SFX_MENU_SCREEN;
            }
            if pressed == buttons::B {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                self.vars.frame_counter.0 &= 0x00FF;
                self.vars.game_mode = self.vars.game_mode.wrapping_sub(1);
                return 0;
            }
            self.stage_game_type_menu_cursors();
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        }
    }

    fn play_selected_music(&mut self) {
        let track = self.rom.table(layout::MUSIC_SELECTION_TABLE, self.vars.music_type as usize);
        self.set_music_track(track);
    }

    /// The flickering cursors next to the chosen game type and music.
    fn stage_game_type_menu_cursors(&mut self) {
        let flicker = self.vars.frame_counter.0 as u8 & MENU_CURSOR_MASK == 0;
        // x = game_type * 6 * 16 + $3F, computed as the original's shift-and-add (whose carries
        // never matter for the two game types).
        let t = self.vars.game_type;
        let t2 = t.wrapping_shl(1);
        let sum = t2.wrapping_shl(1).wrapping_add(t2).wrapping_add(t2 >> 7);
        self.vars.general_counter = t2;
        self.vars.sprite_x_offset = sum.wrapping_shl(4).wrapping_add(0x3F);
        self.vars.sprite_y_offset = 0x3F;
        self.vars.sprite_index_in_oam_content_lookup = if flicker { 0x02 } else { 0x01 };
        self.load_sprite_into_oam_staging();
        self.vars.sprite_y_offset = self.vars.music_type.wrapping_shl(4).wrapping_add(0x8F);
        self.vars.sprite_index_in_oam_content_lookup = if flicker { 0x02 } else { 0x53 };
        self.vars.sprite_x_offset = 0x67;
        self.load_sprite_into_oam_staging();
    }

    /// `gameMode_levelMenu`: choose the start level (and height, for B-type). Start begins the
    /// game, A+Start adds 10 to the level, B goes back. Returns A.
    async fn game_mode_level_menu(&mut self) -> u8 {
        self.reset_mmc1(layout::INIT_RAM);
        self.set_mmc1_control_value(MMC1_4KCHR_32KPRG_H_MIRROR);
        self.update_audio2();
        self.vars.render_mode = RENDER_MENUS;
        self.update_audio_wait_for_nmi_and_disable_ppu_rendering().await;
        self.disable_nmi();
        self.change_chr_bank0(CHR_TITLE_MENU);
        self.change_chr_bank1(CHR_TITLE_MENU);
        self.bulk_copy_to_ppu(layout::GAME_MODE_LEVEL_MENU_COPY_MENU_PALETTE);
        self.bulk_copy_to_ppu(layout::GAME_MODE_LEVEL_MENU_COPY_LEVEL_MENU_NAMETABLE);
        if self.vars.game_type == 0 {
            // The height selector is drawn by default; A-type covers it up.
            self.bulk_copy_to_ppu(layout::GAME_MODE_LEVEL_MENU_COPY_HEIGHT_MENU_NAMETABLEPALETTE_PATCH);
        }
        self.show_high_scores();
        self.wait_for_vblank_and_enable_nmi();
        self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        self.ppu_scroll(0, 0);
        self.update_audio_wait_for_nmi_and_enable_ppu_rendering().await;
        self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        // The menu keeps "selecting level or height" in originalY between frames.
        self.vars.original_y = 0;
        self.vars.drop_speed = 0;
        while self.vars.player1.start_level >= 10 {
            self.vars.player1.start_level -= 10;
        }
        loop {
            self.vars.active_player = 0;
            self.vars.active.start_level = self.vars.player1.start_level;
            self.vars.active.start_height = self.vars.player1.start_height;
            self.vars.selecting_level_or_height = self.vars.original_y;
            self.vars.newly_pressed_buttons = self.vars.newly_pressed_buttons_player1;
            self.game_mode_level_menu_handle_level_height_navigation();
            self.vars.player1.start_level = self.vars.active.start_level;
            self.vars.player1.start_height = self.vars.active.start_height;
            self.vars.original_y = self.vars.selecting_level_or_height;

            let pressed = self.vars.newly_pressed_buttons_player1;
            if pressed == buttons::START {
                // A+Start starts 10 levels up; in the app, Shift+Enter.
                let plus_ten = if self.options.menus {
                    self.shift_held()
                } else {
                    self.vars.held_buttons_player1 == buttons::A | buttons::START
                };
                if plus_ten {
                    self.vars.player1.start_level = self.vars.player1.start_level.wrapping_add(10);
                }
                self.vars.game_mode_state = 0;
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                self.vars.game_mode = self.vars.game_mode.wrapping_add(1);
                return SFX_MENU_SCREEN;
            }
            if pressed == buttons::B {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                self.vars.game_mode = self.vars.game_mode.wrapping_sub(1);
                return SFX_MENU_SCREEN;
            }
            // Keep drawing random garbage holes for both players while the menu is up.
            self.vars.player1.garbage_hole = self.random_column();
            self.vars.player2.garbage_hole = self.random_column();
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        }
    }

    /// Step the RNG until the low nibble of its high byte is a column (0..=9).
    pub(crate) fn random_column(&mut self) -> u8 {
        loop {
            self.step_rng();
            let v = (self.vars.rng_seed.0 >> 8) as u8 & 0x0F;
            // Signed compare, as `cmp #$0A / bpl`.
            if (v.wrapping_sub(0x0A) as i8) < 0 {
                return v;
            }
        }
    }

    /// `gameMode_startDemo`: start an A-type level 0 game in demo mode.
    async fn game_mode_start_demo(&mut self) -> u8 {
        self.vars.game_type = 0;
        self.vars.player1.start_level = 0;
        self.vars.game_mode_state = 0;
        self.vars.player1.play_state = 0;
        self.vars.game_mode = MODE_DEMO;
        self.game_mode_play_and_ending_high_score().await
    }

    /// `gameMode_playAndEndingHighScore`: the play mode's states, one after another; state 8
    /// ends the frame. Returns A.
    async fn game_mode_play_and_ending_high_score(&mut self) -> u8 {
        self.switch_target(layout::GAME_MODE_PLAY_AND_ENDING_HIGH_SCORE + 5, self.vars.game_mode_state);
        match self.vars.game_mode_state {
            0 => self.game_mode_state_init_game_background().await,
            1 => self.game_mode_state_init_game_state().await,
            2 => {
                self.game_mode_state_update_counters_and_non_player_state();
                // A is the Select check (0) or the toggled flag.
                if self.vars.newly_pressed_buttons_player1 & buttons::SELECT == 0 {
                    0
                } else {
                    self.vars.display_next_piece
                }
            }
            3 => self.game_mode_state_handle_game_over().await,
            4 => self.game_mode_state_update_player1().await,
            5 => self.game_mode_state_update_player2().await,
            6 => self.game_mode_state_check_for_reset_key_combo(),
            7 => self.game_mode_state_start_button_handling().await,
            8 => {
                // gameModeState_vblankThenRunState2: back to state 2, and A = 2 ends the frame.
                self.vars.game_mode_state = 2;
                2
            }
            state => panic!("game mode state {state}"),
        }
    }

    /// `gameModeState_initGameBackground`: draw the game screen. Returns A.
    async fn game_mode_state_init_game_background(&mut self) -> u8 {
        self.load_screen(CHR_GAME, layout::GAME_MODE_STATE_INIT_GAME_BACKGROUND_COPY_GAME_PALETTE, layout::GAME_MODE_STATE_INIT_GAME_BACKGROUND_COPY_GAME_NAMETABLE).await;
        // "A-TYPE"/"B-TYPE" and the top score for the type.
        self.ppu_addr(0x2083);
        let b_type = self.vars.game_type != 0;
        self.ppu_data(if b_type { 0x0B } else { 0x0A });
        self.ppu_addr(0x20B8);
        let top = if b_type { self.vars.high_score_scores_b } else { self.vars.high_score_scores_a };
        for digits in &top[..3] {
            self.two_digs_to_ppu(*digits);
        }
        if b_type {
            self.patch_type_b_nametable();
            self.ppu_addr(0x233B);
            self.ppu_data(self.vars.active.start_height & 0x0F);
        }
        self.show_screen().await;
        self.vars.player1.play_state = 1;
        self.vars.player2.play_state = 1;
        self.vars.player1.level_number = self.vars.player1.start_level;
        self.vars.player2.level_number = self.vars.player2.start_level;
        self.vars.game_mode_state = self.vars.game_mode_state.wrapping_add(1);
        self.vars.player2.start_level
    }

    /// The B-type additions to the game screen: runs of (address high, low, data..., $FE for
    /// the next address or $FD to end).
    fn patch_type_b_nametable(&mut self) {
        let mut i = 0u16;
        'address: loop {
            let high = self.rom.byte(layout::GAME_TYPEB_NAMETABLE_PATCH + i);
            let low = self.rom.byte(layout::GAME_TYPEB_NAMETABLE_PATCH + i + 1);
            self.ppu_addr(u16::from_be_bytes([high, low]));
            i += 2;
            loop {
                let v = self.rom.byte(layout::GAME_TYPEB_NAMETABLE_PATCH + i);
                i += 1;
                match v {
                    0xFE => continue 'address,
                    0xFD => return,
                    _ => self.ppu_data(v),
                }
            }
        }
    }

    /// `gameModeState_initGameState`: a new game's variables, first pieces and B-type garbage.
    /// Returns A.
    async fn game_mode_state_init_game_state(&mut self) -> u8 {
        self.memset_page(TILE_EMPTY, 0x04, 0x04);
        self.vars.stats_by_type = [0; 14];
        for p in [&mut self.vars.player1, &mut self.vars.player2] {
            p.tetrimino_x = 5;
            p.tetrimino_y = 0;
            p.vram_row = 0;
            p.fall_timer = 0;
            p.score.0 = [0; 3];
            p.lines.0 = [0; 2];
            p.autorepeat_y = INITIAL_AUTOREPEAT_Y;
        }
        let v = &mut self.vars;
        v.pending_garbage = 0;
        v.pending_garbage_inactive_player = 0;
        v.two_player_piece_delay_counter = 0;
        v.line_clear_stats_by_type = [0; 4];
        v.allegro = 0;
        v.demo_held_buttons = 0;
        v.demo_repeats = 0;
        v.demo_index = 0;
        v.spawn_id = 0;
        v.demo_buttons_addr.0 = layout::DEMO_BUTTONS_TABLE & 0xFF00;
        v.render_mode = RENDER_PLAY;
        let first = self.choose_next_tetrimino();
        self.vars.player1.current_piece = first;
        self.vars.player2.current_piece = first;
        self.increment_piece_stat(first);
        self.step_rng();
        let next = self.choose_next_tetrimino();
        self.vars.next_piece = next;
        self.vars.two_player_piece_delay_piece = next;
        if self.vars.game_type != 0 {
            self.vars.player1.lines.0[0] = 0x25;
            self.vars.player2.lines.0[0] = 0x25;
        }
        self.vars.out_of_date_render_flags = RENDER_ALL;
        self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        self.init_playfield_if_type_b().await;
        self.play_selected_music();
        self.vars.game_mode_state = self.vars.game_mode_state.wrapping_add(1);
        // setMusicTrack leaves A = gameMode, or $FF in the demo.
        if self.vars.game_mode == MODE_DEMO { 0xFF } else { self.vars.game_mode }
    }

    /// `initPlayfieldIfTypeB`: fill the bottom 12 rows with random garbage, one guaranteed hole
    /// per row (a row per frame), then clear rows above the chosen height.
    async fn init_playfield_if_type_b(&mut self) {
        if self.vars.game_type == 0 {
            return;
        }
        // The original keeps its row and column counters in generalCounter..5 across frames.
        self.vars.general_counter = 0x0C;
        while self.vars.general_counter != 0 {
            let row = 0x14u8.wrapping_sub(self.vars.general_counter);
            self.vars.general_counter2 = row;
            self.vars.player1.vram_row = 0;
            self.vars.player2.vram_row = 0;
            let row_start = self.rom.table(layout::MULT_BY10_TABLE, row as usize);
            self.vars.general_counter3 = 9;
            loop {
                self.step_rng();
                let r = (self.vars.rng_seed.0 >> 8) as u8 & 0x07;
                let cell = self.rom.table(layout::RNG_TABLE, r as usize);
                self.vars.general_counter4 = cell;
                let i = row_start.wrapping_add(self.vars.general_counter3);
                self.vars.playfield[i as usize] = cell;
                if self.vars.general_counter3 == 0 {
                    break;
                }
                self.vars.general_counter3 -= 1;
            }
            let hole = self.random_column();
            self.vars.general_counter5 = hole;
            self.vars.playfield[row_start.wrapping_add(hole) as usize] = TILE_EMPTY;
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            self.vars.general_counter -= 1;
        }
        // Copy to player 2's field (indices 200 down to 1, as the original's loop).
        for i in (1..=200).rev() {
            self.vars.playfield_for_second_player[i] = self.vars.playfield[i];
        }
        let clear1 = self.rom.table(layout::TYPE_BBLANK_INIT_COUNT_BY_HEIGHT_TABLE, self.vars.player1.start_height as usize);
        for i in 0..=clear1 as usize {
            self.vars.playfield[i] = TILE_EMPTY;
        }
        let clear2 = self.rom.table(layout::TYPE_BBLANK_INIT_COUNT_BY_HEIGHT_TABLE, self.vars.player2.start_height as usize);
        for i in 0..=clear2 as usize {
            self.vars.playfield_for_second_player[i] = TILE_EMPTY;
        }
    }

    /// `gameModeState_handleGameOver`: when a game ends, record a high score (with name entry)
    /// and go back to the level menu. Returns A.
    async fn game_mode_state_handle_game_over(&mut self) -> u8 {
        self.vars.general_counter2 = 5;
        if self.vars.player1.play_state != 0 {
            if self.vars.number_of_players == 1 {
                self.vars.game_mode_state = self.vars.game_mode_state.wrapping_add(1);
                return 1;
            }
            self.vars.general_counter2 = 4;
            if self.vars.player2.play_state != 0 {
                self.vars.game_mode_state = self.vars.game_mode_state.wrapping_add(1);
                return self.vars.player2.play_state;
            }
        }
        if self.vars.number_of_players != 1 {
            self.vars.game_mode_state = 9;
            return 9;
        }
        self.vars.render_mode = RENDER_PLAY;
        self.handle_high_score_if_necessary().await;
        self.vars.player1.play_state = 1;
        self.vars.player2.play_state = 1;
        self.memset_page(TILE_EMPTY, 0x04, 0x05);
        self.vars.player1.vram_row = 0;
        self.vars.player2.vram_row = 0;
        self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        self.vars.game_mode = MODE_LEVEL_MENU;
        MODE_LEVEL_MENU
    }

    /// `gameModeState_updatePlayer1`: a frame of player 1's game. Returns A.
    async fn game_mode_state_update_player1(&mut self) -> u8 {
        self.make_player1_active();
        self.branch_on_play_state().await;
        self.stage_sprite_for_current_piece();
        self.save_player1_state();
        self.stage_sprite_for_next_piece();
        if self.options.config.rules.ghost {
            self.stage_ghost_piece();
        }
        self.vars.game_mode_state = self.vars.game_mode_state.wrapping_add(1);
        // stageSpriteForNextPiece leaves the flag (preview hidden) or $FF from the sprite loader.
        if self.vars.display_next_piece != 0 { self.vars.display_next_piece } else { 0xFF }
    }

    /// `gameModeState_updatePlayer2` (two-player builds only). Returns A.
    async fn game_mode_state_update_player2(&mut self) -> u8 {
        let mut a = self.vars.number_of_players;
        if self.vars.number_of_players == 2 {
            self.make_player2_active();
            self.branch_on_play_state().await;
            self.stage_sprite_for_current_piece();
            a = self.vars.pending_garbage_inactive_player;
            self.save_player2_state();
        }
        self.vars.game_mode_state = self.vars.game_mode_state.wrapping_add(1);
        a
    }

    /// `branchOnPlayStatePlayer1/2`: run the active piece's play state.
    async fn branch_on_play_state(&mut self) {
        let table = if self.vars.active_player == 2 {
            layout::BRANCH_ON_PLAY_STATE_PLAYER2
        } else {
            layout::BRANCH_ON_PLAY_STATE_PLAYER1
        };
        self.switch_target(table + 5, self.vars.active.play_state);
        match self.vars.active.play_state {
            0 => self.play_state_unassign_orientation_id(),
            1 => self.player_controls_active_tetrimino(),
            2 => self.lock_tetrimino(),
            3 => self.check_for_completed_rows(),
            4 | 9 => self.play_state_noop(),
            5 => self.update_lines_and_statistics(),
            6 => self.b_type_goal_check().await,
            7 => self.receive_garbage(),
            8 => self.play_state_spawn_next_tetrimino(),
            10 => self.update_game_over_curtain().await,
            11 => self.play_state_increment_play_state(),
            state => panic!("play state {state}"),
        }
    }

    /// `gameModeState_checkForResetKeyCombo`: A+B+Select+Start goes back to the legal screen.
    /// Returns A: the held buttons, so holding Down+Left+Right ($07) equals the next state and
    /// makes the main loop wait an extra frame every frame.
    fn game_mode_state_check_for_reset_key_combo(&mut self) -> u8 {
        let held = self.vars.held_buttons_player1;
        if held == buttons::A | buttons::B | buttons::SELECT | buttons::START {
            self.update_audio2();
            self.vars.game_mode = MODE_LEGAL;
            return 0;
        }
        self.vars.game_mode_state = self.vars.game_mode_state.wrapping_add(1);
        if !self.options.config.rules.slowdown && held == self.vars.game_mode_state {
            return 0;
        }
        held
    }

    /// `gameModeState_startButtonHandling`: Start leaves the demo, or pauses the game until
    /// Start again (not during the game-over curtain). Returns A.
    async fn game_mode_state_start_button_handling(&mut self) -> u8 {
        let pressed = self.vars.newly_pressed_buttons_player1;
        let a = if self.vars.game_mode == MODE_DEMO && pressed == buttons::START {
            self.vars.game_mode = MODE_TITLE;
            1
        } else if self.vars.render_mode != RENDER_PLAY {
            self.vars.render_mode
        } else if pressed & buttons::START == 0 {
            0
        } else if self.vars.player1.play_state == 0x0A {
            0x0A
        } else if self.pause().await {
            // Quit from the pause screen (the app's menus): back to the title, as Start leaves
            // the demo.
            self.vars.game_mode = MODE_TITLE;
            1
        } else {
            RENDER_PLAY
        };
        self.vars.game_mode_state = self.vars.game_mode_state.wrapping_add(1);
        a
    }

    /// The pause screen: playfield hidden, "PAUSE" shown, the pause jingle, until Start. With
    /// the app's menus it's a RESUME / QUIT menu instead (returns true to quit).
    async fn pause(&mut self) -> bool {
        self.vars.music_staging_noise_hi = PAUSED;
        self.vars.render_mode = RENDER_LEGAL_AND_TITLE;
        self.update_audio_and_wait_for_nmi().await;
        // Background off, sprites on (not recorded in currentPpuMask).
        self.ppu_mask(0x16);
        self.memset_page(0xFF, 0x02, 0x02);
        if self.options.menus {
            let quit = self.pause_menu().await;
            self.ppu_mask(0x1E);
            self.vars.music_staging_noise_hi = 0;
            self.vars.player1.vram_row = 0;
            self.vars.render_mode = RENDER_PLAY;
            return quit;
        }
        loop {
            self.vars.sprite_x_offset = 0x70;
            self.vars.sprite_y_offset = 0x77;
            self.vars.sprite_index_in_oam_content_lookup = 0x05;
            self.load_sprite_into_oam_staging();
            if self.vars.newly_pressed_buttons_player1 == buttons::START {
                break;
            }
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
        }
        self.ppu_mask(0x1E);
        self.vars.music_staging_noise_hi = 0;
        self.vars.player1.vram_row = 0;
        self.vars.render_mode = RENDER_PLAY;
        false
    }

    /// `playState_updateGameOverCurtain`: lower the curtain a row every 4 frames; then show the
    /// rocket ending for 30000+ points, or wait for Start.
    async fn update_game_over_curtain(&mut self) {
        if self.vars.active.curtain_row != 0x14 {
            if self.vars.frame_counter.0 & 0x03 != 0 {
                return;
            }
            let row = self.vars.active.curtain_row;
            // Negative rows are a delay before the curtain starts.
            if (row as i8) >= 0 {
                let start = self.rom.table(layout::MULT_BY10_TABLE, row as usize);
                self.vars.active.current_piece = HIDDEN;
                for i in 0..10 {
                    self.set_playfield_cell(start.wrapping_add(i), TILE_CURTAIN);
                }
                self.vars.general_counter3 = 10;
                self.vars.active.vram_row = row;
            }
            self.vars.active.curtain_row = row.wrapping_add(1);
            return;
        }
        if self.vars.number_of_players != 2 {
            if self.vars.player1.score.0[2] >= 0x03 {
                self.sleep_for_a_vblanks(ENDING_SLEEP_TIME_1).await;
                self.ending_animation().await;
            } else if self.vars.newly_pressed_buttons_player1 != buttons::START {
                return;
            }
        }
        self.vars.active.play_state = 0;
        self.vars.newly_pressed_buttons_player1 = 0;
    }

    /// `playState_bTypeGoalCheck`: when a B-type game's lines reach 0, show "SUCCESS" and the
    /// ending.
    async fn b_type_goal_check(&mut self) {
        if self.vars.game_type == 0 || self.vars.active.lines.0[0] != 0 {
            self.vars.active.play_state = self.vars.active.play_state.wrapping_add(1);
            return;
        }
        self.set_music_track(0x02);
        let mut i = 0;
        loop {
            let tile = self.rom.byte(layout::TYPEB_SUCCESS_GRAPHIC + i);
            if tile == 0x80 {
                break;
            }
            self.set_playfield_cell(0x46 + i as u8, tile);
            i += 1;
        }
        self.vars.player1.vram_row = 0;
        self.sleep_for_14_vblanks().await;
        self.vars.render_mode = RENDER_LEGAL_AND_TITLE;
        self.sleep_for_a_vblanks(0x80).await;
        self.ending_animation().await;
        self.vars.active.play_state = 0;
        self.vars.game_mode_state = self.vars.game_mode_state.wrapping_add(1);
    }

    /// `handleHighScoreIfNecessary`: if the score makes the type's top 3, shift the lower scores
    /// down, record it with a blank name, and run name entry.
    async fn handle_high_score_if_necessary(&mut self) {
        let mut pos = if self.vars.game_type != 0 { 4 } else { 0 };
        self.vars.high_score_entry_raw_pos = pos;
        let score = self.vars.player1.score.0;
        loop {
            self.vars.general_counter2 = pos;
            let base = pos.wrapping_mul(3);
            let table = [self.high_score_byte(base), self.high_score_byte(base + 1), self.high_score_byte(base + 2)];
            // Compare digits from the top; a tie counts as beating the old score.
            if table <= [score[2], score[1], score[0]] {
                break;
            }
            pos += 1;
            self.vars.high_score_entry_raw_pos = pos;
            if pos == 3 || pos == 7 {
                return;
            }
        }
        if pos & 0x03 < 2 {
            self.copy_high_score_name_to_next_index(6);
            self.copy_high_score_score_to_next_index(3);
            self.copy_high_score_level_to_next_index(1);
            if pos & 0x03 == 0 {
                self.copy_high_score_name_to_next_index(0);
                self.copy_high_score_score_to_next_index(0);
                self.copy_high_score_level_to_next_index(0);
            }
        }
        let name = self.rom.table(layout::HIGH_SCORE_INDEX_TO_HIGH_SCORE_NAMES_OFFSET, pos as usize) as usize;
        self.vars.high_score_names[name..name + 6].fill(0);
        let at = self.rom.table(layout::HIGH_SCORE_INDEX_TO_HIGH_SCORE_SCORES_OFFSET, pos as usize);
        for (i, digits) in [score[2], score[1], score[0]].into_iter().enumerate() {
            self.set_high_score_byte(at + i as u8, digits);
        }
        self.vars.high_score_levels[pos as usize] = self.vars.player1.level_number;
        self.high_score_entry_screen().await;
    }

    /// A byte of the high score table's scores (A-type's 12 then B-type's 12).
    fn high_score_byte(&self, i: u8) -> u8 {
        let i = i as usize;
        if i < 12 { self.vars.high_score_scores_a[i] } else { self.vars.high_score_scores_b[i - 12] }
    }

    fn set_high_score_byte(&mut self, i: u8, v: u8) {
        let i = i as usize;
        if i < 12 {
            self.vars.high_score_scores_a[i] = v;
        } else {
            self.vars.high_score_scores_b[i - 12] = v;
        }
    }
}
