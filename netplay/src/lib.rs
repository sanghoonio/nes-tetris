//! The client side of an online match, with no UI or networking: whoever runs a `Session` passes
//! in the room's messages (`receive`) and calls `tick` once per frame with the player's buttons; it
//! hands back what to draw and the messages to send (`take_outbox`).
//!
//! A player's own game runs here with no network in the loop. Every 15 frames its buttons go to
//! the room in a batch. The other seats' games are replicas, rebuilt from their batches as they
//! arrive and played back steadily a fixed delay behind, so they move smoothly. Every 60 frames
//! each game's checksum is sent, and each replica checks itself against it.

pub mod link;
#[cfg(not(target_arch = "wasm32"))]
pub mod local;

pub use link::{Link, LinkEvent, Listing, LocalRoom, RoomList};

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use protocol::{FromRoom, Outcome, Player, Seat, Settings, ToRoom, BATCH_FRAMES, CHECKSUM_FRAMES};
use tetris_rs::options::{LeftRight, Rules};
use tetris_rs::vars::is_sound_engine;
use tetris_rs::{FrameOut, GameSetup, Pieces, Rom, Runner, Seed, Vars};

pub const WIDTH: usize = tetris_rs::frame::WIDTH;
pub const HEIGHT: usize = tetris_rs::frame::HEIGHT;
pub type Picture = Box<[u8; WIDTH * HEIGHT]>;

/// How far a replica plays behind the newest frame it has: 350 ms, a batch plus the network.
pub const REPLICA_DELAY: usize = 21;

/// Where the session is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Not yet welcomed by the room.
    Connecting,
    Lobby,
    /// A match starts at `at` (ms, the clock `tick` is given).
    Countdown { at: u64 },
    Playing,
    /// The match is decided; `confirmed` once the players' reports are in.
    Over { outcome: Outcome, scores: [u32; 2], confirmed: Option<bool> },
}

/// Things worth telling the player, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    Error(String),
    Gone(Seat),
    /// A replica's game no longer matches the player's own checksum: a bug or a modified client.
    Desync { seat: Seat, frame: u32 },
    Abandoned,
    /// The host removed this player from the room.
    Kicked,
}

/// What to draw this frame.
pub struct View {
    /// Each seat's board: the player's own game, or a replica.
    pub boards: [Option<Picture>; 2],
    /// The player's own game's sound.
    pub samples: Vec<f32>,
}

pub struct Session {
    rom: Arc<Rom>,
    sample_rate: Option<u32>,
    phase: Phase,
    seat: Option<Seat>,
    players: [Option<Player>; 2],
    spectators: u32,
    settings: Settings,
    match_id: u32,
    seeds: [u32; 2],
    /// The player's own game, from the end of the countdown.
    own: Option<Own>,
    /// The host's bot's seat in this match, and its game (run here, on the host's machine).
    bot_seat: Option<Seat>,
    bot: Option<(Own, bots::Bot)>,
    replicas: [Option<Replica>; 2],
    /// The result's frames, until both games have reached them and the report is sent.
    report_due: Option<[u32; 2]>,
    /// The seat this session played the last (or current) match from. The room can move a player
    /// afterwards (the opponent becomes host when the host leaves); the match keeps its seats.
    match_seat: Option<Seat>,
    /// Seats whose player left during this match.
    gone: [bool; 2],
    /// The match's outcome, once decided while the winner plays on.
    decided: Option<Outcome>,
    /// The match is paused: by this session (its games stop) or, watching, by the host.
    paused: bool,
    /// Who played the match (or is playing it), as it started: a player who leaves loses their
    /// seat, but the result still names them.
    match_names: [Option<String>; 2],
    /// And their `Player::id`s, which tell them apart (names needn't).
    match_ids: [Option<u32>; 2],
    /// The music the player's own game plays (0..=2, 3 off); replicas play none that's heard.
    music: u8,
    outbox: Vec<ToRoom>,
    notes: Vec<Note>,
}

struct Own {
    runner: Runner,
    /// The seat whose game this is, when it isn't the sender's own (the host's bot).
    seat: Option<Seat>,
    rules: Rules,
    /// The game's RAM after the last frame (for a bot to look at).
    ram: Box<[u8; 0x800]>,
    /// Score after each frame (`scores[n]` after `n` frames).
    scores: Vec<u32>,
    batch: Vec<u8>,
    /// Frames played, and whether the game has ended.
    frames: u32,
    over: bool,
    last: Option<Picture>,
}

struct Replica {
    runner: Runner,
    /// Every frame's buttons received so far.
    inputs: Vec<u8>,
    /// Whether the game has ended (no more inputs to come).
    over: bool,
    played: usize,
    /// Playback has begun (once a delay's worth of frames is in).
    rolling: bool,
    scores: Vec<u32>,
    expected: HashMap<u32, u64>,
    computed: HashMap<u32, u64>,
    desynced: bool,
    /// Checksums compared and found equal.
    verified: u32,
    last: Option<Picture>,
}

impl Session {
    /// A session that joins a room as `name`, or to watch.
    pub fn new(rom: Arc<Rom>, sample_rate: Option<u32>, name: &str, watch: bool) -> Self {
        Session {
            rom,
            sample_rate,
            phase: Phase::Connecting,
            seat: None,
            players: [None, None],
            spectators: 0,
            settings: Settings::default(),
            match_id: 0,
            seeds: [0, 0],
            own: None,
            bot_seat: None,
            bot: None,
            replicas: [None, None],
            report_due: None,
            match_seat: None,
            gone: [false; 2],
            decided: None,
            paused: false,
            match_names: [None, None],
            match_ids: [None, None],
            music: 0,
            outbox: vec![ToRoom::Hello { name: name.to_string(), watch }],
            notes: vec![],
        }
    }

    /// The music for the player's own games from the next match on (0..=2 music 1..3, 3 off).
    pub fn set_music(&mut self, music: u8) {
        self.music = music.min(3);
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// This session's seat; none when watching.
    pub fn seat(&self) -> Option<Seat> {
        self.seat
    }

    pub fn players(&self) -> &[Option<Player>; 2] {
        &self.players
    }

    pub fn spectators(&self) -> u32 {
        self.spectators
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// Each seat's score as this session last saw it (own game, the bot's, or a replica).
    pub fn scores(&self) -> [Option<u32>; 2] {
        [0, 1].map(|s| match self.played_here(s) {
            Some(own) => own.map(|o| *o.scores.last().unwrap()),
            None => self.replicas[s].as_ref().map(|r| *r.scores.last().unwrap()),
        })
    }

    /// The game for seat `s` if this session plays it (its own, or its bot's): some (none until
    /// it starts); none if it's replicated.
    fn played_here(&self, s: usize) -> Option<Option<&Own>> {
        if self.match_seat == Some(s as Seat) {
            Some(self.own.as_ref())
        } else if self.bot_seat == Some(s as Seat) {
            Some(self.bot.as_ref().map(|b| &b.0))
        } else {
            None
        }
    }

    /// Checksums each replica has matched so far (none for the player's own seat).
    pub fn verified(&self) -> [u32; 2] {
        [0, 1].map(|s| self.replicas[s].as_ref().map_or(0, |r| r.verified))
    }

    /// The seat this session played the last (or current) match from; none when watching.
    pub fn match_seat(&self) -> Option<Seat> {
        self.match_seat
    }

    /// Who played the last (or current) match, by seat, as it started.
    pub fn match_names(&self) -> &[Option<String>; 2] {
        &self.match_names
    }

    /// Who played the last (or current) match, as `Player::id`s.
    pub fn match_ids(&self) -> [Option<u32>; 2] {
        self.match_ids
    }

    /// The outcome while the match is decided but the winner plays on.
    pub fn decided(&self) -> Option<Outcome> {
        self.decided
    }

    /// Seats whose player left during this match (their board stops where it is).
    pub fn gone(&self) -> [bool; 2] {
        self.gone
    }

    pub fn take_outbox(&mut self) -> Vec<ToRoom> {
        std::mem::take(&mut self.outbox)
    }

    pub fn take_notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }

    /// The host's choice.
    pub fn set_settings(&mut self, settings: Settings) {
        self.outbox.push(ToRoom::Settings { settings });
    }

    pub fn set_ready(&mut self, ready: bool) {
        self.outbox.push(ToRoom::Ready { ready });
    }

    /// As host, remove the opponent (between matches).
    pub fn kick(&mut self) {
        self.outbox.push(ToRoom::Kick);
    }

    /// Whether this session plays a bot in the current match (the host's own).
    pub fn playing_bot(&self) -> bool {
        self.bot_seat.is_some()
    }

    /// Pause the match against our own bot, or carry on: our games (ours and the bot's) stop
    /// where they are; spectators are told.
    pub fn set_paused(&mut self, paused: bool) {
        if paused != self.paused && self.bot_seat.is_some() {
            self.paused = paused;
            self.outbox.push(ToRoom::Pause { paused });
        }
    }

    /// Whether the match is paused (by us, or watching, by the host).
    pub fn paused(&self) -> bool {
        self.paused && self.phase == Phase::Playing
    }

    /// Give up the match: a forfeit, staying in the room.
    pub fn resign(&mut self) {
        self.outbox.push(ToRoom::Resign);
    }

    /// As host, seat a bot from the roster by name (between matches). This session plays it.
    pub fn add_bot(&mut self, name: &str) {
        self.outbox.push(ToRoom::AddBot { name: name.to_string() });
    }

    pub fn receive(&mut self, now: u64, msg: FromRoom) {
        match msg {
            FromRoom::Welcome { seat } => {
                self.seat = seat;
                if self.phase == Phase::Connecting {
                    self.phase = Phase::Lobby;
                }
            }
            FromRoom::Lobby { players, spectators, settings, .. } => {
                self.players = players;
                self.spectators = spectators;
                self.settings = settings;
            }
            FromRoom::Start { match_id, settings, seeds, countdown_ms } => {
                self.match_id = match_id;
                self.settings = settings;
                self.seeds = seeds;
                self.own = None;
                self.report_due = None;
                self.gone = [false; 2];
                self.decided = None;
                self.paused = false;
                self.match_seat = self.seat;
                self.match_names = self.players.clone().map(|p| p.map(|p| p.name));
                self.match_ids = self.players.clone().map(|p| p.map(|p| p.id));
                // The host plays its bot's game here.
                self.bot = None;
                self.bot_seat = (self.seat == Some(0))
                    .then(|| (0..2).find(|&s| self.players[s].as_ref().is_some_and(|p| p.bot)))
                    .flatten()
                    .map(|s| s as Seat);
                self.replicas = [0, 1].map(|s| {
                    (self.match_seat != Some(s as Seat) && self.bot_seat != Some(s as Seat))
                        .then(|| Replica::new(Runner::start_game(self.rom.clone(), None, self.setup(s))))
                });
                self.phase = Phase::Countdown { at: now + countdown_ms };
            }
            FromRoom::Inputs { seat, frame, buttons, over, .. } => {
                if let Some(r) = self.replicas[seat as usize].as_mut() {
                    if frame as usize == r.inputs.len() {
                        r.inputs.extend_from_slice(&buttons);
                        r.over = over;
                    }
                }
            }
            FromRoom::Checksum { seat, frame, hash } => {
                if let Some(r) = self.replicas[seat as usize].as_mut() {
                    r.expected.insert(frame, hash);
                    if r.check(frame) {
                        self.notes.push(Note::Desync { seat, frame });
                    }
                }
            }
            FromRoom::Gone { seat } => {
                self.gone[seat as usize] = true;
                self.notes.push(Note::Gone(seat));
            }
            FromRoom::Decided { match_id, outcome } => {
                if match_id == self.match_id {
                    self.decided = Some(outcome);
                }
            }
            FromRoom::Result { match_id, outcome, scores, frames } => {
                if match_id != self.match_id {
                    return;
                }
                self.phase = Phase::Over { outcome, scores, confirmed: None };
                self.paused = false;
                if self.match_seat.is_some() {
                    self.report_due = Some(frames);
                    self.report_if_ready();
                }
            }
            FromRoom::Confirmed { match_id, agreed } => {
                if let Phase::Over { confirmed, .. } = &mut self.phase {
                    if match_id == self.match_id {
                        *confirmed = Some(agreed);
                    }
                }
            }
            FromRoom::Abandoned { .. } => {
                self.phase = Phase::Lobby;
                self.own = None;
                self.bot = None;
                self.replicas = [None, None];
                self.notes.push(Note::Abandoned);
            }
            FromRoom::Error { message } => self.notes.push(Note::Error(message)),
            FromRoom::Kicked => self.notes.push(Note::Kicked),
            FromRoom::Paused { paused } => self.paused = paused,
        }
    }

    /// One frame: the player's game takes `buttons` (NES bits) if it's running; replicas play on.
    pub fn tick(&mut self, now: u64, buttons: u8) -> View {
        if let Phase::Countdown { at } = self.phase {
            if now >= at {
                self.phase = Phase::Playing;
                if let Some(s) = self.match_seat {
                    let setup = GameSetup { music: self.music, ..self.setup(s as usize) };
                    self.own = Some(Own::new(Runner::start_game(self.rom.clone(), self.sample_rate, setup), None, setup.rules));
                }
                if let Some(s) = self.bot_seat {
                    let setup = GameSetup { music: 3, ..self.setup(s as usize) };
                    let name = self.match_names[s as usize].clone().unwrap_or_default();
                    let profile = bots::profile(&name).unwrap_or(bots::ROSTER[2].1);
                    let bot = bots::Bot::new(&self.rom, profile, self.seeds[s as usize] as u64 ^ self.match_id as u64);
                    self.bot = Some((Own::new(Runner::start_game(self.rom.clone(), None, setup), Some(s), setup.rules), bot));
                }
            }
        }
        let mut samples = vec![];
        if self.phase == Phase::Playing && !(self.paused && self.bot_seat.is_some()) {
            if let Some(own) = self.own.as_mut() {
                samples = own.step(buttons, &mut self.outbox);
            }
            if let Some((game, bot)) = self.bot.as_mut() {
                let pressed = bot.buttons(&game.ram, &game.rules);
                game.step(pressed, &mut self.outbox);
            }
        }
        let decided = matches!(self.phase, Phase::Over { .. });
        for s in 0..2 {
            if let Some(r) = self.replicas[s].as_mut() {
                // Once decided, replicas run straight to the result's frames (for the report).
                let target = self.report_due.map(|f| f[s] as usize);
                let steps = match target {
                    Some(t) if decided => t.saturating_sub(r.played).min(r.inputs.len() - r.played),
                    _ if decided => 0,
                    _ => r.steps(),
                };
                for _ in 0..steps {
                    if r.step() {
                        self.notes.push(Note::Desync { seat: s as Seat, frame: r.played as u32 });
                    }
                }
            }
        }
        self.report_if_ready();
        let boards = [0, 1].map(|s| match self.played_here(s) {
            Some(own) => own.and_then(|o| o.last.clone()),
            None => self.replicas[s].as_ref().and_then(|r| r.last.clone()),
        });
        View { boards, samples }
    }

    /// Once the result's frames are reached in every game, report the scores there.
    fn report_if_ready(&mut self) {
        let Some(frames) = self.report_due else { return };
        let mut scores = [0; 2];
        for s in 0..2 {
            let f = frames[s] as usize;
            let have = match self.played_here(s) {
                Some(own) => own.map(|o| o.scores.get(f).copied()),
                None => self.replicas[s].as_ref().map(|r| (r.played >= f).then(|| r.scores[f])),
            };
            match have {
                Some(Some(score)) => scores[s] = score,
                // No game for this seat (it never started here): report the room's own figure.
                None => {
                    if let Phase::Over { scores: official, .. } = self.phase {
                        scores[s] = official[s];
                    }
                }
                Some(None) => return,
            }
        }
        self.report_due = None;
        self.outbox.push(ToRoom::Report { match_id: self.match_id, scores });
    }

    fn setup(&self, seat: usize) -> GameSetup {
        let s = self.settings;
        let r = s.rules;
        GameSetup {
            b_type: false,
            level: s.level,
            height: 0,
            music: 0,
            seed: Seed::from_u32(self.seeds[seat]),
            pieces: match s.pieces {
                protocol::Pieces::Separate => Pieces::Original,
                protocol::Pieces::Identical => Pieces::Identical,
            },
            rules: Rules {
                das_delay: r.das_delay,
                das_repeat: r.das_repeat,
                left_right: match r.left_right {
                    protocol::LeftRight::Both => LeftRight::Both,
                    protocol::LeftRight::Last => LeftRight::Last,
                    protocol::LeftRight::Neither => LeftRight::Neither,
                },
                slowdown: true,
                hard_drop: r.hard_drop,
                ghost: r.ghost,
            },
            next_piece: r.next_piece,
        }
    }
}

impl Own {
    fn new(runner: Runner, seat: Option<Seat>, rules: Rules) -> Own {
        Own { runner, seat, rules, ram: Box::new([0; 0x800]), scores: vec![0], batch: vec![], frames: 0, over: false, last: None }
    }

    /// A frame of the player's game (or the bot's); inputs and checksums go out as they fall due.
    fn step(&mut self, buttons: u8, outbox: &mut Vec<ToRoom>) -> Vec<f32> {
        let out = self.runner.next_frame(buttons);
        self.last = Some(out.pixels.clone());
        *self.ram = out.ram;
        if self.over {
            return out.samples;
        }
        self.frames += 1;
        self.batch.push(buttons);
        let score = score(&out);
        self.scores.push(score);
        self.over = out.game_over;
        if self.batch.len() == BATCH_FRAMES || self.over {
            let buttons = std::mem::take(&mut self.batch);
            let frame = self.frames - buttons.len() as u32;
            outbox.push(ToRoom::Inputs { frame, buttons, score, over: self.over, seat: self.seat });
        }
        if self.frames.is_multiple_of(CHECKSUM_FRAMES) {
            outbox.push(ToRoom::Checksum { frame: self.frames, hash: checksum(&out.ram), seat: self.seat });
        }
        out.samples
    }
}

impl Replica {
    fn new(runner: Runner) -> Self {
        Replica {
            runner,
            inputs: vec![],
            over: false,
            played: 0,
            rolling: false,
            scores: vec![0],
            expected: HashMap::new(),
            computed: HashMap::new(),
            desynced: false,
            verified: 0,
            last: None,
        }
    }

    /// Frames to play this tick: one, to stay `REPLICA_DELAY` behind; more when further behind
    /// (catching up after a stall, or a spectator arriving mid-match); none when out of input.
    fn steps(&mut self) -> usize {
        let ahead = self.inputs.len() - self.played;
        if !self.rolling {
            self.rolling = ahead >= REPLICA_DELAY || (self.over && ahead > 0);
        }
        if !self.rolling || ahead == 0 {
            0
        } else if ahead > 2 * REPLICA_DELAY + BATCH_FRAMES {
            (ahead - REPLICA_DELAY).min(60)
        } else if ahead > REPLICA_DELAY + BATCH_FRAMES {
            2
        } else {
            1
        }
    }

    /// Play the next frame. Returns true if it just found a desync.
    fn step(&mut self) -> bool {
        let out = self.runner.next_frame(self.inputs[self.played]);
        self.played += 1;
        self.scores.push(score(&out));
        self.last = Some(out.pixels.clone());
        let frame = self.played as u32;
        if frame.is_multiple_of(CHECKSUM_FRAMES) {
            self.computed.insert(frame, checksum(&out.ram));
            return self.check(frame);
        }
        false
    }

    /// Compare the checksum at `frame` if both are known. Reports a desync once.
    fn check(&mut self, frame: u32) -> bool {
        match (self.expected.get(&frame), self.computed.get(&frame)) {
            (Some(a), Some(b)) if a == b => {
                self.verified += 1;
                false
            }
            (Some(_), Some(_)) if !self.desynced => {
                self.desynced = true;
                true
            }
            _ => false,
        }
    }
}

/// Player 1's score, from the game's BCD.
fn score(out: &FrameOut) -> u32 {
    let bcd = Vars::from_ram(&out.ram).player1.score.0;
    bcd.iter().rev().fold(0, |acc, &b| acc * 100 + (b >> 4) as u32 * 10 + (b & 15) as u32)
}

/// FNV-1a over the game's RAM, less the sound engine's (each player picks their own music).
fn checksum(ram: &[u8; 0x800]) -> u64 {
    static SOUND: OnceLock<[bool; 0x800]> = OnceLock::new();
    let sound = SOUND.get_or_init(|| {
        let mut sound = [false; 0x800];
        for &(name, addr, size) in Vars::LAYOUT {
            if is_sound_engine(name) {
                sound[addr..addr + size].fill(true);
            }
        }
        sound
    });
    ram.iter()
        .zip(sound)
        .filter(|(_, &s)| !s)
        .fold(0xCBF2_9CE4_8422_2325, |h, (&b, _)| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3))
}
