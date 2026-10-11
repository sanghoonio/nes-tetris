//! The app's menus: START / SETTINGS / EXIT on the title screen, the settings screen, the
//! pause menu (RESUME / QUIT) and typing names on the high score screen.
//!
//! Both are drawn the way the game draws its own menus: text in the menu font, the flickering
//! arrow sprites around the chosen item (hidden one frame in four), and the music box's frame
//! for the settings panel. Only used when `Options::menus` is on, so the original game, which
//! the oracles check, never runs this code. They read the menu keys directly (`pc_input`).

use crate::flow::{CHR_TITLE_MENU, MENU_CURSOR_MASK, RENDER_LEGAL_AND_TITLE, SFX_MENU_OPTION, SFX_MENU_SCREEN};
use crate::options::{key_label, Config, LeftRight, ACTIONS};
use crate::pc_input::{Keys, MenuKey, Screen};
use crate::saved::NAME_CHARS;
use crate::{buttons, layout, Game};

/// `renderMode` for the settings screen: the menu screens' scroll reset, after writing any
/// changed text into the nametable.
pub(crate) const RENDER_SETTINGS: u8 = 5;

/// The menu arrow, flipped horizontally for the right-hand one.
const ARROW_TILE: u8 = 0x27;
const FLIP_H: u8 = 0x40;
const BLANK_TILE: u8 = 0xFF;

/// The pause screen's title and menu, centred across where the original's "PAUSE" is (x 112..152)
/// and a row above it (y 119). The title says "PAUSED", with a blank row before the menu.
const PAUSE_TITLE: (&str, u8) = ("PAUSED", 103);
const PAUSE_ITEMS: [(&str, u8); 2] = [("RESUME", 127), ("QUIT", 139)];
const PAUSE_CENTRE: u8 = 132;

/// The title menu, left-aligned under the logo where `PUSH START` was (row 20, columns 7..16).
#[cfg(not(feature = "versus"))]
const TITLE_ITEMS: [(&str, u8, u8); 3] = [("START", 18, 7), ("SETTINGS", 20, 7), ("EXIT", 22, 7)];
/// With VERSUS, four items, starting a row higher.
#[cfg(feature = "versus")]
const TITLE_ITEMS: [(&str, u8, u8); 4] = [("START", 16, 7), ("VERSUS", 18, 7), ("SETTINGS", 20, 7), ("EXIT", 22, 7)];
/// What each title item does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TitleItem {
    Start,
    #[cfg(feature = "versus")]
    Versus,
    Settings,
    Exit,
}
#[cfg(not(feature = "versus"))]
const TITLE_ACTIONS: [TitleItem; 3] = [TitleItem::Start, TitleItem::Settings, TitleItem::Exit];
#[cfg(feature = "versus")]
const TITLE_ACTIONS: [TitleItem; 4] = [TitleItem::Start, TitleItem::Versus, TitleItem::Settings, TitleItem::Exit];
const PUSH_START: (u8, u8, usize) = (20, 7, 10);

/// The level grid's first digit cell (row, column); cells are every other column and row.
const LEVEL_GRID: (usize, usize) = (11, 7);

/// Digits 3 pixels wide and 7 high (the menu font's digits are 7 high), one row per byte,
/// left pixel in bit 2.
const NARROW_DIGITS: [[u8; 7]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b001, 0b111, 0b100, 0b100, 0b111],
    [0b111, 0b001, 0b001, 0b111, 0b001, 0b001, 0b111],
    [0b101, 0b101, 0b101, 0b111, 0b001, 0b001, 0b001],
    [0b111, 0b100, 0b100, 0b111, 0b001, 0b001, 0b111],
    [0b111, 0b100, 0b100, 0b111, 0b101, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b101, 0b111, 0b101, 0b101, 0b111],
    [0b111, 0b101, 0b101, 0b111, 0b001, 0b001, 0b111],
];

/// The settings panel: the music box's frame around rows 2..=27, columns 2..=29.
const PANEL: (u8, u8, u8, u8) = (2, 2, 27, 29);
const FRAME_TILES: [u8; 8] = [0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F];
pub(crate) const LABEL_COL: u8 = 5;
pub(crate) const VALUE_COL: u8 = 19;
pub(crate) const VALUE_WIDTH: usize = 9;
/// Inside the frame: columns 3..=28.
pub(crate) const INNER: (u8, usize) = (3, 26);
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
    NextPiece,
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
            Item::NextPiece,
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
    /// The keys this frame.
    pub keys: Keys,
    /// The screen up, for what the keys mean (`pc_input`).
    pub screen: Screen,
    pub last_mapping: Option<crate::pc_input::Mapping>,
    /// Keys held across the last change of meaning, ignored until released.
    pub stale: Vec<String>,
    /// Name entry: move the cursor on after the letter typed last frame has been drawn.
    name_advance: bool,
    /// EXIT was chosen.
    pub quit: bool,
    /// A game started with `Runner::start_game` has ended.
    pub game_over: bool,
    /// The settings changed this frame; the runner hands them to the app.
    pub config_changed: bool,
    title_cursor: usize,
    /// The buttons as the app sent them last frame, and the one of Left/Right pressed last.
    pub raw_buttons: u8,
    pub last_horizontal: u8,
    /// Nametable writes for the next NMI: (PPU address, tiles).
    ppu_queue: Vec<(u16, Vec<u8>)>,
    /// What the app last handed the VERSUS screens, and what the player asked for since.
    #[cfg(feature = "versus")]
    pub versus: Option<crate::versus::VersusView>,
    #[cfg(feature = "versus")]
    pub versus_requests: Vec<crate::versus::VersusRequest>,
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
            Item::NextPiece => "NEXT PIECE",
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
            Item::NextPiece => if c.next_piece { "SHOW" } else { "HIDE" }.to_string(),
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
            Item::NextPiece => c.next_piece = !c.next_piece,
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
        self.ui.screen = Screen::TitleMenu;
        self.update_audio2();
        self.vars.render_mode = RENDER_LEGAL_AND_TITLE;
        self.vars.demo_recording = 0;
        // The next-piece preview is a setting here (Select toggles it in the original).
        self.vars.display_next_piece = !self.options.config.next_piece as u8;
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
            let keys = self.menu_pressed();
            let cursor = &mut self.ui.title_cursor;
            let n = TITLE_ITEMS.len();
            if keys.contains(&MenuKey::Up) || keys.contains(&MenuKey::Down) {
                *cursor = (*cursor + if keys.contains(&MenuKey::Up) { n - 1 } else { 1 }) % n;
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
            } else if keys.contains(&MenuKey::Confirm) {
                match TITLE_ACTIONS[*cursor] {
                    TitleItem::Start => {
                        self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                        self.vars.game_mode = self.vars.game_mode.wrapping_add(1);
                        self.ui.screen = Screen::Game;
                        return SFX_MENU_SCREEN;
                    }
                    #[cfg(feature = "versus")]
                    TitleItem::Versus => {
                        self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                        self.versus_menu().await;
                        return 0xFF;
                    }
                    TitleItem::Settings => {
                        self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                        self.settings_screen().await;
                        // Back to a freshly drawn title screen.
                        return 0xFF;
                    }
                    TitleItem::Exit => self.ui.quit = true,
                }
            }
            let (text, row, col) = TITLE_ITEMS[self.ui.title_cursor];
            self.stage_menu_arrows(row, col, text.len());
        }
    }

    /// The settings screen, until Back (or B). Changes take effect, and are saved by the app,
    /// as they're made.
    async fn settings_screen(&mut self) {
        self.ui.screen = Screen::Settings;
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
            let keys = self.menu_pressed();
            let has = |k| keys.contains(&k);
            let items = PAGES[page].1;
            let item = items[cursor];
            if capturing {
                if let Some(key) = self.ui.keys.pressed.first().cloned() {
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
            } else if has(MenuKey::Back) {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                self.ui.title_cursor = TITLE_ACTIONS.iter().position(|&a| a == TitleItem::Settings).unwrap();
                self.ui.screen = Screen::TitleMenu;
                return;
            } else if has(MenuKey::Up) || has(MenuKey::Down) {
                cursor = (cursor + if has(MenuKey::Up) { items.len() - 1 } else { 1 }) % items.len();
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
            } else if has(MenuKey::Left) || has(MenuKey::Right) || has(MenuKey::Confirm) {
                let start = has(MenuKey::Confirm);
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
                        item.step(&mut config, if has(MenuKey::Left) { -1 } else { 1 }, start);
                        self.queue_text(item_row(cursor), VALUE_COL, &format!("{:<1$}", item.value(&config), VALUE_WIDTH));
                        if config != self.options.config {
                            self.set_config(config);
                        }
                    }
                }
                self.vars.sfx_init[1] = if matches!(item, Item::Page(_)) && start { SFX_MENU_SCREEN } else { SFX_MENU_OPTION };
            }
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

    /// While Shift is held on the level menu, the level grid reads 10..19 (Shift+Enter starts
    /// there). Two digits don't fit a cell in the NES font, so the app draws them over the
    /// finished picture in a narrow font of its own, the original digits' height, in each
    /// cell's own colours.
    pub(crate) fn draw_overlays(&self, pixels: &mut [u8]) {
        if self.vars.game_mode != 3 || !self.shift_held() || self.vars.current_ppu_mask & 0x08 == 0 {
            return;
        }
        for level in 0..10usize {
            let (row, col) = (LEVEL_GRID.0 + 2 * (level / 5), LEVEL_GRID.1 + 2 * (level % 5));
            let (x0, y0) = (col * 8, row * 8);
            let cell = |x: usize, y: usize| (y0 + y) * crate::frame::WIDTH + x0 + x;
            // The cell's colours: the commonest is its background, the next its digit.
            let mut counts = [0usize; 64];
            for y in 0..8 {
                for x in 0..8 {
                    counts[(pixels[cell(x, y)] & 0x3F) as usize] += 1;
                }
            }
            let mut order: Vec<usize> = (0..64).filter(|&c| counts[c] > 0).collect();
            order.sort_by_key(|&c| std::cmp::Reverse(counts[c]));
            let (Some(&bg), Some(&fg)) = (order.first(), order.get(1)) else { continue };
            for y in 0..8 {
                for x in 0..8 {
                    pixels[cell(x, y)] = bg as u8;
                }
            }
            for (i, digit) in [1, level].into_iter().enumerate() {
                for (y, bits) in NARROW_DIGITS[digit].iter().enumerate() {
                    for x in 0..3 {
                        if bits >> (2 - x) & 1 != 0 {
                            pixels[cell(i * 4 + x, y)] = fg as u8;
                        }
                    }
                }
            }
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

    /// The pause screen's menu (the app's): RESUME / QUIT under "PAUSED", the same arrows.
    /// Enter chooses, Escape resumes. Returns whether to quit the game.
    pub(crate) async fn pause_menu(&mut self) -> bool {
        self.ui.screen = Screen::Pause;
        let mut cursor = 0;
        let quit = loop {
            let left = |text: &str| PAUSE_CENTRE - 4 * text.len() as u8;
            for (text, y) in std::iter::once(PAUSE_TITLE).chain(PAUSE_ITEMS) {
                for (i, t) in self.text_tiles(text).into_iter().enumerate() {
                    self.stage_sprite(y, t, 0, left(text) + 8 * i as u8);
                }
            }
            if self.vars.frame_counter.0 as u8 & MENU_CURSOR_MASK != 0 {
                let (text, y) = PAUSE_ITEMS[cursor];
                self.stage_sprite(y, ARROW_TILE, 0, left(text) - 9);
                self.stage_sprite(y, ARROW_TILE, FLIP_H, left(text) + 8 * text.len() as u8 + 1);
            }
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            let keys = self.menu_pressed();
            if keys.contains(&MenuKey::Back) {
                break false;
            }
            if keys.contains(&MenuKey::Confirm) {
                break cursor == 1;
            }
            if keys.contains(&MenuKey::Up) || keys.contains(&MenuKey::Down) {
                cursor ^= 1;
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
            }
        };
        self.ui.screen = Screen::Game;
        quit
    }

    /// Name entry by typing (the app's): a letter, digit or `-,/()". ` replaces the one under
    /// the cursor, which moves on once it's drawn; Backspace moves back and blanks.
    pub(crate) fn type_name(&mut self) {
        const SPACE: u8 = 43;
        if std::mem::take(&mut self.ui.name_advance) {
            let next = self.vars.high_score_entry_name_offset_for_letter + 1;
            self.vars.high_score_entry_name_offset_for_letter = if next < 6 { next } else { 0 };
        }
        let Some(key) = self.ui.keys.pressed.first().cloned() else { return };
        let typed = match key.as_str() {
            "Backspace" => {
                let at = self.vars.high_score_entry_name_offset_for_letter;
                self.vars.high_score_entry_name_offset_for_letter = if at == 0 { 5 } else { at - 1 };
                let i = self.entered_letter_index();
                self.vars.high_score_names[i] = SPACE;
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
                None
            }
            k => self.typed_char(k),
        };
        if let Some(c) = typed.and_then(|c| NAME_CHARS.find(c)) {
            let i = self.entered_letter_index();
            self.vars.high_score_names[i] = c as u8;
            self.vars.sfx_init[1] = SFX_MENU_OPTION;
            self.ui.name_advance = true;
        }
    }

    /// The character a key types (letters, digits, `-,/().` and space; Shift+9/0 for brackets).
    pub(crate) fn typed_char(&self, key: &str) -> Option<char> {
        let shift = self.shift_held();
        match key {
            k if k.len() == 4 && k.starts_with("Key") => k.chars().nth(3),
            "Digit9" if shift => Some('('),
            "Digit0" if shift => Some(')'),
            k if k.starts_with("Digit") || k.starts_with("Numpad") && k.len() == 7 => k.chars().last(),
            "Minus" => Some('-'),
            "Comma" => Some(','),
            "Slash" => Some('/'),
            "Period" => Some('.'),
            "Quote" => Some('"'),
            "Space" => Some(' '),
            _ => None,
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
    pub(crate) fn stage_menu_arrows(&mut self, row: u8, col: u8, len: usize) {
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

    pub(crate) fn text_tiles(&self, text: &str) -> Vec<u8> {
        text.chars()
            .map(|c| match NAME_CHARS.find(c) {
                Some(i) => self.rom.table(layout::HIGH_SCORE_CHAR_TO_TILE, i),
                None => BLANK_TILE,
            })
            .collect()
    }

    /// Write text straight to the nametable (rendering off).
    pub(crate) fn put_text(&mut self, row: u8, col: u8, text: &str) {
        self.ppu_addr(nametable_addr(row, col));
        for t in self.text_tiles(text) {
            self.ppu_data(t);
        }
    }

    /// Write text at the next NMI.
    pub(crate) fn queue_text(&mut self, row: u8, col: u8, text: &str) {
        let tiles = self.text_tiles(text);
        self.ui.ppu_queue.push((nametable_addr(row, col), tiles));
    }

    /// Clear the panel, frame it with the music box's border, and give it the box's palette
    /// (0: white text, yellow frame).
    pub(crate) fn draw_panel(&mut self) {
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
