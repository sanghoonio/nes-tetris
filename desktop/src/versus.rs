//! VERSUS (the `versus` feature): the connection to a room, the lobby's view, and the match.
//!
//! The VERSUS screens are the game's (`tetris_rs::versus`): each frame they get a `VersusView`
//! from here and hand back what the player chose. A match is a `netplay::Session` over a
//! `netplay::Link`; while one is on (the countdown, play, and the result for a few seconds) the
//! game isn't run at all, and the window shows both boards side by side instead, with the
//! countdown, the result and Escape's prompt drawn over them.

use std::sync::Arc;
use std::time::Instant;

use netplay::{Link, LinkEvent, Note, Phase, Session, HEIGHT, WIDTH};
use protocol::{Outcome, Pieces, Settings};
use tetris_rs::options::LeftRight;
use tetris_rs::pc_input::play_buttons;
use tetris_rs::versus::{draw_text, Connection, RoomSettings, VersusRequest, VersusView, WHITE};
use tetris_rs::{Config, Rom};

use crate::settings;

/// Columns of black between the two boards.
const GAP: usize = 8;
/// How long the result stays over the boards (unless Enter is pressed first).
const RESULT_MS: u64 = 6000;
/// The middle of the playfield on the game screen, in pixels from its left.
const PLAYFIELD_CENTRE: usize = 136;
/// How long Escape's "again to quit" lasts.
const ESCAPE_MS: u64 = 2000;

pub struct Versus {
    rom: Arc<Rom>,
    sample_rate: Option<u32>,
    start: Instant,
    /// The room server and the name, for this run (the saved ones unless overridden).
    pub server: String,
    pub name: String,
    room: Option<Room>,
    /// A connection that failed or ended, until the player leaves the lobby.
    lost: Option<String>,
}

struct Room {
    link: Link,
    session: Session,
    code: String,
    /// Our room settings have been sent (once seated as host).
    sent_settings: bool,
    /// The result stays over the boards until this time.
    result_until: Option<u64>,
    /// Escape was pressed during the match at this time.
    escape_at: Option<u64>,
    /// The last thing worth saying in the lobby's status line.
    note: Option<String>,
}

/// What a frame of VERSUS needs from the app.
pub struct Frame<'a> {
    pub held: &'a [String],
    pub pressed: &'a [String],
    pub config: &'a Config,
}

impl Versus {
    pub fn new(rom: Arc<Rom>, sample_rate: Option<u32>, server: String, name: String) -> Self {
        Versus { rom, sample_rate, start: Instant::now(), server, name, room: None, lost: None }
    }

    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
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
        let Some(room) = self.room.as_mut() else { return };
        while let Ok(event) = room.link.rx.try_recv() {
            match event {
                LinkEvent::Message(msg) => room.session.receive(now, msg),
                LinkEvent::Closed(why) => {
                    self.lost = Some(why.to_uppercase());
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
                ..Default::default()
            };
        };
        let s = &room.session;
        let connection = if *s.phase() == Phase::Connecting { Connection::Connecting } else { Connection::InRoom };
        let players = s.players().clone().map(|p| p.map(|p| (p.name, p.ready)));
        let names = s.match_names().clone().map(|n| n.unwrap_or_else(|| "-".to_string()));
        let status = match (s.phase(), &room.note) {
            (Phase::Connecting, _) => "CONNECTING TO THE ROOM".to_string(),
            (Phase::Over { outcome, scores, confirmed }, _) => {
                let what = match outcome {
                    Outcome::Win { seat } => format!("{} WON {} TO {}", names[*seat as usize], scores[*seat as usize], scores[1 - *seat as usize]),
                    Outcome::Draw => format!("A DRAW AT {}", scores[0]),
                    Outcome::Forfeit { seat } => format!("{} LEFT AND FORFEITS", names[*seat as usize]),
                };
                let check = match confirmed {
                    Some(true) => ". CONFIRMED",
                    Some(false) => ". DISPUTED",
                    None => "",
                };
                format!("{what}{check}. READY FOR A REMATCH")
            }
            (_, Some(note)) => note.clone(),
            _ if players[1].is_none() => format!("WAITING FOR A PLAYER. THE CODE IS {}", room.code),
            _ if s.seat().is_none() => "WATCHING".to_string(),
            _ => "WAITING FOR BOTH PLAYERS TO READY UP".to_string(),
        };
        VersusView {
            name: self.name.clone(),
            connection,
            code: room.code.clone(),
            seat: s.seat(),
            players,
            watching: s.spectators(),
            settings: from_protocol(&s.settings()),
            status,
        }
    }

    /// Act on what the player chose in the lobby. Returns true if the saved settings changed.
    pub fn request(&mut self, r: VersusRequest, saved: &mut settings::Versus) -> bool {
        match r {
            VersusRequest::Host => {
                let code = protocol::room_code(random());
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
        let session = Session::new(self.rom.clone(), self.sample_rate, &self.name, watch);
        self.lost = None;
        self.room = Some(Room {
            link: Link::open(&url),
            session,
            code,
            sent_settings: false,
            result_until: None,
            escape_at: None,
            note: None,
        });
    }

    /// Send what the session has for the room, the host's settings once seated. Between matches
    /// the session still ticks, so that a result's report goes out even if the result was
    /// dismissed before both games reached it.
    pub fn flush(&mut self, saved: &settings::Versus) {
        let now = self.now();
        let in_match = self.in_match();
        let Some(room) = self.room.as_mut() else { return };
        if !in_match {
            room.session.tick(now, 0);
        }
        if room.session.seat() == Some(0) && !room.sent_settings && *room.session.phase() == Phase::Lobby {
            room.session.set_settings(to_protocol(&saved.room.to_game()));
            room.sent_settings = true;
        }
        for note in room.session.take_notes() {
            room.note = Some(match note {
                Note::Error(e) => e.to_uppercase(),
                Note::Gone(seat) => format!("PLAYER {} LEFT", seat + 1),
                Note::Desync { .. } => "THE GAMES DISAGREE".to_string(),
                Note::Abandoned => "BOTH PLAYERS LEFT".to_string(),
            });
        }
        for msg in room.session.take_outbox() {
            let _ = room.link.tx.send(msg);
        }
    }

    /// A frame of the match: our game takes the keys (watching: nobody's), and both boards are
    /// drawn side by side with what goes over them. Returns the picture and our game's sound.
    pub fn match_frame(&mut self, f: Frame) -> (Vec<u8>, Vec<f32>) {
        let now = self.now();
        let rom = self.rom.clone();
        let room = self.room.as_mut().expect("a match");
        let pressed = |k: &str| f.pressed.iter().any(|p| p == k);
        // Escape twice to quit (a forfeit, if our game's still on).
        if pressed("Escape") {
            if room.escape_at.is_some_and(|t| now < t + ESCAPE_MS) {
                self.room = None;
                self.lost = Some("YOU LEFT THE MATCH".into());
                return (vec![0x0F; WIDTH * HEIGHT], vec![]);
            }
            room.escape_at = Some(now);
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
        let width = 2 * WIDTH + GAP;
        let mut pixels = vec![0x0Fu8; width * HEIGHT];
        for (seat, x0) in [(left, 0), (1 - left, WIDTH + GAP)] {
            if let Some(board) = &view.boards[seat] {
                for y in 0..HEIGHT {
                    pixels[y * width + x0..][..WIDTH].copy_from_slice(&board[y * WIDTH..][..WIDTH]);
                }
            }
            // Each board's player, along its top edge.
            if let Some(name) = &room.session.match_names()[seat] {
                draw_text(&rom, &mut pixels, width, x0 + 128 - 4 * name.len(), 1, name, WHITE);
            }
            // A player who left: their board stops, marked.
            if room.session.gone()[seat] {
                draw_text(&rom, &mut pixels, width, x0 + PLAYFIELD_CENTRE - 8, 72, "LEFT", WHITE);
            }
        }
        let centre = |text: &str, x0: usize| x0 + 128 - 4 * text.len();
        match room.session.phase() {
            Phase::Countdown { at } => {
                let n = at.saturating_sub(now).div_ceil(1000).to_string();
                for x0 in [0, WIDTH + GAP] {
                    draw_text(&rom, &mut pixels, width, centre(&n, x0), 112, &n, WHITE);
                }
            }
            Phase::Over { outcome, scores, confirmed } => {
                let names = room.session.match_names().clone().map(|n| n.unwrap_or_else(|| "-".to_string()));
                let headline = match outcome {
                    Outcome::Win { seat } if room.session.match_seat() == Some(*seat) => "YOU WIN".to_string(),
                    Outcome::Win { seat } => format!("{} WINS", names[*seat as usize]),
                    Outcome::Draw => "DRAW".to_string(),
                    Outcome::Forfeit { seat } => format!("{} FORFEITS", names[*seat as usize]),
                };
                let lines = [
                    headline,
                    format!("{} - {}", scores[left], scores[1 - left]),
                    match confirmed {
                        Some(true) => "CONFIRMED".into(),
                        Some(false) => "DISPUTED".into(),
                        None => String::new(),
                    },
                ];
                // Over each board's playfield (columns 12..=21 of the game screen).
                for x0 in [0, WIDTH + GAP] {
                    for (i, line) in lines.iter().filter(|l| !l.is_empty()).enumerate() {
                        let x = x0 + PLAYFIELD_CENTRE - 4 * line.len();
                        draw_text(&rom, &mut pixels, width, x, 96 + 12 * i, line, WHITE);
                    }
                }
            }
            _ => {}
        }
        if room.escape_at.is_some_and(|t| now < t + ESCAPE_MS) {
            let text = "ESC AGAIN TO QUIT";
            draw_text(&rom, &mut pixels, width, centre(text, 0), 224, text, WHITE);
        }
        (pixels, std::mem::take(&mut view.samples))
    }
}

fn random() -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    (t.as_nanos() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ std::process::id() as u64
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
    }
}
