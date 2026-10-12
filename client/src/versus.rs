//! VERSUS (the `versus` feature): the connection to a room, the lobby's view, and the match.
//!
//! The VERSUS screens are the game's (`tetris_rs::versus`): each frame they get a `VersusView`
//! from here and hand back what the player chose. A match is a `netplay::Session` over a
//! `netplay::Link`; while one is on (the countdown, play, and the result for a few seconds) the
//! game isn't run at all, and the window shows both boards side by side instead, with the
//! countdown, the result and Escape's prompt drawn over them.

use std::sync::Arc;

use netplay::{Link, LinkEvent, LocalRoom, Note, Phase, RoomList as ListLink, Session, HEIGHT, WIDTH};
use protocol::{Outcome, Pieces, Settings};
use tetris_rs::options::LeftRight;
use tetris_rs::pc_input::play_buttons;
use tetris_rs::versus::{draw_glyphs, draw_text, Connection, ListedRoom, ListedState, RoomList, RoomSettings, VersusRequest, VersusView, BLACK, WHITE};
use tetris_rs::{Config, Rom};

use crate::backdrop::{self, Backdrop};
use crate::settings;

/// The match picture, in tiles, after Tetris Effect's classic versus: a margin, each board, with
/// a gap between them. A board is the game screen's columns 11..=30 (the playfield with LINES
/// above it, then the panel: TOP and SCORE, NEXT, LEVEL; not the statistics or A-TYPE), with the
/// score box a tile wider, to hold the difference's sign, and two shorter, for its three lines.
/// The background is laid afresh across all of it (`backdrop`).
const MARGIN: usize = 3;
const GAP: usize = 5;
const BOARD_COLS: usize = 21;
const MATCH_COLS: usize = 2 * MARGIN + 2 * BOARD_COLS + GAP;
pub const MATCH_WIDTH: usize = 8 * MATCH_COLS;
/// Above the boards, a row of background, a box for each player (their name and the matches
/// they've won here), and a row; below them, a row. The game screen's rows 2..=26 (the boards'
/// boxes) land on rows 6..=30.
const TOP_ROWS: usize = 6;
const MATCH_ROWS: usize = 32;
pub const MATCH_HEIGHT: usize = 8 * MATCH_ROWS;
/// The players' boxes: rows, and columns (wide enough for an 8-letter name and a margin each
/// side), centred in each half of the picture.
const NAME_BOX_ROWS: (usize, usize) = (1, 4);
const NAME_BOX_COLS: usize = 12;

/// The first column of the `i`th player's box.
fn name_box_col(i: usize) -> usize {
    let half = MATCH_COLS as f64 / 2.0;
    (half * i as f64 + half / 2.0 - NAME_BOX_COLS as f64 / 2.0).round() as usize
}

/// The picture row where a board's game-screen row `r` goes.
fn board_row(r: usize) -> usize {
    r + TOP_ROWS - 2
}
/// The game screen's first column in a board.
const FIRST_COL: usize = 11;
/// The score box: its rows and columns on the game screen, the column repeated to widen it, and
/// the row its bottom edge moves up to.
const SCORE_ROWS: (usize, usize) = (2, 10);
const SCORE_COLS: (usize, usize) = (23, 30);
const WIDENED_AT: usize = 26;
const SCORE_BOTTOM: usize = 8;

/// The picture's column where the `i`th board starts.
fn board_col(i: usize) -> usize {
    MARGIN + i * (BOARD_COLS + GAP)
}

/// The game screen's cell (row, column) shown in a board's `col`th column on row `row`: its own,
/// except for the score box, which is a column wider (one of its inside columns shows twice and
/// the rest move right) and ends at `SCORE_BOTTOM` (its bottom edge moved up, the background
/// under it); none past the panel.
fn source_cell(row: usize, col: usize) -> Option<(usize, usize)> {
    let c = col + FIRST_COL;
    if c < SCORE_COLS.0 || !(SCORE_ROWS.0..=SCORE_ROWS.1).contains(&row) {
        return (c <= SCORE_COLS.1).then_some((row, c));
    }
    let c = if c > WIDENED_AT { c - 1 } else { c };
    match row {
        r if r < SCORE_BOTTOM => Some((r, c)),
        SCORE_BOTTOM => Some((SCORE_ROWS.1, c)),
        _ => None,
    }
}

/// The score box's inside in a board (pixels from the board's left, and rows), and its lines:
/// SCORE, the score, the difference, 8 pixels apart and centred top to bottom (each 7 high). The
/// sign takes 5 pixels before the difference's digits; the rest line up with them.
const SCORE_INSIDE: (usize, usize, usize, usize) = (
    8 * (SCORE_COLS.0 - FIRST_COL) + 5,
    8 * (SCORE_COLS.1 + 1 - FIRST_COL) + 8 - 5,
    8 * SCORE_ROWS.0 + 5,
    8 * SCORE_BOTTOM + 3,
);
const SIGN_X: usize = SCORE_INSIDE.0 + 4;
const DIGITS_X: usize = SIGN_X + 6;
const SCORE_TOP: usize = SCORE_INSIDE.2 + (SCORE_INSIDE.3 - SCORE_INSIDE.2 - 23) / 2;
const SCORE_LINES: [usize; 3] = [SCORE_TOP, SCORE_TOP + 8, SCORE_TOP + 16];
/// The difference's colours: green ahead (or level), red behind; the result's: WIN yellow, LOSE
/// grey.
const GREEN: u8 = 0x2A;
const RED: u8 = 0x16;
const YELLOW: u8 = 0x28;
const GREY: u8 = 0x10;
/// The sign before the difference, narrower than a letter (5 by 7, left pixel in bit 4): the font
/// has no plus.
const PLUS: [u8; 7] = [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000];
const MINUS: [u8; 7] = [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000];
/// Escape's prompt: under the boards.
const PROMPT_Y: usize = MATCH_HEIGHT - 12;
/// How long the result stays over the boards (unless Enter is pressed first).
const RESULT_MS: u64 = 6000;
/// The middle of the playfield on the game screen, in pixels from its left.
const PLAYFIELD_CENTRE: usize = 136;

/// How long Escape's "again to quit" lasts.
const ESCAPE_MS: u64 = 2000;

pub struct Versus {
    rom: Arc<Rom>,
    sample_rate: Option<u32>,
    /// Milliseconds on the app's clock, as of this frame.
    pub now: u64,
    /// For room codes.
    rng: u64,
    /// The room server and the name, for this run (the saved ones unless overridden).
    pub server: String,
    pub name: String,
    /// The music the player's game plays in a match (the game's music choice: 0..=2, 3 off).
    pub music: u8,
    room: Option<Room>,
    /// A connection that failed or ended, until the player leaves the lobby.
    lost: Option<String>,
    /// The list of public rooms, while JOIN or WATCH is up, and the latest of it.
    list: Option<ListLink>,
    rooms: RoomList,
    /// The game screen's box cells, and the match picture's background.
    boxes: [[bool; 32]; 30],
    backdrop: Backdrop,
    /// The game screen with nothing in it, for a board whose game hasn't begun.
    empty: Vec<u8>,
}

struct Room {
    link: Conn,
    /// OFFLINE: a room run here, with no server; a bot is seated once we are.
    offline: bool,
    bot_added: bool,
    session: Session,
    code: String,
    /// Our room settings have been sent (once seated as host).
    sent_settings: bool,
    /// The result stays over the boards until this time.
    result_until: Option<u64>,
    /// Escape was pressed during the match at this time.
    escape_at: Option<u64>,
    /// Paused, against our own bot: the pause menu's cursor (0 RESUME, 1 QUIT).
    pause: Option<usize>,
    /// The last thing worth saying in the lobby's status line.
    note: Option<String>,
    /// The matches between the room's two players so far.
    series: Option<Series>,
    /// The current result has been counted in `series`.
    counted: bool,
}

/// The way to a room: a server's, or one run here (OFFLINE).
enum Conn {
    Server(Link),
    Here(Box<LocalRoom>),
}

impl Conn {
    fn send(&mut self, msg: protocol::ToRoom) {
        match self {
            Conn::Server(link) => link.send(msg),
            Conn::Here(room) => room.send(msg),
        }
    }

    fn poll(&mut self) -> Option<LinkEvent> {
        match self {
            Conn::Server(link) => link.poll(),
            Conn::Here(room) => room.poll(),
        }
    }
}

/// Matches won by each of two players (by seat in their matches), since they first met here.
struct Series {
    /// The two (`Player::id`, which tells them apart), and their names to show.
    ids: [Option<u32>; 2],
    names: [String; 2],
    wins: [u32; 2],
    /// The players' reports of the last result disagreed.
    disputed: bool,
}

/// What a frame of VERSUS needs from the app.
pub struct Frame<'a> {
    pub held: &'a [String],
    pub pressed: &'a [String],
    pub config: &'a Config,
}

impl Versus {
    /// `seed` seeds the room codes this makes up.
    pub fn new(rom: Arc<Rom>, sample_rate: Option<u32>, server: String, name: String, seed: u64) -> Self {
        let boxes = backdrop::game_boxes(&rom);
        let backdrop = Backdrop::new(MATCH_COLS, MATCH_ROWS, |pr, c| {
            let name_box = (0..2).any(|i| (NAME_BOX_ROWS.0..=NAME_BOX_ROWS.1).contains(&pr) && (name_box_col(i)..name_box_col(i) + NAME_BOX_COLS).contains(&c));
            let board = pr >= board_row(0)
                && (0..2).any(|i| {
                    let r = pr - board_row(0);
                    (board_col(i)..board_col(i) + BOARD_COLS).contains(&c) && source_cell(r, c - board_col(i)).is_some_and(|(sr, sc)| boxes[sr][sc])
                });
            name_box || board
        });
        let empty = backdrop::empty_game_screen(&rom);
        Versus { rom, sample_rate, now: 0, rng: seed | 1, server, name, music: 0, room: None, lost: None, list: None, rooms: RoomList::Closed, boxes, backdrop, empty }
    }

    fn now(&self) -> u64 {
        self.now
    }


    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// Whether connected to a room (or trying to be).
    pub fn in_room(&self) -> bool {
        self.room.is_some()
    }

    /// Whether a match has the window: the countdown, play, or the result being shown.
    pub fn in_match(&self) -> bool {
        let Some(room) = &self.room else { return false };
        match room.session.phase() {
            Phase::Countdown { .. } | Phase::Playing => true,
            // A result not yet shown, or still being shown.
            Phase::Over { .. } => room.result_until.is_none_or(|t| self.now() < t),
            _ => false,
        }
    }

    /// Take what the room said since the last frame.
    pub fn poll(&mut self) {
        let now = self.now();
        if let Some(list) = self.list.as_mut() {
            self.rooms = match list.latest() {
                None => RoomList::Loading,
                Some(Err(why)) => {
                    if self.rooms != RoomList::Unreachable {
                        eprintln!("versus: {why}");
                    }
                    RoomList::Unreachable
                }
                Some(Ok(rooms)) => RoomList::Rooms(rooms.iter().map(listed).collect()),
            };
        }
        let Some(room) = self.room.as_mut() else { return };
        while let Some(event) = room.link.poll() {
            match event {
                // Removed by the host (the connection closes next).
                LinkEvent::Message(protocol::FromRoom::Kicked) => {
                    self.lost = Some("THE HOST REMOVED YOU".into());
                    self.room = None;
                    return;
                }
                LinkEvent::Message(msg) => room.session.receive(now, msg),
                LinkEvent::Closed(why) => {
                    eprintln!("versus: {why}");
                    let reached = !matches!(room.session.phase(), Phase::Connecting);
                    self.lost = Some(if reached { "LOST THE CONNECTION" } else { "CAN NOT REACH SERVER" }.into());
                    self.room = None;
                    return;
                }
            }
        }
        // A new host (the first left) sends their settings too.
        if room.session.seat() != Some(0) {
            room.sent_settings = false;
        }
    }

    /// The lobby's view of things.
    pub fn view(&self, saved: &settings::Versus) -> VersusView {
        let room_settings = saved.room.to_game();
        let Some(room) = &self.room else {
            return VersusView {
                name: self.name.clone(),
                connection: match &self.lost {
                    Some(why) => Connection::Lost(why.clone()),
                    None => Connection::None,
                },
                settings: room_settings,
                rooms: self.rooms.clone(),
                ..Default::default()
            };
        };
        let s = &room.session;
        let connection = if *s.phase() == Phase::Connecting { Connection::Connecting } else { Connection::InRoom };
        // A bot's name says so ("(CPU)"), as "(YOU)" does.
        let players = s.players().clone().map(|p| p.map(|p| (if p.bot { format!("{} (CPU)", p.name) } else { p.name }, p.ready)));
        let status = match (s.phase(), &room.note) {
            (Phase::Connecting, _) => "CONNECTING TO THE ROOM".to_string(),
            (_, Some(note)) => note.clone(),
            _ if players.iter().any(Option::is_none) => "WAITING FOR A PLAYER".to_string(),
            _ if s.seat().is_none() => "WATCHING".to_string(),
            (Phase::Over { .. }, _) => "READY FOR A REMATCH".to_string(),
            _ => "WAITING FOR READY".to_string(),
        };
        // The score between the two (or who of them left, while their seat's empty).
        let mut series = vec![];
        if let Some(sr) = &room.series {
            let here = |id: &Option<u32>| s.players().iter().flatten().any(|p| Some(p.id) == *id);
            match (0..2).find(|&i| !here(&sr.ids[i])).map(|i| &sr.names[i]) {
                Some(gone) if players.iter().any(Option::is_none) => series.push(format!("{gone} LEFT")),
                Some(_) => {}
                None => {
                    series.push(format!("{} {} - {} {}", sr.names[0], sr.wins[0], sr.wins[1], sr.names[1]));
                    if sr.disputed {
                        series.push("LAST RESULT DISPUTED".to_string());
                    }
                }
            }
        }
        VersusView {
            name: self.name.clone(),
            connection,
            code: room.code.clone(),
            offline: room.offline,
            seat: s.seat(),
            players,
            watching: s.spectators(),
            settings: from_protocol(&s.settings()),
            status,
            series,
            rooms: self.rooms.clone(),
        }
    }

    /// Act on what the player chose in the lobby. Returns true if the saved settings changed.
    pub fn request(&mut self, r: VersusRequest, saved: &mut settings::Versus) -> bool {
        match r {
            VersusRequest::Host => {
                let code = protocol::room_code(self.random());
                self.join(code, false);
            }
            VersusRequest::Join(code) => self.join(code, false),
            VersusRequest::Watch(code) => self.join(code, true),
            VersusRequest::Ready(ready) => {
                if let Some(room) = self.room.as_mut() {
                    room.session.set_ready(ready);
                    room.note = None;
                }
            }
            VersusRequest::Settings(s) => {
                saved.room = settings::Room::from_game(&s);
                if let Some(room) = self.room.as_mut() {
                    room.session.set_settings(to_protocol(&s));
                }
                return true;
            }
            VersusRequest::Leave => {
                self.room = None;
                self.lost = None;
            }
            VersusRequest::VsBot => self.play_bot(),
            VersusRequest::AddBot => {
                let pick = (self.random() % bots::ROSTER.len() as u64) as usize;
                if let Some(room) = self.room.as_mut() {
                    room.session.add_bot(bots::ROSTER[pick].0);
                }
            }
            VersusRequest::Kick => {
                if let Some(room) = self.room.as_mut() {
                    room.session.kick();
                }
            }
            VersusRequest::ListRooms(open) => {
                self.list = open.then(|| ListLink::open(&self.server));
                self.rooms = if open { RoomList::Loading } else { RoomList::Closed };
            }
            VersusRequest::SetName(name) => {
                let name = protocol::clean_name(&name);
                self.name = name.clone();
                saved.name = name;
                return true;
            }
        }
        false
    }

    fn join(&mut self, code: String, watch: bool) {
        let url = format!("{}/room/{code}", self.server.trim_end_matches('/'));
        self.enter(Conn::Server(Link::open(&url)), code, watch);
    }

    /// OFFLINE: a room run here, which we host, with a bot seated as soon as we are.
    fn play_bot(&mut self) {
        let seed = self.random();
        self.enter(Conn::Here(Box::new(LocalRoom::new(seed))), String::new(), false);
    }

    fn enter(&mut self, link: Conn, code: String, watch: bool) {
        let session = Session::new(self.rom.clone(), self.sample_rate, &self.name, watch);
        self.lost = None;
        let offline = matches!(link, Conn::Here(_));
        self.room = Some(Room {
            link,
            offline,
            bot_added: false,
            session,
            code,
            sent_settings: false,
            result_until: None,
            escape_at: None,
            pause: None,
            note: None,
            series: None,
            counted: false,
        });
    }

    /// Send what the session has for the room, the host's settings once seated. Between matches
    /// the session still ticks, so that a result's report goes out even if the result was
    /// dismissed before both games reached it.
    pub fn flush(&mut self, saved: &settings::Versus) {
        let now = self.now();
        let in_match = self.in_match();
        let Some(room) = self.room.as_mut() else { return };
        room.session.set_music(self.music);
        if !in_match {
            room.session.tick(now, 0);
        }
        match room.session.phase() {
            Phase::Over { outcome, confirmed, .. } => {
                let names = room.session.match_names().clone().map(|n| n.unwrap_or_else(|| "-".to_string()));
                if !room.counted {
                    room.counted = true;
                    let ids = room.session.match_ids();
                    let series = room.series.get_or_insert_with(|| Series { ids, names: names.clone(), wins: [0; 2], disputed: false });
                    if series.ids != ids {
                        *series = Series { ids, names, wins: [0; 2], disputed: false };
                    }
                    match outcome {
                        Outcome::Win { seat } => series.wins[*seat as usize] += 1,
                        Outcome::Forfeit { seat } => series.wins[1 - *seat as usize] += 1,
                        Outcome::Draw => {}
                    }
                }
                if let Some(series) = room.series.as_mut() {
                    series.disputed = *confirmed == Some(false);
                }
            }
            Phase::Countdown { .. } | Phase::Playing => room.counted = false,
            _ => {}
        }
        // OFFLINE: once seated, a bot (once; kicked, ADD CPU brings another).
        if room.offline && !room.bot_added && room.session.seat() == Some(0) && *room.session.phase() == Phase::Lobby {
            room.bot_added = true;
            let pick = {
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 7;
                self.rng ^= self.rng << 17;
                (self.rng % bots::ROSTER.len() as u64) as usize
            };
            room.session.add_bot(bots::ROSTER[pick].0);
        }
        if room.session.seat() == Some(0) && !room.sent_settings && *room.session.phase() == Phase::Lobby {
            room.session.set_settings(to_protocol(&saved.room.to_game()));
            room.sent_settings = true;
        }
        for note in room.session.take_notes() {
            room.note = Some(match note {
                Note::Error(e) => e.to_uppercase(),
                // The empty seat says so; being removed is taken as it arrives (`poll`).
                Note::Gone(_) | Note::Kicked => continue,
                Note::Desync { .. } => "THE GAMES DISAGREE".to_string(),
                Note::Abandoned => "BOTH PLAYERS LEFT".to_string(),
            });
        }
        for msg in room.session.take_outbox() {
            room.link.send(msg);
        }
    }

    /// A frame of the match: our game takes the keys (watching: nobody's), and both boards are
    /// drawn side by side with what goes over them. Returns the picture and our game's sound.
    pub fn match_frame(&mut self, f: Frame) -> (Vec<u8>, Vec<f32>) {
        let now = self.now();
        let rom = self.rom.clone();
        let room = self.room.as_mut().expect("a match");
        let pressed = |k: &str| f.pressed.iter().any(|p| p == k);
        let any = |ks: &[&str]| ks.iter().any(|k| pressed(k));
        // Against our own bot, the pause key (or Escape) pauses, with the game's pause menu:
        // RESUME, or QUIT, which resigns (we stay in the room).
        let own_bot = room.session.playing_bot();
        if own_bot && *room.session.phase() == Phase::Playing {
            let pause_key = any(&["Escape", "PadStart", f.config.keys[6].as_str()]);
            match room.pause {
                None if pause_key => {
                    room.pause = Some(0);
                    room.session.set_paused(true);
                }
                None => {}
                Some(at) => {
                    if any(&["ArrowUp", "ArrowDown", "PadUp", "PadDown"]) {
                        room.pause = Some(1 - at);
                    } else if any(&["Enter", "NumpadEnter", "PadSouth"]) || pause_key {
                        room.pause = None;
                        room.session.set_paused(false);
                        if at == 1 && !pause_key {
                            room.session.resign();
                        }
                    }
                }
            }
        } else {
            room.pause = None;
        }
        // Against people, Escape twice leaves the room (a forfeit, if our game's still on).
        if !own_bot && pressed("Escape") {
            if room.escape_at.is_some_and(|t| now < t + ESCAPE_MS) {
                room.escape_at = None;
                self.room = None;
                self.lost = Some("YOU LEFT THE MATCH".into());
                return (vec![BLACK; WIDTH * HEIGHT], vec![]);
            } else {
                room.escape_at = Some(now);
            }
        }
        let settings = room.session.settings();
        let held: Vec<&str> = f.held.iter().map(String::as_str).collect();
        // Start would pause; there's no pausing in a race.
        let buttons = play_buttons(&f.config.keys, settings.rules.hard_drop, &held) & !tetris_rs::buttons::START;
        let mut view = room.session.tick(now, buttons);
        if matches!(room.session.phase(), Phase::Countdown { .. } | Phase::Playing) {
            room.result_until = None;
        }
        if let Phase::Over { .. } = room.session.phase() {
            let until = *room.result_until.get_or_insert(now + RESULT_MS);
            if pressed("Enter") || pressed("PadSouth") {
                room.result_until = Some(now.min(until));
            }
        }
        // Ours (or the host's, watching) on the left.
        let left = room.session.match_seat().unwrap_or(0) as usize;
        let width = MATCH_WIDTH;
        let mut pixels = vec![BLACK; width * MATCH_HEIGHT];
        self.backdrop.draw(&mut pixels);
        // The boards' rows start this many pixels down (their game screen's y + oy).
        let oy = 8 * board_row(0);
        // Where a game screen x lands for the `i`th board, and its playfield's middle.
        let sx = |i: usize, x: usize| 8 * board_col(i) + x - 8 * FIRST_COL;
        let centres = [sx(0, PLAYFIELD_CENTRE), sx(1, PLAYFIELD_CENTRE)];
        let names = room.session.match_names().clone().map(Option::unwrap_or_default);
        let mut scores = room.session.scores().map(Option::unwrap_or_default);
        // The result's scores once it's in (a forfeit's are only as far as the room heard, so
        // the games' own stand).
        if let Phase::Over { scores: official, outcome, .. } = room.session.phase() {
            if !matches!(outcome, Outcome::Forfeit { .. }) {
                scores = *official;
            }
        }
        for (i, seat) in [left, 1 - left].into_iter().enumerate() {
            let middle = centres[i];
            // Until the game's picture comes (at the end of the countdown), the empty boxes.
            let board = view.boards[seat].as_deref().map_or(&self.empty[..], |b| &b[..]);
            // The game's boxes, a tile at a time; the background between them is the backdrop's.
            for r in 0..30 {
                for c in 0..BOARD_COLS {
                    let Some((sr, sc)) = source_cell(r, c).filter(|&(sr, sc)| self.boxes[sr][sc]) else { continue };
                    for y in 0..8 {
                        pixels[(board_row(r) * 8 + y) * width + 8 * (board_col(i) + c)..][..8].copy_from_slice(&board[(sr * 8 + y) * WIDTH + sc * 8..][..8]);
                    }
                }
            }
            // The score box: SCORE, the score, and the difference from the opponent's.
            let bx = |x: usize| 8 * board_col(i) + x;
            let (x0, x1, y0, y1) = SCORE_INSIDE;
            for y in y0 + oy..y1 + oy {
                pixels[y * width + bx(x0)..y * width + bx(x1)].fill(BLACK);
            }
            let diff = scores[seat] as i64 - scores[1 - seat] as i64;
            let colour = if diff >= 0 { GREEN } else { RED };
            // The letters alone: a box round them would cover the frame.
            draw_glyphs(&rom, &mut pixels, width, bx(DIGITS_X), oy + SCORE_LINES[0], "SCORE", WHITE);
            if view.boards[seat].is_none() {
                continue;
            }
            draw_glyphs(&rom, &mut pixels, width, bx(DIGITS_X), oy + SCORE_LINES[1], &format!("{:06}", scores[seat].min(999_999)), WHITE);
            draw_glyphs(&rom, &mut pixels, width, bx(DIGITS_X), oy + SCORE_LINES[2], &format!("{:06}", diff.unsigned_abs().min(999_999)), colour);
            let sign = if diff >= 0 { PLUS } else { MINUS };
            for (dy, bits) in sign.iter().enumerate() {
                for dx in 0..5 {
                    if bits >> (4 - dx) & 1 != 0 {
                        pixels[(oy + SCORE_LINES[2] + dy) * width + bx(SIGN_X) + dx] = colour;
                    }
                }
            }
            // A player who left: their board stops, marked with who left.
            if room.session.gone()[seat] {
                for (k, line) in [names[seat].as_str(), "LEFT"].into_iter().enumerate() {
                    draw_text(&rom, &mut pixels, width, middle - 4 * line.len(), oy + 84 + 12 * k, line, WHITE);
                }
            }
        }
        // Above, each player's box, centred in their half: their name, and the matches they've
        // won here.
        let wins = room.series.as_ref().filter(|s| s.ids == room.session.match_ids()).map_or([0, 0], |s| s.wins);
        for (i, seat) in [left, 1 - left].into_iter().enumerate() {
            let col = name_box_col(i);
            draw_box(&self.empty, &mut pixels, width, col, NAME_BOX_ROWS.0, NAME_BOX_COLS, NAME_BOX_ROWS.1 + 1 - NAME_BOX_ROWS.0);
            let middle = 8 * col + 4 * NAME_BOX_COLS;
            let top = 8 * (NAME_BOX_ROWS.0 + 1);
            let won = wins[seat].to_string();
            draw_glyphs(&rom, &mut pixels, width, middle - 4 * names[seat].len(), top, &names[seat], WHITE);
            draw_glyphs(&rom, &mut pixels, width, middle - 4 * won.len(), top + 8, &won, WHITE);
        }
        match room.session.phase() {
            Phase::Countdown { at } => {
                let n = at.saturating_sub(now).div_ceil(1000).to_string();
                for middle in centres {
                    draw_text(&rom, &mut pixels, width, middle - 4 * n.len(), oy + 112, &n, WHITE);
                }
            }
            Phase::Over { outcome, confirmed, .. } => {
                // At the top of each playfield: WIN or LOSE (or DRAW).
                for (seat, middle) in [(left, centres[0]), (1 - left, centres[1])] {
                    let (text, colour) = verdict(outcome, seat);
                    draw_text(&rom, &mut pixels, width, middle - 4 * text.len(), oy + 56, text, colour);
                    if *confirmed == Some(false) {
                        draw_text(&rom, &mut pixels, width, middle - 4 * 8, oy + 68, "DISPUTED", WHITE);
                    }
                }
            }
            // Decided while the winner plays on: nothing yet; WIN and LOSE come together at the end.
            _ => {}
        }
        // Paused: both playfields blank (as the game hides its own), and the game's pause screen
        // centred in the picture; watching, PAUSED only.
        if room.session.paused() {
            for i in 0..2 {
                for y in oy + 48..oy + 208 {
                    pixels[y * width + sx(i, 96)..y * width + sx(i, 176)].fill(BLACK);
                }
            }
            let middle = width / 2;
            draw_text(&rom, &mut pixels, width, middle - 4 * 6, oy + 104, "PAUSED", WHITE);
            if let Some(at) = room.pause {
                let items = ["RESUME", "QUIT"];
                for (k, item) in items.iter().enumerate() {
                    draw_text(&rom, &mut pixels, width, middle - 4 * item.len(), oy + 128 + 12 * k, item, WHITE);
                }
                // The menus' flickering arrows (gone one frame in four).
                if !(now * 60 / 1000).is_multiple_of(4) {
                    let (item, y) = (items[at], oy + 128 + 12 * at);
                    let (x0, x1) = (middle - 4 * item.len(), middle + 4 * item.len());
                    draw_arrow(&rom, &mut pixels, width, x0 - 10, y, false);
                    draw_arrow(&rom, &mut pixels, width, x1 + 2, y, true);
                }
            }
        }
        if room.escape_at.is_some_and(|t| now < t + ESCAPE_MS) {
            let text = "ESC AGAIN TO QUIT";
            draw_text(&rom, &mut pixels, width, width / 2 - 4 * text.len(), PROMPT_Y, text, WHITE);
        }
        (pixels, std::mem::take(&mut view.samples))
    }
}

/// Whether the last match's result has been confirmed by the room.
#[cfg(test)]
impl Versus {
    pub fn playing(&self) -> bool {
        self.room.as_ref().is_some_and(|r| *r.session.phase() == Phase::Playing)
    }

    pub fn scores(&self) -> [Option<u32>; 2] {
        self.room.as_ref().map_or([None, None], |r| r.session.scores())
    }

    pub fn decided(&self) -> bool {
        self.room.as_ref().is_some_and(|r| r.session.decided().is_some())
    }

    pub fn confirmed(&self) -> bool {
        self.room.as_ref().is_some_and(|r| matches!(r.session.phase(), Phase::Over { confirmed: Some(true), .. }))
    }
}


fn listed(r: &protocol::RoomSummary) -> ListedRoom {
    ListedRoom {
        code: r.code.clone(),
        host: r.host.clone(),
        players: r.players,
        level: r.level,
        state: match r.state {
            protocol::RoomState::Open => ListedState::Open,
            protocol::RoomState::Full => ListedState::Full,
            protocol::RoomState::Playing => ListedState::Playing,
        },
    }
}

/// The menus' arrow (the menu font's tile $27, pointing right; `flip` points it left), in white on
/// black.
fn draw_arrow(rom: &Rom, pixels: &mut [u8], width: usize, x: usize, y: usize, flip: bool) {
    const ARROW: usize = 0x27;
    // On black, as the text is.
    for py in y.saturating_sub(1)..y + 9 {
        pixels[py * width + x - 1..][..10].fill(BLACK);
    }
    for row in 0..8 {
        let (lo, hi) = (rom.chr[ARROW * 16 + row], rom.chr[ARROW * 16 + row + 8]);
        for col in 0..8 {
            let bit = if flip { col } else { 7 - col };
            if ((lo | hi) >> bit) & 1 != 0 {
                pixels[(y + row) * width + x + col] = WHITE;
            }
        }
    }
}

/// What `outcome` is for `seat`: WIN, LOSE or DRAW, and its colour.
fn verdict(outcome: &Outcome, seat: usize) -> (&'static str, u8) {
    let winner = match outcome {
        Outcome::Win { seat } => Some(*seat as usize),
        Outcome::Forfeit { seat } => Some(1 - *seat as usize),
        Outcome::Draw => None,
    };
    match winner {
        None => ("DRAW", WHITE),
        Some(w) if w == seat => ("WIN", YELLOW),
        Some(_) => ("LOSE", GREY),
    }
}

/// A box like the game's LEVEL box, `cols` by `rows` tiles from (`col`, `row`): its frame's
/// tiles taken from the game screen (`screen`, an empty game screen: the LEVEL box is rows 19..=22,
/// columns 23..=29), black inside.
fn draw_box(screen: &[u8], pixels: &mut [u8], width: usize, col: usize, row: usize, cols: usize, rows: usize) {
    const LEVEL_BOX: (usize, usize, usize, usize) = (19, 22, 23, 29);
    let (top, bottom, left, right) = LEVEL_BOX;
    for r in 0..rows {
        for c in 0..cols {
            let sr = if r == 0 { top } else if r == rows - 1 { bottom } else { top + 2 };
            let sc = if c == 0 { left } else if c == cols - 1 { right } else { left + 1 };
            for y in 0..8 {
                pixels[((row + r) * 8 + y) * width + (col + c) * 8..][..8].copy_from_slice(&screen[(sr * 8 + y) * WIDTH + sc * 8..][..8]);
            }
        }
    }
}

fn to_protocol(s: &RoomSettings) -> Settings {
    Settings {
        level: s.level,
        pieces: if s.identical { Pieces::Identical } else { Pieces::Separate },
        rules: protocol::Rules {
            das_delay: s.das_delay,
            das_repeat: s.das_repeat,
            left_right: match s.left_right {
                LeftRight::Both => protocol::LeftRight::Both,
                LeftRight::Last => protocol::LeftRight::Last,
                LeftRight::Neither => protocol::LeftRight::Neither,
            },
            hard_drop: s.hard_drop,
            ghost: s.ghost,
            next_piece: s.next_piece,
        },
        public: s.public,
    }
}

fn from_protocol(s: &Settings) -> RoomSettings {
    RoomSettings {
        level: s.level,
        identical: s.pieces == Pieces::Identical,
        das_delay: s.rules.das_delay,
        das_repeat: s.rules.das_repeat,
        left_right: match s.rules.left_right {
            protocol::LeftRight::Both => LeftRight::Both,
            protocol::LeftRight::Last => LeftRight::Last,
            protocol::LeftRight::Neither => LeftRight::Neither,
        },
        hard_drop: s.rules.hard_drop,
        ghost: s.rules.ghost,
        next_piece: s.rules.next_piece,
        public: s.public,
    }
}
