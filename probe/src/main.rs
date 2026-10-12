//! A bare client of the game library, the way an online client uses it: the ROM is read at runtime
//! (nothing of it is built in), and games start directly, with no legal screen, title or menus.
//!
//! On its own, one game; it prints the result at game over and closes:
//!
//!     cargo run --release -p probe -- tetris.nes [--type a|b] [--level 0-19] [--height 0-5]
//!         [--music 0-3] [--seed N] [--pieces original|identical]
//!
//! Online, through a room: `server/` under `wrangler dev` (`ws://localhost:8787`, the default), or
//! `cargo run --release -p netplay --bin local-room` (`--server ws://localhost:8788`):
//!
//!     ... -- tetris.nes --host [--level N] [--pieces identical] [--name NAME] [--server URL]
//!     ... -- tetris.nes --join CODE [--name NAME] [--server URL]
//!     ... -- tetris.nes --watch CODE [--server URL]
//!
//! The host's room code, the lobby, the countdown and the result are printed in the terminal; the
//! window shows your game on the left and the other player's on the right (a watcher: host left).
//! Enter readies up (and readies for a rematch after a result).
//!
//! Keys: arrows move and soft-drop, X and Z rotate, Escape quits.

use std::sync::Arc;
use std::time::Instant;

use netplay::{Note, Phase, Session, View, HEIGHT, WIDTH};
use netplay::{Link, LinkEvent};
use protocol::{FromRoom, Outcome, Settings};
use tetris_rs::options::Rules;
use tetris_rs::{buttons, GameSetup, Pieces, Rom, Runner, Seed, Vars};

/// Frames the final screen stays up after a single game ends.
const LINGER: u32 = 180;
/// Columns between the two boards online.
const GAP: usize = 8;
/// The NES's black, for the gap and empty boards.
const BLACK: u8 = 0x0F;

/// NES buttons from the keyboard.
fn nes_buttons(input: &frontend::Input) -> u8 {
    let held = |k: &str| input.held.iter().any(|h| h == k);
    [("ArrowLeft", buttons::LEFT), ("ArrowRight", buttons::RIGHT), ("ArrowDown", buttons::DOWN), ("KeyX", buttons::A), ("KeyZ", buttons::B)]
        .into_iter()
        .filter(|(k, _)| held(k))
        .fold(0, |b, (_, button)| b | button)
}

/// Little-endian BCD, two digits per byte.
fn bcd(bytes: &[u8]) -> u32 {
    bytes.iter().rev().fold(0, |acc, &b| acc * 100 + (b >> 4) as u32 * 10 + (b & 15) as u32)
}

/// One game, offline.
struct Single {
    runner: Runner,
    over: Option<u32>,
    quit: bool,
}

impl frontend::Game for Single {
    fn frame(&mut self, input: &frontend::Input) -> (Vec<u8>, Vec<f32>) {
        let out = self.runner.next_frame(nes_buttons(input));
        if out.game_over && self.over.is_none() {
            let v = Vars::from_ram(&out.ram);
            println!("game over: score {}, lines {}, level {}", bcd(&v.player1.score.0), bcd(&v.player1.lines.0), v.player1.level_number);
            self.over = Some(0);
        }
        if let Some(n) = self.over.as_mut() {
            *n += 1;
            self.quit |= *n >= LINGER;
        }
        self.quit |= input.held.iter().any(|k| k == "Escape");
        ((out.pixels as Box<[u8]>).into_vec(), out.samples)
    }

    fn quit(&self) -> bool {
        self.quit
    }
}

/// A match through a room.
struct Online {
    session: Session,
    link: Link,
    start: Instant,
    /// The host's settings, sent once the room has seated us.
    settings: Option<Settings>,
    /// What was last printed, to print only changes.
    shown: String,
    quit: bool,
}

impl Online {
    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// The lobby and match state as a line, printed when it changes.
    fn status(&self) -> String {
        let players: Vec<String> = self
            .session
            .players()
            .iter()
            .map(|p| match p {
                Some(p) => format!("{}{}", p.name, if p.ready { " (ready)" } else { "" }),
                None => "-".to_string(),
            })
            .collect();
        let seat = match self.session.seat() {
            Some(0) => "host",
            Some(_) => "opponent",
            None => "watching",
        };
        let s = self.session.settings();
        let lobby = format!(
            "{seat} | {} vs {} | {} watching | level {}, {:?} pieces",
            players[0],
            players[1],
            self.session.spectators(),
            s.level,
            s.pieces
        );
        match self.session.phase() {
            Phase::Connecting => "connecting".to_string(),
            Phase::Lobby => format!("{lobby} | Enter to ready up"),
            Phase::Countdown { .. } => format!("{lobby} | starting"),
            Phase::Playing => format!("{lobby} | playing"),
            Phase::Over { outcome, scores, confirmed } => {
                let name = |seat: u8| players[seat as usize].split(" (").next().unwrap_or("").to_string();
                let what = match outcome {
                    Outcome::Win { seat } => format!("{} wins", name(*seat)),
                    Outcome::Draw => "draw".to_string(),
                    Outcome::Forfeit { seat } => format!("{} forfeits", name(*seat)),
                };
                let check = match confirmed {
                    None => "",
                    Some(true) => ", confirmed",
                    Some(false) => ", DISPUTED",
                };
                format!("{lobby} | {what}: {} to {}{check} | Enter for a rematch", scores[0], scores[1])
            }
        }
    }

    /// Both boards side by side: ours (or the host's, watching) on the left.
    fn compose(&self, view: View) -> Vec<u8> {
        let left = self.session.match_seat().unwrap_or(0) as usize;
        let [a, b] = [left, 1 - left].map(|s| view.boards[s].as_ref());
        let width = 2 * WIDTH + GAP;
        let mut pixels = vec![BLACK; width * HEIGHT];
        for (board, x0) in [(a, 0), (b, WIDTH + GAP)] {
            if let Some(board) = board {
                for y in 0..HEIGHT {
                    pixels[y * width + x0..][..WIDTH].copy_from_slice(&board[y * WIDTH..][..WIDTH]);
                }
            }
        }
        pixels
    }
}

impl frontend::Game for Online {
    fn frame(&mut self, input: &frontend::Input) -> (Vec<u8>, Vec<f32>) {
        let now = self.now();
        while let Ok(msg) = self.link.rx.try_recv() {
            match msg {
                LinkEvent::Message(msg) => {
                    if let (FromRoom::Welcome { seat: Some(0) }, Some(settings)) = (&msg, self.settings.take()) {
                        self.session.set_settings(settings);
                    }
                    self.session.receive(now, msg);
                }
                LinkEvent::Closed(why) => {
                    println!("disconnected: {why}");
                    self.quit = true;
                }
            }
        }
        if input.pressed.iter().any(|k| k == "Enter") && self.session.seat().is_some() {
            let ready = matches!(self.session.phase(), Phase::Lobby | Phase::Over { .. });
            if ready {
                self.session.set_ready(true);
            }
        }
        let mut view = self.session.tick(now, nes_buttons(input));
        for note in self.session.take_notes() {
            match note {
                Note::Error(e) => println!("room: {e}"),
                Note::Gone(seat) => println!("seat {seat} disconnected"),
                Note::Desync { seat, frame } => println!("DESYNC: seat {seat}'s game differs at frame {frame}"),
                Note::Abandoned => println!("both players left; no result"),
                Note::Kicked => println!("the host removed you"),
            }
        }
        for msg in self.session.take_outbox() {
            let _ = self.link.tx.send(msg);
        }
        let status = self.status();
        if status != self.shown {
            println!("{status}");
            self.shown = status;
        }
        self.quit |= input.held.iter().any(|k| k == "Escape");
        let samples = std::mem::take(&mut view.samples);
        (self.compose(view), samples)
    }

    fn quit(&self) -> bool {
        self.quit
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: probe <tetris.nes> [--host | --join CODE | --watch CODE] [options]; see the source");
        std::process::exit(2);
    };
    let flag = |name: &str| args.windows(2).find(|w| w[0] == name).map(|w| w[1].as_str());
    let has = |name: &str| args.iter().any(|a| a == name);
    let num = |name: &str, default: u32| flag(name).map_or(default, |v| v.parse().unwrap_or_else(|_| panic!("{name} {v}")));
    let file = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let rom = Arc::new(Rom::new(&file).unwrap_or_else(|e| panic!("{path}: {e}")));
    let random = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;

    let online = has("--host") || flag("--join").is_some() || flag("--watch").is_some();
    if !online {
        let seed = flag("--seed").map_or(random as u32, |v| v.parse().unwrap_or_else(|_| panic!("--seed {v}")));
        let setup = GameSetup {
            b_type: flag("--type") == Some("b"),
            level: num("--level", 0) as u8,
            height: num("--height", 0) as u8,
            music: num("--music", 0) as u8,
            seed: Seed::from_u32(seed),
            pieces: if flag("--pieces") == Some("identical") { Pieces::Identical } else { Pieces::Original },
            rules: Rules::ORIGINAL,
            next_piece: true,
        };
        println!("seed {seed}: {setup:?}");
        frontend::run("Probe", frontend::Options::default(), |rate| Single {
            runner: Runner::start_game(rom, Some(rate), setup),
            over: None,
            quit: false,
        });
        return;
    }

    let server = flag("--server").unwrap_or("ws://localhost:8787").trim_end_matches('/').to_string();
    let code = match (flag("--join"), flag("--watch")) {
        (Some(c), _) | (_, Some(c)) => c.to_uppercase(),
        _ => protocol::room_code(random),
    };
    if !protocol::valid_code(&code) {
        eprintln!("{code} isn't a room code (four letters, no I or O)");
        std::process::exit(2);
    }
    if has("--host") {
        println!("room code: {code}  (others: --join {code}, or --watch {code})");
    }
    let settings = has("--host").then(|| Settings {
        level: num("--level", 18) as u8,
        pieces: if flag("--pieces") == Some("identical") { protocol::Pieces::Identical } else { protocol::Pieces::Separate },
        ..Settings::default()
    });
    let link = Link::open(&format!("{server}/room/{code}"));
    let name = flag("--name").unwrap_or(if has("--host") { "host" } else { "guest" });
    let options = frontend::Options { width: 2 * WIDTH + GAP, ..frontend::Options::default() };
    frontend::run("Probe", options, |rate| Online {
        session: Session::new(rom, Some(rate), name, flag("--watch").is_some()),
        link,
        start: Instant::now(),
        settings,
        shown: String::new(),
        quit: false,
    });
}
