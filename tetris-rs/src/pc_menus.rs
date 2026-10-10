//! The app's menus: START / SETTINGS / EXIT on the title screen, and the settings screen.
//!
//! Both are drawn the way the game draws its own menus: text in the menu font, the flickering
//! arrow sprites around the chosen item (hidden one frame in four), and the music box's frame
//! for the settings panel. Only used when `Options::menus` is on, so the original game, which
//! the oracles check, never runs this code.

use crate::flow::{CHR_TITLE_MENU, MENU_CURSOR_MASK, RENDER_LEGAL_AND_TITLE, SFX_MENU_OPTION, SFX_MENU_SCREEN};
use crate::options::{key_label, Config, LeftRight, ACTIONS, BACK};
use crate::saved::NAME_CHARS;
use crate::{buttons, layout, Game};

/// `renderMode` for the settings screen: the menu screens' scroll reset, after writing any
/// changed text into the nametable.
pub(crate) const RENDER_SETTINGS: u8 = 5;

/// The menu arrow, flipped horizontally for the right-hand one.
const ARROW_TILE: u8 = 0x27;
const FLIP_H: u8 = 0x40;
const BLANK_TILE: u8 = 0xFF;

/// "PAUSE" is five sprites at x 112..152, y 119; the hint goes two rows under it.
const PAUSE_CENTRE_X: u8 = 132;
const PAUSE_HINT_Y: u8 = 135;

/// The title menu, left-aligned under the logo where `PUSH START` was (row 20, columns 7..16).
const TITLE_ITEMS: [(&str, u8, u8); 3] = [("START", 18, 8), ("SETTINGS", 20, 8), ("EXIT", 22, 8)];
const PUSH_START: (u8, u8, usize) = (20, 7, 10);

/// The settings panel: the music box's frame around rows 2..=27, columns 2..=29.
const PANEL: (u8, u8, u8, u8) = (2, 2, 27, 29);
const FRAME_TILES: [u8; 8] = [0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F];
const LABEL_COL: u8 = 5;
const VALUE_COL: u8 = 19;
const VALUE_WIDTH: usize = 9;
/// Inside the frame: columns 3..=28.
const INNER: (u8, usize) = (3, 26);
const HEADER_ROW: u8 = 4;
/// Items are double-spaced, like the game's own menus, from row 6.
const FIRST_ROW: u8 = 6;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    Key(usize),
    Volume,
    SaveScores,
    DasDelay,
    DasRepeat,
    LeftRight,
    Slowdown,
    HardDrop,
    Ghost,
    Fullscreen,
    PixelScale,
    Reset,
    /// Go to another page.
    Page(usize),
}

/// The pages: (header, items). The last item of each goes to the next page.
const PAGES: [(&str, &[Item]); 3] = [
    (
        "CONTROLS",
        &[
            Item::Key(0),
            Item::Key(1),
            Item::Key(2),
            Item::Key(3),
            Item::Key(4),
            Item::Key(5),
            Item::Key(6),
            Item::Key(7),
            Item::Key(8),
            Item::Page(1),
        ],
    ),
    (
        "GAME",
        &[
            Item::DasDelay,
            Item::DasRepeat,
            Item::LeftRight,
            Item::Slowdown,
            Item::HardDrop,
            Item::Ghost,
            Item::SaveScores,
            Item::Page(2),
        ],
    ),
    ("SYSTEM", &[Item::Volume, Item::Fullscreen, Item::PixelScale, Item::Reset, Item::Page(0)]),
];

fn item_row(i: usize) -> u8 {
    FIRST_ROW + 2 * i as u8
}

/// The menus' state, outside the original's RAM.
#[derive(Default)]
pub(crate) struct Ui {
    /// The key pressed this frame, by name (for rebinding and Back).
    pub key: Option<String>,
    /// EXIT was chosen.
    pub quit: bool,
    /// The settings changed this frame; the runner hands them to the app.
    pub config_changed: bool,
    title_cursor: usize,
    /// The buttons as the app sent them last frame, and the one of Left/Right pressed last.
    pub raw_buttons: u8,
    pub last_horizontal: u8,
    /// Nametable writes for the next NMI: (PPU address, tiles).
    ppu_queue: Vec<(u16, Vec<u8>)>,
}

impl Item {
    fn label(self) -> &'static str {
        match self {
            Item::Key(i) => ACTIONS[i],
            Item::Volume => "VOLUME",
            Item::SaveScores => "SAVE SCORES",
            Item::DasDelay => "DAS DELAY",
            Item::DasRepeat => "DAS REPEAT",
            Item::LeftRight => "LEFT/RIGHT",
            Item::Slowdown => "SLOWDOWN",
            Item::HardDrop => "HARD DROP",
            Item::Ghost => "GHOST",
            Item::Fullscreen => "FULLSCREEN",
            Item::PixelScale => "PIXEL SCALE",
            Item::Reset => "RESET ALL",
            Item::Page(p) => ["CONTROLS", "GAME SETTINGS", "SYSTEM SETTINGS"][p],
        }
    }

    fn value(self, c: &Config) -> String {
        let on_off = |b: bool| if b { "ON" } else { "OFF" }.to_string();
        match self {
            Item::Key(i) => key_label(&c.keys[i]),
            Item::Volume => c.volume.to_string(),
            Item::SaveScores => on_off(c.save_high_scores),
            Item::DasDelay => c.rules.das_delay.to_string(),
            Item::DasRepeat => c.rules.das_repeat.to_string(),
            Item::LeftRight => match c.rules.left_right {
                LeftRight::Both => "BOTH",
                LeftRight::Last => "LAST",
                LeftRight::Neither => "NEITHER",
            }
            .to_string(),
            Item::Slowdown => on_off(c.rules.slowdown),
            Item::HardDrop => on_off(c.rules.hard_drop),
            Item::Ghost => on_off(c.rules.ghost),
            Item::Fullscreen => on_off(c.fullscreen),
            Item::PixelScale => if c.integer_scale { "WHOLE" } else { "FIT" }.to_string(),
            Item::Reset | Item::Page(_) => String::new(),
        }
    }

    /// Step the setting by `dir` (-1 or 1; Start steps forward, wrapping).
    fn step(self, c: &mut Config, dir: i8, wrap: bool) {
        fn num(v: &mut u8, dir: i8, min: u8, max: u8, wrap: bool) {
            let next = *v as i16 + dir as i16;
            *v = if next > max as i16 {
                if wrap { min } else { max }
            } else if next < min as i16 {
                if wrap { max } else { min }
            } else {
                next as u8
            };
        }
        let r = &mut c.rules;
        match self {
            Item::Volume => num(&mut c.volume, dir, 0, 10, wrap),
            Item::SaveScores => c.save_high_scores = !c.save_high_scores,
            Item::DasDelay => num(&mut r.das_delay, dir, 1, 30, wrap),
            Item::DasRepeat => num(&mut r.das_repeat, dir, 1, 10, wrap),
            Item::LeftRight => {
                let modes = [LeftRight::Both, LeftRight::Last, LeftRight::Neither];
                let i = modes.iter().position(|&m| m == r.left_right).unwrap() as i8;
                r.left_right = modes[(i + dir).rem_euclid(3) as usize];
            }
            Item::Slowdown => r.slowdown = !r.slowdown,
            Item::HardDrop => r.hard_drop = !r.hard_drop,
            Item::Ghost => r.ghost = !r.ghost,
            Item::Fullscreen => c.fullscreen = !c.fullscreen,
            Item::PixelScale => c.integer_scale = !c.integer_scale,
            Item::Key(_) | Item::Reset | Item::Page(_) => {}
        }
    }
}

impl Game {
    /// The title screen with the START / SETTINGS / EXIT menu in place of `PUSH START`, and no
    /// attract demo. Returns A for the main loop, as `gameMode_titleScreen` does.
    pub(crate) async fn title_menu(&mut self) -> u8 {
        self.update_audio2();
        self.vars.render_mode = RENDER_LEGAL_AND_TITLE;
        self.vars.demo_recording = 0;
        self.vars.display_next_piece = 0;
        self.load_screen(
            CHR_TITLE_MENU,
            layout::GAME_MODE_TITLE_SCREEN_COPY_MENU_PALETTE,
            layout::GAME_MODE_TITLE_SCREEN_COPY_TITLE_SCREEN_NAMETABLE,
        )
        .await;
        let (row, col, len) = PUSH_START;
        self.put_text(row, col, &" ".repeat(len));
        for (text, row, col) in TITLE_ITEMS {
            self.put_text(row, col, text);
        }
        self.show_screen().await;
        self.memset_page(0x00, 0x02, 0x02);
        // As the original title screen (whose demo timer this was).
        self.vars.frame_counter.0 &= 0x00FF;
        loop {
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            let pressed = self.vars.newly_pressed_buttons_player1;
            let cursor = &mut self.ui.title_cursor;
            if pressed == buttons::UP || pressed == buttons::DOWN {
                *cursor = (*cursor + if pressed == buttons::UP { 2 } else { 1 }) % 3;
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
            } else if pressed == buttons::START {
                match *cursor {
                    0 => {
                        self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                        self.vars.game_mode = self.vars.game_mode.wrapping_add(1);
                        return SFX_MENU_SCREEN;
                    }
                    1 => {
                        self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                        self.settings_screen().await;
                        // Back to a freshly drawn title screen.
                        return 0xFF;
                    }
                    _ => self.ui.quit = true,
                }
            }
            let (text, row, col) = TITLE_ITEMS[self.ui.title_cursor];
            self.stage_menu_arrows(row, col, text.len());
        }
    }

    /// The settings screen, until Back (or B). Changes take effect, and are saved by the app,
    /// as they're made.
    async fn settings_screen(&mut self) {
        self.vars.render_mode = RENDER_SETTINGS;
        self.load_screen(
            CHR_TITLE_MENU,
            layout::GAME_MODE_GAME_TYPE_MENU_COPY_MENU_PALETTE,
            layout::GAME_MODE_GAME_TYPE_MENU_COPY_GAME_TYPE_MENU_NAMETABLE,
        )
        .await;
        self.draw_panel();
        let mut page = 0;
        for (row, text) in self.page_lines(page) {
            self.put_text(row, INNER.0, &text);
        }
        self.show_screen().await;
        self.memset_page(0x00, 0x02, 0x02);
        let mut cursor = 0;
        let mut capturing = false;
        loop {
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            let pressed = self.vars.newly_pressed_buttons_player1;
            let items = PAGES[page].1;
            let item = items[cursor];
            if capturing {
                if let Some(key) = self.ui.key.take() {
                    let Item::Key(i) = item else { unreachable!() };
                    let mut config = self.options.config.clone();
                    // A key already bound elsewhere swaps with this one.
                    if let Some(j) = config.keys.iter().position(|k| *k == key).filter(|&j| j != i) {
                        config.keys[j] = config.keys[i].clone();
                    }
                    config.keys[i] = key;
                    self.set_config(config);
                    self.queue_page(page);
                    self.vars.sfx_init[1] = SFX_MENU_OPTION;
                    capturing = false;
                }
            } else if self.back_pressed() || pressed == buttons::B {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                self.ui.title_cursor = 1;
                return;
            } else if pressed == buttons::UP || pressed == buttons::DOWN {
                cursor = (cursor + if pressed == buttons::UP { items.len() - 1 } else { 1 }) % items.len();
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
            } else if pressed == buttons::LEFT || pressed == buttons::RIGHT || pressed == buttons::START {
                let start = pressed == buttons::START;
                match item {
                    Item::Key(_) if start => {
                        capturing = true;
                        self.queue_text(item_row(cursor), VALUE_COL, &format!("{:<1$}", "PRESS KEY", VALUE_WIDTH));
                    }
                    Item::Reset if start => {
                        self.set_config(Config::default());
                        self.queue_page(page);
                    }
                    Item::Page(p) if start => {
                        page = p;
                        cursor = 0;
                        self.queue_page(page);
                    }
                    Item::Key(_) | Item::Reset | Item::Page(_) => {}
                    _ => {
                        let mut config = self.options.config.clone();
                        item.step(&mut config, if pressed == buttons::LEFT { -1 } else { 1 }, start);
                        self.queue_text(item_row(cursor), VALUE_COL, &format!("{:<1$}", item.value(&config), VALUE_WIDTH));
                        if config != self.options.config {
                            self.set_config(config);
                        }
                    }
                }
                self.vars.sfx_init[1] = if matches!(item, Item::Page(_)) && start { SFX_MENU_SCREEN } else { SFX_MENU_OPTION };
            }
            self.ui.key = None;
            let item = PAGES[page].1[cursor];
            self.stage_menu_arrows(item_row(cursor), LABEL_COL, item.label().len());
        }
    }

    /// Every line inside the panel for `page`, padded to the panel's width.
    fn page_lines(&self, page: usize) -> Vec<(u8, String)> {
        let (header, items) = PAGES[page];
        let (_, width) = INNER;
        let mut lines = vec![(HEADER_ROW, format!("{:^1$}", header, width))];
        // Every page's rows, so a shorter page blanks what a longer one left.
        let rows = PAGES.iter().map(|(_, items)| items.len()).max().unwrap();
        for i in 0..rows {
            let label_at = (LABEL_COL - INNER.0) as usize;
            let value_at = (VALUE_COL - LABEL_COL) as usize;
            let text = match items.get(i) {
                Some(item) => format!("{:label_at$}{:<value_at$}{}", "", item.label(), item.value(&self.options.config)),
                None => String::new(),
            };
            lines.push((item_row(i), format!("{text:<width$}")));
        }
        lines
    }

    /// Redraw `page` at the next NMI.
    fn queue_page(&mut self, page: usize) {
        for (row, text) in self.page_lines(page) {
            self.queue_text(row, INNER.0, &text);
        }
    }

    /// The legal screen's first wait (255 frames), which Start skips here; the original's
    /// can't be skipped until it's over.
    pub(crate) async fn skippable_legal_wait(&mut self, frames: u8) {
        self.vars.sleep_counter = frames;
        while self.vars.sleep_counter != 0 {
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            if self.vars.newly_pressed_buttons_player1 == buttons::START {
                break;
            }
        }
    }

    /// The key bound to Back was pressed this frame.
    pub(crate) fn back_pressed(&self) -> bool {
        self.ui.key.as_deref() == Some(self.options.config.keys[BACK].as_str())
    }

    /// Under "PAUSE": which key quits the game, as two centred lines of sprites in the same
    /// font (the game's sprites and background share the play screen's tiles).
    pub(crate) fn stage_pause_hint(&mut self) {
        let label = key_label(&self.options.config.keys[BACK]);
        for (text, y) in [(label.as_str(), PAUSE_HINT_Y), ("TO QUIT", PAUSE_HINT_Y + 10)] {
            let tiles = self.text_tiles(text);
            let x0 = PAUSE_CENTRE_X - 4 * tiles.len() as u8;
            for (i, t) in tiles.into_iter().enumerate() {
                if t != BLANK_TILE {
                    self.stage_sprite(y, t, 0, x0 + 8 * i as u8);
                }
            }
        }
    }

    fn set_config(&mut self, config: Config) {
        self.options.config = config;
        self.ui.config_changed = true;
    }

    /// `render` for the settings screen: write the changed text, then reset the scroll as the
    /// menu screens do.
    pub(crate) fn render_mode_settings(&mut self) {
        for (addr, tiles) in std::mem::take(&mut self.ui.ppu_queue) {
            self.ppu_addr(addr);
            for t in tiles {
                self.ppu_data(t);
            }
        }
        self.render_mode_menu_screens();
    }

    /// The two arrows either side of `len` characters at (row, col), unless this is the frame
    /// in four they're hidden.
    fn stage_menu_arrows(&mut self, row: u8, col: u8, len: usize) {
        if self.vars.frame_counter.0 as u8 & MENU_CURSOR_MASK == 0 {
            return;
        }
        let y = row * 8 - 1;
        let x = col * 8;
        self.stage_sprite(y, ARROW_TILE, 0, x - 9);
        self.stage_sprite(y, ARROW_TILE, FLIP_H, x + 8 * len as u8 + 1);
    }

    fn stage_sprite(&mut self, y: u8, tile: u8, attributes: u8, x: u8) {
        let at = self.vars.oam_staging_length as usize;
        self.vars.oam_staging[at..at + 4].copy_from_slice(&[y, tile, attributes, x]);
        self.vars.oam_staging_length = self.vars.oam_staging_length.wrapping_add(4);
    }

    fn text_tiles(&self, text: &str) -> Vec<u8> {
        text.chars()
            .map(|c| match NAME_CHARS.find(c) {
                Some(i) => self.rom.table(layout::HIGH_SCORE_CHAR_TO_TILE, i),
                None => BLANK_TILE,
            })
            .collect()
    }

    /// Write text straight to the nametable (rendering off).
    fn put_text(&mut self, row: u8, col: u8, text: &str) {
        self.ppu_addr(nametable_addr(row, col));
        for t in self.text_tiles(text) {
            self.ppu_data(t);
        }
    }

    /// Write text at the next NMI.
    fn queue_text(&mut self, row: u8, col: u8, text: &str) {
        let tiles = self.text_tiles(text);
        self.ui.ppu_queue.push((nametable_addr(row, col), tiles));
    }

    /// Clear the panel, frame it with the music box's border, and give it the box's palette
    /// (0: white text, yellow frame).
    fn draw_panel(&mut self) {
        let (top, left, bottom, right) = PANEL;
        let [tl, t, tr, l, r, bl, b, br] = FRAME_TILES;
        for row in top..=bottom {
            self.ppu_addr(nametable_addr(row, left));
            for col in left..=right {
                let tile = match (row, col) {
                    (r0, c0) if r0 == top && c0 == left => tl,
                    (r0, c0) if r0 == top && c0 == right => tr,
                    (r0, _) if r0 == top => t,
                    (r0, c0) if r0 == bottom && c0 == left => bl,
                    (r0, c0) if r0 == bottom && c0 == right => br,
                    (r0, _) if r0 == bottom => b,
                    (_, c0) if c0 == left => l,
                    (_, c0) if c0 == right => r,
                    _ => BLANK_TILE,
                };
                self.ppu_data(tile);
            }
        }
        // Attributes: each byte covers 4x4 tiles, two bits per 2x2 quarter.
        for i in 0..64u16 {
            let (arow, acol) = ((i / 8) as u8 * 4, (i % 8) as u8 * 4);
            let mut attr = self.ppu.vram[0x3C0 + i as usize];
            for q in 0..4 {
                let (qr, qc) = (arow + (q / 2) * 2, acol + (q % 2) * 2);
                if qr >= top && qr + 1 <= bottom && qc >= left && qc + 1 <= right {
                    attr &= !(3 << (q * 2));
                }
            }
            self.ppu_addr(0x23C0 + i);
            self.ppu_data(attr);
        }
    }
}

fn nametable_addr(row: u8, col: u8) -> u16 {
    0x2000 + row as u16 * 32 + col as u16
}
