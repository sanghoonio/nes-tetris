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
use crate::pc_menus::{BACK_ROW, HALF, INNER, LABEL_COL, RENDER_SETTINGS, VALUE_COL, VALUE_WIDTH};
use crate::versus::{
    Connection, ListedRoom, ListedState, RoomList, RoomSettings, VersusRequest, VersusView, CODE_LEN, CODE_LETTERS, NAME_LEN,
};
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
    (MenuItem::Back, "BACK", BACK_ROW),
];

/// The JOIN and WATCH lists: a room on each of these rows, which of them are showing when there
/// are more, ENTER A CODE, and BACK on `BACK_ROW`.
const LIST_ROWS: [u8; 7] = [6, 8, 10, 12, 14, 16, 18];
const SHOWING_ROW: u8 = 20;
const CODE_ROW: u8 = 23;

/// The lobby's rows under the room code: the status, the players, the series, ROOM SETTINGS,
/// READY, and LEAVE on `BACK_ROW`.
const STATUS_ROWS: [u8; 2] = [6, 7];
const PLAYER_ROWS: [u8; 2] = [9, 11];
const WATCHING_ROW: u8 = 13;
/// The series' lines: under the players, or two rows lower when the watching count is there.
const SERIES_ROWS: [u8; 2] = [14, 15];
const ROOM_SETTINGS_ROW: u8 = 19;
const READY_ROW: u8 = 21;
/// The ROOM SETTINGS page: the settings double-spaced from row 6, a line for those who can't
/// change them, and BACK on `BACK_ROW`.
const FIRST_SETTING_ROW: u8 = 6;
const HOST_ONLY_ROW: u8 = 24;

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
    Public,
}

const SETTINGS: [Setting; 9] = [
    Setting::Level,
    Setting::Pieces,
    Setting::DasDelay,
    Setting::DasRepeat,
    Setting::LeftRight,
    Setting::HardDrop,
    Setting::Ghost,
    Setting::NextPiece,
    Setting::Public,
];

impl Setting {
    fn label(self) -> &'static str {
        match self {
            Setting::Level => "LEVEL",
            Setting::Pieces => "SAME SEED",
            Setting::DasDelay => "DAS DELAY",
            Setting::DasRepeat => "DAS REPEAT",
            Setting::LeftRight => "LEFT/RIGHT",
            Setting::HardDrop => "HARD DROP",
            Setting::Ghost => "GHOST",
            Setting::NextPiece => "NEXT PIECE",
            Setting::Public => "PUBLIC",
        }
    }

    fn value(self, s: &RoomSettings) -> String {
        let on_off = |b: bool| if b { "ON" } else { "OFF" }.to_string();
        match self {
            Setting::Level => s.level.to_string(),
            Setting::Pieces => on_off(s.identical),
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
            Setting::Public => on_off(s.public),
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
            Setting::Public => s.public = !s.public,
        }
    }
}

/// The lobby's two pages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Main,
    RoomSettings,
}

/// What the lobby's cursor can be on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LobbyItem {
    /// The opponent's name, for the host to remove them.
    Opponent,
    Ready,
    RoomSettings,
    Leave,
    Setting(Setting),
    Back,
}

impl LobbyItem {
    fn row(self) -> u8 {
        match self {
            LobbyItem::Opponent => PLAYER_ROWS[1],
            LobbyItem::Ready => READY_ROW,
            LobbyItem::RoomSettings => ROOM_SETTINGS_ROW,
            LobbyItem::Setting(s) => FIRST_SETTING_ROW + 2 * SETTINGS.iter().position(|&x| x == s).unwrap() as u8,
            LobbyItem::Leave | LobbyItem::Back => BACK_ROW,
        }
    }

    fn label(self) -> &'static str {
        match self {
            LobbyItem::Opponent => "",
            LobbyItem::Ready => "READY",
            LobbyItem::RoomSettings => "ROOM SETTINGS",
            LobbyItem::Leave => "LEAVE",
            LobbyItem::Setting(s) => s.label(),
            LobbyItem::Back => "BACK",
        }
    }
}

/// `text` word-wrapped to the panel's width, centred, on `rows` (what doesn't fit is dropped).
fn wrapped(text: &str, rows: &[u8]) -> Vec<(u8, String)> {
    let mut lines: Vec<String> = vec![String::new()];
    for word in text.split_whitespace() {
        let last = lines.last_mut().unwrap();
        if !last.is_empty() && last.len() + 1 + word.len() > INNER.1 {
            lines.push(word.to_string());
        } else {
            if !last.is_empty() {
                last.push(' ');
            }
            last.push_str(word);
        }
    }
    rows.iter().zip(lines).map(|(&row, line)| (row, Game::centred(&line))).collect()
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

    /// A line of the panel, centred (half a tile right, if it has to be: `HALF`).
    fn centred(text: &str) -> String {
        let line = format!("{:^1$}", text, INNER.1);
        if (INNER.1 - text.chars().count().min(INNER.1)) % 2 == 1 {
            format!("{HALF}{line}")
        } else {
            line
        }
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
                self.ui.underline = None;
                if let Some(name) = editing_name.as_mut() {
                    let before = name.clone();
                    let done = self.edit_text(name, NAME_LEN, |c| crate::saved::NAME_CHARS.contains(c) && c != ',');
                    if *name != before {
                        self.queue_text(MENU[3].2, INNER.0, &Self::label_value("NAME", name));
                    }
                    if done {
                        let name = editing_name.take().unwrap().trim().to_string();
                        self.queue_text(MENU[3].2, INNER.0, &Self::label_value("NAME", &name));
                        self.request(VersusRequest::SetName(name));
                    } else {
                        let at = name.chars().count().min(NAME_LEN - 1);
                        self.ui.underline = Some((MENU[3].2, VALUE_COL + at as u8));
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
                            if let Some(code) = self.room_list(watch).await {
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

    /// JOIN or WATCH: the list of public rooms (JOIN, those with a seat free; WATCH, all, matches
    /// on first), then ENTER A CODE and BACK. The list comes from the app while this is up.
    /// Returns the room chosen, or none if backed out of.
    async fn room_list(&mut self, watch: bool) -> Option<String> {
        self.request(VersusRequest::ListRooms(true));
        let chosen = self.room_list_screen(watch).await;
        self.request(VersusRequest::ListRooms(false));
        chosen
    }

    /// The rooms JOIN or WATCH shows, from the app's list.
    fn listed(view: &VersusView, watch: bool) -> Vec<ListedRoom> {
        let RoomList::Rooms(rooms) = &view.rooms else { return vec![] };
        let mut rooms: Vec<ListedRoom> = rooms.iter().filter(|r| watch || r.state == ListedState::Open).cloned().collect();
        if watch {
            rooms.sort_by_key(|r| r.state != ListedState::Playing);
        }
        rooms
    }

    /// Every line inside the panel for the list (`top` is the first room shown), blank rows
    /// included.
    fn room_list_lines(view: &VersusView, watch: bool, rooms: &[ListedRoom], top: usize) -> Vec<(u8, String)> {
        let mut lines: Vec<(u8, String)> = (HEADER_ROW..=BACK_ROW).map(|row| (row, Self::label_value("", ""))).collect();
        let mut put = |row: u8, text: String| lines[(row - HEADER_ROW) as usize].1 = text;
        put(HEADER_ROW, Self::centred(if watch { "WATCH" } else { "JOIN" }));
        let message = match &view.rooms {
            RoomList::Closed | RoomList::Loading => Some("LOOKING FOR ROOMS"),
            RoomList::Unreachable => Some("CAN NOT REACH THE SERVER"),
            RoomList::Rooms(_) if rooms.is_empty() => Some(if watch { "NO ROOMS YET" } else { "NO OPEN ROOMS" }),
            RoomList::Rooms(_) => None,
        };
        if let Some(message) = message {
            put(LIST_ROWS[0], Self::centred(message));
        }
        for (room, row) in rooms.iter().skip(top).zip(LIST_ROWS) {
            let at = (LABEL_COL - INNER.0) as usize;
            put(row, format!("{:<1$}", format!("{:at$}{}", "", Self::room_text(room, watch)), INNER.1));
        }
        // Which are showing, when they don't all fit.
        if rooms.len() > LIST_ROWS.len() {
            let last = (top + LIST_ROWS.len()).min(rooms.len());
            put(SHOWING_ROW, Self::centred(&format!("ROOMS {}-{} OF {}", top + 1, last, rooms.len())));
        }
        put(CODE_ROW, Self::label_value("ENTER A CODE", ""));
        put(BACK_ROW, Self::label_value("BACK", ""));
        lines
    }

    /// A room's row: host, players, level, and in WATCH its state.
    fn room_text(room: &ListedRoom, watch: bool) -> String {
        let mut text = format!("{:<8} {}/2 LVL{:02}", room.host, room.players, room.level);
        if watch {
            text.push(' ');
            text.push_str(match room.state {
                ListedState::Open => "OPEN",
                ListedState::Full => "FULL",
                ListedState::Playing => "LIVE",
            });
        }
        text
    }

    async fn room_list_screen(&mut self, watch: bool) -> Option<String> {
        /// Where the cursor is: on a room (by code, as the list moves), ENTER A CODE, or BACK.
        #[derive(Clone, PartialEq)]
        enum At {
            Room(String),
            Code,
            Back,
        }
        let mut at = At::Back;
        let mut moved = false;
        let mut index = 0usize;
        let mut top = 0usize;
        'screen: loop {
            self.versus_panel().await;
            let view = self.view();
            let rooms = Self::listed(&view, watch);
            let mut shown = Self::room_list_lines(&view, watch, &rooms, top);
            for (row, text) in &shown {
                self.put_text(*row, INNER.0, text);
            }
            self.show_screen().await;
            self.memset_page(0x00, 0x02, 0x02);
            loop {
                self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
                let view = self.view();
                let rooms = Self::listed(&view, watch);
                // The cursor's place in rooms, ENTER A CODE, BACK; a room that's gone leaves it
                // where it was.
                let count = rooms.len() + 2;
                index = match &at {
                    At::Room(code) => rooms.iter().position(|r| &r.code == code).unwrap_or(index.min(count - 1)),
                    At::Code => rooms.len(),
                    At::Back => rooms.len() + 1,
                };
                // Until the player moves it, the cursor goes to the first room once there is one.
                if !moved && at == At::Back && !rooms.is_empty() {
                    index = 0;
                }
                let keys = self.menu_pressed();
                let has = |k| keys.contains(&k);
                if has(MenuKey::Up) || has(MenuKey::Down) {
                    moved = true;
                    index = (index + if has(MenuKey::Up) { count - 1 } else { 1 }) % count;
                    self.vars.sfx_init[1] = SFX_MENU_OPTION;
                }
                at = match index {
                    i if i < rooms.len() => At::Room(rooms[i].code.clone()),
                    i if i == rooms.len() => At::Code,
                    _ => At::Back,
                };
                // Keep the cursor's room in view.
                if index < rooms.len() {
                    top = top.min(index).max((index + 1).saturating_sub(LIST_ROWS.len()));
                }
                top = top.min(rooms.len().saturating_sub(LIST_ROWS.len()));
                let lines = Self::room_list_lines(&view, watch, &rooms, top);
                for (line, old) in lines.iter().zip(&shown) {
                    if line != old {
                        self.queue_text(line.0, INNER.0, &line.1);
                    }
                }
                shown = lines;
                if has(MenuKey::Back) || (has(MenuKey::Confirm) && at == At::Back) {
                    self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                    return None;
                }
                if has(MenuKey::Confirm) {
                    self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                    match &at {
                        At::Room(code) => return Some(code.clone()),
                        At::Code => {
                            if let Some(code) = self.code_entry(watch).await {
                                return Some(code);
                            }
                            continue 'screen;
                        }
                        At::Back => unreachable!(),
                    }
                }
                let (row, len) = match &at {
                    At::Room(_) => (LIST_ROWS[index - top], Self::room_text(&rooms[index], watch).len()),
                    At::Code => (CODE_ROW, "ENTER A CODE".len()),
                    At::Back => (BACK_ROW, "BACK".len()),
                };
                self.stage_menu_arrows(row, LABEL_COL, len);
            }
        }
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

    /// Every line inside the lobby's panel for `view` on `page`, blank rows included (so a page
    /// change redraws all of it).
    fn lobby_lines(view: &VersusView, page: Page, kick: Option<bool>) -> Vec<(u8, String)> {
        let mut lines: Vec<(u8, String)> = (HEADER_ROW..=BACK_ROW).map(|row| (row, Self::label_value("", ""))).collect();
        let mut put = |(row, text): (u8, String)| lines[(row - HEADER_ROW) as usize].1 = text;
        let in_room = view.connection == Connection::InRoom;
        match page {
            Page::Main => {
                let header = match &view.connection {
                    // The colon is drawn over the full stop (`draw_overlays`).
                    Connection::InRoom => format!("ROOM CODE. {}", view.code),
                    Connection::Connecting => "CONNECTING".to_string(),
                    Connection::None | Connection::Lost(_) => "NOT CONNECTED".to_string(),
                };
                put((HEADER_ROW, Self::centred(&header)));
                let status = match &view.connection {
                    Connection::Lost(why) => why.clone(),
                    _ => view.status.clone(),
                };
                wrapped(&status, &STATUS_ROWS).into_iter().for_each(&mut put);
                for (i, row) in PLAYER_ROWS.into_iter().enumerate() {
                    let text = match &view.players[i] {
                        // The host asking whether to remove the opponent.
                        Some((name, _)) if i == 1 && kick.is_some() => {
                            Self::label_value(name, if kick == Some(true) { "KICK? YES" } else { "KICK? NO" })
                        }
                        Some((name, ready)) => {
                            let you = if view.seat == Some(i as u8) { " (YOU)" } else { "" };
                            Self::label_value(&format!("{name}{you}"), if *ready { "READY" } else { "" })
                        }
                        None => Self::label_value("-", ""),
                    };
                    put((row, text));
                }
                if view.watching > 0 {
                    put((WATCHING_ROW, Self::label_value(&format!("{} WATCHING", view.watching), "")));
                }
                if in_room {
                    let below = if view.watching > 0 { 2 } else { 0 };
                    for (line, row) in view.series.iter().zip(SERIES_ROWS) {
                        put((row + below, Self::centred(line)));
                    }
                }
                if let Some(ready) = view.seat.and_then(|s| view.players[s as usize].as_ref()).map(|p| p.1) {
                    put((READY_ROW, Self::label_value("READY", if ready { "YES" } else { "NO" })));
                }
                if in_room {
                    put((ROOM_SETTINGS_ROW, Self::label_value("ROOM SETTINGS", "")));
                }
                put((BACK_ROW, Self::label_value("LEAVE", "")));
            }
            Page::RoomSettings => {
                put((HEADER_ROW, Self::centred("ROOM SETTINGS")));
                for s in SETTINGS {
                    put((LobbyItem::Setting(s).row(), Self::label_value(s.label(), &s.value(&view.settings))));
                }
                if view.seat != Some(0) {
                    put((HOST_ONLY_ROW, Self::centred("SET BY THE HOST")));
                }
                put((BACK_ROW, Self::label_value("BACK", "")));
            }
        }
        lines
    }

    /// What the cursor can reach on `page`: the host can change the settings; a watcher has no
    /// READY.
    fn lobby_items(view: &VersusView, page: Page) -> Vec<LobbyItem> {
        let mut items = vec![];
        match page {
            Page::Main => {
                if view.connection == Connection::InRoom {
                    if view.seat == Some(0) && view.players[1].is_some() {
                        items.push(LobbyItem::Opponent);
                    }
                    items.push(LobbyItem::RoomSettings);
                    if view.seat.is_some() {
                        items.push(LobbyItem::Ready);
                    }
                }
                items.push(LobbyItem::Leave);
            }
            Page::RoomSettings => {
                if view.seat == Some(0) {
                    items.extend(SETTINGS.map(LobbyItem::Setting));
                }
                items.push(LobbyItem::Back);
            }
        }
        items
    }

    /// The lobby, until LEAVE (or Escape).
    async fn lobby(&mut self) {
        self.versus_panel().await;
        let mut page = Page::Main;
        let mut shown = Self::lobby_lines(&self.view(), page, None);
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
        // Asking whether to remove the opponent: the answer chosen so far.
        let mut kick: Option<bool> = None;
        loop {
            self.update_audio_wait_for_nmi_and_reset_oam_staging().await;
            let mut view = self.view();
            if pending == Some(view.settings) || view.connection != Connection::InRoom {
                pending = None;
            }
            if let Some(p) = pending {
                view.settings = p;
            }
            // Out of the room: back to the main page.
            if page == Page::RoomSettings && view.connection != Connection::InRoom {
                page = Page::Main;
            }
            let items = Self::lobby_items(&view, page);
            if !items.contains(&LobbyItem::Opponent) {
                kick = None;
            }
            // Redraw what changed.
            let lines = Self::lobby_lines(&view, page, kick);
            for (line, old) in lines.iter().zip(&shown) {
                if line != old {
                    self.queue_text(line.0, INNER.0, &line.1);
                }
            }
            shown = lines;
            self.ui.colon = shown[0].1.trim_start_matches(HALF).find('.').map(|i| (HEADER_ROW, INNER.0 + i as u8));
            // The kick question's mark, drawn over the blank the font leaves (with the text).
            let row = PLAYER_ROWS[1];
            let question = shown.iter().find(|l| l.0 == row).and_then(|l| l.1.find('?')).map(|i| (row, INNER.0 + i as u8));
            if question != self.ui.question {
                self.ui.question_next = Some(question);
            }
            // Arriving in the room (seated): the cursor starts on READY.
            let has_ready = items.contains(&LobbyItem::Ready);
            if has_ready && !had_ready {
                at = LobbyItem::Ready;
            }
            if page == Page::Main {
                had_ready = has_ready;
            }
            if !items.contains(&at) {
                at = *items.iter().find(|i| **i == LobbyItem::Ready).unwrap_or(&items[0]);
            }
            let cursor = items.iter().position(|&i| i == at).unwrap();
            let keys = self.menu_pressed();
            let has = |k| keys.contains(&k);
            let back = has(MenuKey::Back) || (has(MenuKey::Confirm) && matches!(at, LobbyItem::Leave | LobbyItem::Back));
            // Up or Down while asking is a no, and moves the cursor as usual.
            if kick.is_some() && (has(MenuKey::Up) || has(MenuKey::Down)) {
                kick = None;
            }
            if let Some(yes) = kick {
                // The question takes the other keys until answered: Left/Right choose, Enter
                // answers, Escape is a no.
                if has(MenuKey::Left) || has(MenuKey::Right) {
                    kick = Some(!yes);
                    self.vars.sfx_init[1] = SFX_MENU_OPTION;
                } else if has(MenuKey::Confirm) {
                    if yes {
                        self.request(VersusRequest::Kick);
                    }
                    kick = None;
                    self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                } else if !keys.is_empty() {
                    kick = None;
                    self.vars.sfx_init[1] = SFX_MENU_OPTION;
                }
            } else if back && page == Page::RoomSettings {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                page = Page::Main;
                at = LobbyItem::RoomSettings;
            } else if back {
                self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                self.ui.colon = None;
                self.ui.question = None;
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
                    LobbyItem::Opponent if has(MenuKey::Confirm) => {
                        kick = Some(false);
                        self.vars.sfx_init[1] = SFX_MENU_OPTION;
                    }
                    LobbyItem::RoomSettings if has(MenuKey::Confirm) => {
                        page = Page::RoomSettings;
                        at = Self::lobby_items(&view, page)[0];
                        self.vars.sfx_init[1] = SFX_MENU_SCREEN;
                    }
                    _ => {}
                }
            }
            let len = match at {
                LobbyItem::Opponent => view.players[1].as_ref().map_or(0, |p| p.0.len()),
                _ => at.label().len(),
            };
            self.stage_menu_arrows(at.row(), LABEL_COL, len);
        }
    }
}
