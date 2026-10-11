//! The VERSUS screens (the app's, with the `versus` feature): the VERSUS menu (HOST / JOIN /
//! WATCH / NAME / BACK), room-code entry, and the lobby. Drawn like the settings screen: the menu
//! screen with the music box's panel, the menu font, the flickering arrows and the menu sounds.
//!
//! The game knows nothing of the network: each frame the app hands it a `VersusView`
//! (`Ui::versus`), and the player's choices go back as `VersusRequest`s (`Ui::versus_requests`).
//! While a match is on the app doesn't run the game at all, so the lobby simply carries on when
//! the match is over.

use crate::flow::{CHR_TITLE_MENU, SFX_MENU_OPTION, SFX_MENU_SCREEN};
use crate::options::LeftRight;
use crate::pc_input::{MenuKey, Screen};
use crate::pc_menus::{INNER, LABEL_COL, RENDER_SETTINGS, VALUE_COL, VALUE_WIDTH};
use crate::versus::{Connection, RoomSettings, VersusRequest, VersusView, CODE_LEN, CODE_LETTERS, NAME_LEN};
use crate::{layout, Game};

const HEADER_ROW: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuItem {
    Host,
    Join,
    Watch,
    Name,
    Back,
}

const MENU: [(MenuItem, &str, u8); 5] = [
    (MenuItem::Host, "HOST", 8),
    (MenuItem::Join, "JOIN", 10),
    (MenuItem::Watch, "WATCH", 12),
    (MenuItem::Name, "NAME", 14),
    (MenuItem::Back, "BACK", 16),
];

/// The lobby's rows: players, the room's settings (single-spaced), READY, LEAVE, the status.
const PLAYER_ROWS: [u8; 2] = [6, 7];
const WATCHING_ROW: u8 = 8;
const FIRST_SETTING_ROW: u8 = 10;
const READY_ROW: u8 = 19;
const LEAVE_ROW: u8 = 21;
const STATUS_ROWS: [u8; 3] = [23, 24, 25];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Setting {
    Level,
    Pieces,
    DasDelay,
    DasRepeat,
    LeftRight,
    HardDrop,
    Ghost,
    NextPiece,
}

const SETTINGS: [Setting; 8] = [
    Setting::Level,
    Setting::Pieces,
    Setting::DasDelay,
    Setting::DasRepeat,
    Setting::LeftRight,
    Setting::HardDrop,
    Setting::Ghost,
    Setting::NextPiece,
];

impl Setting {
    fn label(self) -> &'static str {
        match self {
            Setting::Level => "LEVEL",
            Setting::Pieces => "PIECES",
            Setting::DasDelay => "DAS DELAY",
            Setting::DasRepeat => "DAS REPEAT",
            Setting::LeftRight => "LEFT/RIGHT",
            Setting::HardDrop => "HARD DROP",
            Setting::Ghost => "GHOST",
            Setting::NextPiece => "NEXT PIECE",
        }
    }

    fn value(self, s: &RoomSettings) -> String {
        let on_off = |b: bool| if b { "ON" } else { "OFF" }.to_string();
        match self {
            Setting::Level => s.level.to_string(),
            Setting::Pieces => if s.identical { "SAME" } else { "OWN" }.to_string(),
            Setting::DasDelay => s.das_delay.to_string(),
            Setting::DasRepeat => s.das_repeat.to_string(),
            Setting::LeftRight => match s.left_right {
                LeftRight::Both => "BOTH",
                LeftRight::Last => "LAST",
                LeftRight::Neither => "NEITHER",
            }
            .to_string(),
            Setting::HardDrop => on_off(s.hard_drop),
            Setting::Ghost => on_off(s.ghost),
            Setting::NextPiece => if s.next_piece { "SHOW" } else { "HIDE" }.to_string(),
        }
    }

    /// Step by `dir` (-1 or 1), wrapping when `wrap` (Enter steps forward, wrapping).
    fn step(self, s: &mut RoomSettings, dir: i8, wrap: bool) {
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
        match self {
            Setting::Level => num(&mut s.level, dir, 0, 19, wrap),
            Setting::Pieces => s.identical = !s.identical,
            Setting::DasDelay => num(&mut s.das_delay, dir, 1, 30, wrap),
            Setting::DasRepeat => num(&mut s.das_repeat, dir, 1, 10, wrap),
            Setting::LeftRight => {
                let modes = [LeftRight::Both, LeftRight::Last, LeftRight::Neither];
                let i = modes.iter().position(|&m| m == s.left_right).unwrap() as i8;
                s.left_right = modes[(i + dir).rem_euclid(3) as usize];
            }
            Setting::HardDrop => s.hard_drop = !s.hard_drop,
            Setting::Ghost => s.ghost = !s.ghost,
            Setting::NextPiece => s.next_piece = !s.next_piece,
        }
    }
}

/// What the lobby's cursor can be on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LobbyItem {
    Setting(Setting),
    Ready,
    Leave,
}

impl LobbyItem {
    fn row(self) -> u8 {
        match self {
            LobbyItem::Setting(s) => FIRST_SETTING_ROW + SETTINGS.iter().position(|&x| x == s).unwrap() as u8,
            LobbyItem::Ready => READY_ROW,
            LobbyItem::Leave => LEAVE_ROW,
        }
    }

    fn label(self) -> &'static str {
        match self {
            LobbyItem::Setting(s) => s.label(),
            LobbyItem::Ready => "READY",
            LobbyItem::Leave => "LEAVE",
        }
    }
}

impl Game {
    /// The view the app handed over this frame.
    fn view(&self) -> VersusView {
        self.ui.versus.clone().unwrap_or_default()
    }

    fn request(&mut self, r: VersusRequest) {
        self.ui.versus_requests.push(r);
    }

    /// The menu screen with an empty panel, ready for text.
    async fn versus_panel(&mut self) {
        self.vars.render_mode = RENDER_SETTINGS;
        self.load_screen(
            CHR_TITLE_MENU,
            layout::GAME_MODE_GAME_TYPE_MENU_COPY_MENU_PALETTE,
            layout::GAME_MODE_GAME_TYPE_MENU_COPY_GAME_TYPE_MENU_NAMETABLE,
        )
        .await;
        self.draw_panel();
    }

    /// A line of the panel, centred.
    fn centred(text: &str) -> String {
        format!("{:^1$}", text, INNER.1)
    }

    /// A label and a value in the settings screen's columns, padded to the panel's width.
    fn label_value(label: &str, value: &str) -> String {
        let label_at = (LABEL_COL - INNER.0) as usize;
        let value_at = (VALUE_COL - LABEL_COL) as usize;
        let label: String = label.chars().take(value_at - 1).collect();
        let line = format!("{:label_at$}{:<value_at$}{:<VALUE_WIDTH$}", "", label, value);
        format!("{:<1$}", line, INNER.1)
    }

    /// The VERSUS menu, until BACK (or Escape).
    pub(crate) async fn versus_menu(&mut self) {
        self.ui.screen = Screen::Versus;
        let mut cursor = 0;
        let mut editing_name: Option<String> = None;
        'screen: loop {
            self.versus_panel().await;
            self.put_text(HEADER_ROW, INNER.0, &Self::centred("VERSUS"));
            let name = self.view().name;
            for (item, label, row) in MENU {
                let value = if item == MenuItem::Name { name.as_str() } else { "" };
                self.put_text(row, INNER.0, &Self::label_value(label, value));
            }
            self.show_screen().await;
            self.memset_page(0x00, 0x02, 0x02);
            loop {
                self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
                if let Some(name) = editing_name.as_mut() {
                    let done = self.edit_text(name, NAME_LEN, |c| crate::saved::NAME_CHARS.contains(c) && c != ',');
                    self.queue_text(MENU[3].2, INNER.0, &Self::label_value("NAME", &format!("{name}_")));
                    if done {
                        let name = editing_name.take().unwrap().trim().to_string();
                        self.queue_text(MENU[3].2, INNER.0, &Self::label_value("NAME", &name));
                        self.request(VersusRequest::SetName(name));
                    }
                    continue;
                }
                let keys = self.menu_pressed();
                let has = |k| keys.contains(&k);
                if has(MenuKey::Back) {
                    self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                    break 'screen;
                } else if has(MenuKey::Up) || has(MenuKey::Down) {
                    cursor = (cursor + if has(MenuKey::Up) { MENU.len() - 1 } else { 1 }) % MENU.len();
                    self.vars.sfx_init[1] = SFX_MENU_OPTION;
                } else if has(MenuKey::Confirm) {
                    self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                    match MENU[cursor].0 {
                        MenuItem::Host => {
                            self.request(VersusRequest::Host);
                            self.lobby().await;
                            continue 'screen;
                        }
                        MenuItem::Join | MenuItem::Watch => {
                            let watch = MENU[cursor].0 == MenuItem::Watch;
                            if let Some(code) = self.code_entry(watch).await {
                                self.request(if watch { VersusRequest::Watch(code) } else { VersusRequest::Join(code) });
                                self.lobby().await;
                            }
                            continue 'screen;
                        }
                        MenuItem::Name => {
                            self.vars.sfx_init[1] = SFX_MENU_OPTION;
                            editing_name = Some(self.view().name);
                        }
                        MenuItem::Back => break 'screen,
                    }
                }
                let (_, label, row) = MENU[cursor];
                self.stage_menu_arrows(row, LABEL_COL, label.len());
            }
        }
        self.ui.screen = Screen::TitleMenu;
    }

    /// Typing into `text` (at most `max` characters that pass `allowed`, upper case): a key adds
    /// its character, Backspace removes the last. Returns true once Enter (or Escape) ends it.
    fn edit_text(&mut self, text: &mut String, max: usize, allowed: impl Fn(char) -> bool) -> bool {
        let pressed = self.ui.keys.pressed.clone();
        for key in pressed {
            match key.as_str() {
                "Enter" | "NumpadEnter" | "Escape" | "PadSouth" | "PadEast" => {
                    self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                    return true;
                }
                "Backspace" => {
                    if text.pop().is_some() {
                        self.vars.sfx_init[1] = SFX_MENU_OPTION;
                    }
                }
                k => {
                    if let Some(c) = self.typed_char(k).filter(|&c| allowed(c)) {
                        if text.chars().count() < max {
                            text.push(c);
                            self.vars.sfx_init[1] = SFX_MENU_OPTION;
                        }
                    }
                }
            }
        }
        false
    }

    /// Typing a room code: four letters. None if backed out of.
    async fn code_entry(&mut self, watch: bool) -> Option<String> {
        self.versus_panel().await;
        self.put_text(HEADER_ROW, INNER.0, &Self::centred(if watch { "WATCH" } else { "JOIN" }));
        self.put_text(10, INNER.0, &Self::centred("ROOM CODE"));
        self.put_text(12, INNER.0, &Self::centred("_ _ _ _"));
        self.put_text(16, INNER.0, &Self::centred("TYPE IT, THEN ENTER"));
        self.show_screen().await;
        self.memset_page(0x00, 0x02, 0x02);
        let mut code = String::new();
        loop {
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            let before = code.clone();
            let keys = self.menu_pressed();
            if keys.contains(&MenuKey::Back) {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                return None;
            }
            if keys.contains(&MenuKey::Confirm) && code.len() == CODE_LEN {
                return Some(code);
            }
            // Letters only (Enter and Escape are handled above).
            let pressed = self.ui.keys.pressed.clone();
            for key in pressed {
                if key == "Backspace" {
                    code.pop();
                } else if let Some(c) = self.typed_char(&key).filter(|c| CODE_LETTERS.contains(*c)) {
                    if code.len() < CODE_LEN {
                        code.push(c);
                    }
                }
            }
            if code != before {
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
                let shown: Vec<String> = (0..CODE_LEN).map(|i| code.chars().nth(i).map_or("_".into(), |c| c.to_string())).collect();
                self.queue_text(12, INNER.0, &Self::centred(&shown.join(" ")));
            }
        }
    }

    /// Everything in the lobby's panel for `view`, line by line.
    fn lobby_lines(view: &VersusView) -> Vec<(u8, String)> {
        let header = match &view.connection {
            Connection::InRoom => format!("ROOM {}", view.code),
            Connection::Connecting => "CONNECTING".to_string(),
            Connection::None | Connection::Lost(_) => "NOT CONNECTED".to_string(),
        };
        let mut lines = vec![(HEADER_ROW, Self::centred(&header))];
        for (i, row) in PLAYER_ROWS.into_iter().enumerate() {
            let text = match &view.players[i] {
                Some((name, ready)) => {
                    let you = if view.seat == Some(i as u8) { " (YOU)" } else { "" };
                    Self::label_value(&format!("{name}{you}"), if *ready { "READY" } else { "" })
                }
                None => Self::label_value("-", ""),
            };
            lines.push((row, text));
        }
        let watching = match view.watching {
            0 => String::new(),
            n => format!("{n} WATCHING"),
        };
        lines.push((WATCHING_ROW, Self::label_value(&watching, "")));
        for s in SETTINGS {
            lines.push((LobbyItem::Setting(s).row(), Self::label_value(s.label(), &s.value(&view.settings))));
        }
        let ready = view.seat.and_then(|s| view.players[s as usize].as_ref()).map(|p| p.1);
        let ready_line = match ready {
            Some(r) => Self::label_value("READY", if r { "YES" } else { "NO" }),
            None => Self::label_value("", ""),
        };
        lines.push((READY_ROW, ready_line));
        lines.push((LEAVE_ROW, Self::label_value("LEAVE", "")));
        // The status, word-wrapped onto three lines.
        let status = match &view.connection {
            Connection::Lost(why) => why.clone(),
            _ => view.status.clone(),
        };
        let mut wrapped: Vec<String> = vec![String::new()];
        for word in status.split_whitespace() {
            let last = wrapped.last_mut().unwrap();
            if !last.is_empty() && last.len() + 1 + word.len() > INNER.1 {
                wrapped.push(word.to_string());
            } else {
                if !last.is_empty() {
                    last.push(' ');
                }
                last.push_str(word);
            }
        }
        for (i, row) in STATUS_ROWS.into_iter().enumerate() {
            lines.push((row, Self::centred(wrapped.get(i).map_or("", String::as_str))));
        }
        lines
    }

    /// What the cursor can reach: the host gets the settings too; a watcher only LEAVE.
    fn lobby_items(view: &VersusView) -> Vec<LobbyItem> {
        let mut items = vec![];
        if view.connection == Connection::InRoom {
            if view.seat == Some(0) {
                items.extend(SETTINGS.map(LobbyItem::Setting));
            }
            if view.seat.is_some() {
                items.push(LobbyItem::Ready);
            }
        }
        items.push(LobbyItem::Leave);
        items
    }

    /// The lobby, until LEAVE (or Escape).
    async fn lobby(&mut self) {
        self.versus_panel().await;
        let mut shown = Self::lobby_lines(&self.view());
        for (row, text) in &shown {
            self.put_text(*row, INNER.0, text);
        }
        self.show_screen().await;
        self.memset_page(0x00, 0x02, 0x02);
        let mut at = LobbyItem::Leave;
        let mut had_ready = false;
        // The host's changes not yet confirmed by the room: shown, and stepped from, meanwhile
        // (a quick run of key presses shouldn't each start from the room's last word).
        let mut pending: Option<RoomSettings> = None;
        loop {
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            let mut view = self.view();
            if pending == Some(view.settings) || view.connection != Connection::InRoom {
                pending = None;
            }
            if let Some(p) = pending {
                view.settings = p;
            }
            // Redraw what changed.
            let lines = Self::lobby_lines(&view);
            for (line, old) in lines.iter().zip(&shown) {
                if line != old {
                    self.queue_text(line.0, INNER.0, &line.1);
                }
            }
            shown = lines;
            let items = Self::lobby_items(&view);
            // Arriving in the room (seated): the cursor starts on READY.
            let has_ready = items.contains(&LobbyItem::Ready);
            if has_ready && !had_ready {
                at = LobbyItem::Ready;
            }
            had_ready = has_ready;
            if !items.contains(&at) {
                at = *items.iter().find(|i| **i == LobbyItem::Ready).unwrap_or(&items[0]);
            }
            let cursor = items.iter().position(|&i| i == at).unwrap();
            let keys = self.menu_pressed();
            let has = |k| keys.contains(&k);
            if has(MenuKey::Back) || (has(MenuKey::Confirm) && at == LobbyItem::Leave) {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                self.request(VersusRequest::Leave);
                return;
            } else if has(MenuKey::Up) || has(MenuKey::Down) {
                at = items[(cursor + if has(MenuKey::Up) { items.len() - 1 } else { 1 }) % items.len()];
                self.vars.sfx_init[1] = SFX_MENU_OPTION;
            } else if has(MenuKey::Left) || has(MenuKey::Right) || has(MenuKey::Confirm) {
                match at {
                    LobbyItem::Setting(s) => {
                        let mut settings = view.settings;
                        s.step(&mut settings, if has(MenuKey::Left) { -1 } else { 1 }, has(MenuKey::Confirm));
                        if settings != view.settings {
                            self.request(VersusRequest::Settings(settings));
                            pending = Some(settings);
                            self.vars.sfx_init[1] = SFX_MENU_OPTION;
                        }
                    }
                    LobbyItem::Ready if has(MenuKey::Confirm) => {
                        let ready = view.seat.and_then(|s| view.players[s as usize].as_ref()).is_some_and(|p| p.1);
                        self.request(VersusRequest::Ready(!ready));
                        self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                    }
                    _ => {}
                }
            }
            self.stage_menu_arrows(at.row(), LABEL_COL, at.label().len());
        }
    }
}
