//! The client side of an online match, with no UI or networking: whoever runs a `Session` passes
//! in the room's messages (`receive`) and calls `tick` once per frame with the player's buttons; it
//! hands back what to draw and the messages to send (`take_outbox`).
//!
//! A player's own game runs here with no network in the loop. Every 15 frames its buttons go to
//! the room in a batch. The other seats' games are replicas, rebuilt from their batches as they
//! arrive and played back steadily a fixed delay behind, so they move smoothly. Every 60 frames
//! each game's checksum is sent, and each replica checks itself against it.

use std::collections::HashMap;
use std::sync::Arc;

use protocol::{FromRoom, Outcome, Player, Seat, Settings, ToRoom, BATCH_FRAMES, CHECKSUM_FRAMES};
use tetris_rs::options::{LeftRight, Rules};
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
    replicas: [Option<Replica>; 2],
    /// The result's frames, until both games have reached them and the report is sent.
    report_due: Option<[u32; 2]>,
    outbox: Vec<ToRoom>,
    notes: Vec<Note>,
}

struct Own {
    runner: Runner,
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
            replicas: [None, None],
            report_due: None,
            outbox: vec![ToRoom::Hello { name: name.to_string(), watch }],
            notes: vec![],
        }
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

    /// Each seat's score as this session last saw it (own game, or replica).
    pub fn scores(&self) -> [Option<u32>; 2] {
        [0, 1].map(|s| {
            if self.seat == Some(s as Seat) {
                self.own.as_ref().map(|o| *o.scores.last().unwrap())
            } else {
                self.replicas[s].as_ref().map(|r| *r.scores.last().unwrap())
            }
        })
    }

    /// Checksums each replica has matched so far (none for the player's own seat).
    pub fn verified(&self) -> [u32; 2] {
        [0, 1].map(|s| self.replicas[s].as_ref().map_or(0, |r| r.verified))
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
                self.replicas = [0, 1].map(|s| {
                    (self.seat != Some(s as Seat)).then(|| Replica::new(Runner::start_game(self.rom.clone(), None, self.setup(s))))
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
            FromRoom::Gone { seat } => self.notes.push(Note::Gone(seat)),
            FromRoom::Result { match_id, outcome, scores, frames } => {
                if match_id != self.match_id {
                    return;
                }
                self.phase = Phase::Over { outcome, scores, confirmed: None };
                if self.seat.is_some() {
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
                self.replicas = [None, None];
                self.notes.push(Note::Abandoned);
            }
            FromRoom::Error { message } => self.notes.push(Note::Error(message)),
        }
    }

    /// One frame: the player's game takes `buttons` (NES bits) if it's running; replicas play on.
    pub fn tick(&mut self, now: u64, buttons: u8) -> View {
        if let Phase::Countdown { at } = self.phase {
            if now >= at {
                self.phase = Phase::Playing;
                if let Some(s) = self.seat {
                    let runner = Runner::start_game(self.rom.clone(), self.sample_rate, self.setup(s as usize));
                    self.own = Some(Own { runner, scores: vec![0], batch: vec![], frames: 0, over: false, last: None });
                }
            }
        }
        let mut samples = vec![];
        if self.phase == Phase::Playing {
            if let Some(own) = self.own.as_mut() {
                samples = own.step(buttons, &mut self.outbox);
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
        let boards = [0, 1].map(|s| {
            if self.seat == Some(s as Seat) {
                self.own.as_ref().and_then(|o| o.last.clone())
            } else {
                self.replicas[s].as_ref().and_then(|r| r.last.clone())
            }
        });
        View { boards, samples }
    }

    /// Once the result's frames are reached in every game, report the scores there.
    fn report_if_ready(&mut self) {
        let Some(frames) = self.report_due else { return };
        let mut scores = [0; 2];
        for s in 0..2 {
            let f = frames[s] as usize;
            let have = if self.seat == Some(s as Seat) {
                self.own.as_ref().map(|o| o.scores.get(f).copied())
            } else {
                self.replicas[s].as_ref().map(|r| (r.played >= f).then(|| r.scores[f]))
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
    /// A frame of the player's game; inputs and checksums go out as they fall due.
    fn step(&mut self, buttons: u8, outbox: &mut Vec<ToRoom>) -> Vec<f32> {
        let out = self.runner.next_frame(buttons);
        self.last = Some(out.pixels.clone());
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
            outbox.push(ToRoom::Inputs { frame, buttons, score, over: self.over });
        }
        if self.frames % CHECKSUM_FRAMES == 0 {
            outbox.push(ToRoom::Checksum { frame: self.frames, hash: checksum(&out.ram) });
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
        if frame % CHECKSUM_FRAMES == 0 {
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

/// FNV-1a over the game's RAM.
fn checksum(ram: &[u8; 0x800]) -> u64 {
    ram.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3))
}
