//! The room: who's seated, the settings, and the match in progress. Whoever runs it (the Worker,
//! or a test) numbers the connections, feeds in what happens (`Event`), and sends the messages
//! that come back.

use serde::{Deserialize, Serialize};

use crate::{
    clean_name, ConnId, FromRoom, Outcome, Pieces, Player, RoomState, RoomSummary, Seat, Settings, ToRoom,
    BATCH_FRAMES,
};

/// Longest input batch the room accepts, in frames.
const MAX_BATCH: usize = 4 * BATCH_FRAMES;

pub enum Event {
    /// A connection opened; it says `Hello` next.
    Joined(ConnId),
    Left(ConnId),
    Message(ConnId, ToRoom),
}

/// A message for one connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Send {
    pub to: ConnId,
    pub msg: FromRoom,
}

/// Everything but the match in progress survives being saved and restored (`serde`), which is
/// what the Worker does between messages so that an idle room can sleep. A match's input log
/// lives in memory only.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Room {
    rng: u64,
    settings: Settings,
    seats: [Option<Occupant>; 2],
    spectators: Vec<ConnId>,
    /// Connections that haven't said `Hello` yet.
    pending: Vec<ConnId>,
    next_match: u32,
    /// The next player's `id`.
    #[serde(default)]
    next_player: u32,
    #[serde(skip)]
    game: Option<Match>,
    /// The last result, until the players' reports are in.
    awaiting: Option<Awaiting>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Occupant {
    /// Numbered in order of arrival (`Player::id`).
    #[serde(default)]
    id: u32,
    /// A bot, whose `conn` is the host that plays it.
    #[serde(default)]
    bot: bool,
    /// None once the player has left during a match (the seat is kept until it ends).
    conn: Option<ConnId>,
    name: String,
    ready: bool,
}

#[derive(Clone, Debug)]
struct Match {
    id: u32,
    seeds: [u32; 2],
    plays: [Play; 2],
    /// Decided while the winner plays on.
    decided: Option<Outcome>,
}

#[derive(Clone, Debug, Default)]
struct Play {
    /// The buttons of every frame so far.
    inputs: Vec<u8>,
    score: u32,
    over: bool,
    /// The player's connection dropped (after topping out; before, it's a forfeit).
    gone: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Awaiting {
    match_id: u32,
    scores: [u32; 2],
    reports: [Option<[u32; 2]>; 2],
    /// Who played each seat, if still connected when the result came: the reports expected.
    /// (Seats can change after a match: a lone opponent becomes host.)
    players: [Option<ConnId>; 2],
}

impl Room {
    /// An empty room; `seed` seeds the games' seeds.
    pub fn new(seed: u64) -> Self {
        Room {
            rng: seed | 1,
            settings: Settings::default(),
            seats: [None, None],
            spectators: vec![],
            pending: vec![],
            next_match: 1,
            next_player: 1,
            game: None,
            awaiting: None,
        }
    }

    /// Whether a match is on.
    pub fn playing(&self) -> bool {
        self.game.is_some()
    }

    /// The room as the list of rooms shows it, under `code`: none if it isn't public or has no
    /// host.
    pub fn summary(&self, code: &str) -> Option<RoomSummary> {
        let host = self.seats[0].as_ref()?;
        if !self.settings.public {
            return None;
        }
        let players = self.seats.iter().flatten().count() as u8;
        Some(RoomSummary {
            code: code.to_string(),
            host: host.name.clone(),
            players,
            spectators: self.spectators.len() as u32,
            level: self.settings.level,
            same_seed: self.settings.pieces == Pieces::Identical,
            state: match (self.game.is_some(), players) {
                (true, _) => RoomState::Playing,
                (false, 2) => RoomState::Full,
                _ => RoomState::Open,
            },
        })
    }

    /// Whether anyone is connected.
    pub fn empty(&self) -> bool {
        self.conns().is_empty() && self.pending.is_empty()
    }

    pub fn handle(&mut self, event: Event) -> Vec<Send> {
        let mut out = Out(vec![]);
        match event {
            Event::Joined(conn) => self.pending.push(conn),
            Event::Left(conn) => self.left(conn, &mut out),
            Event::Message(conn, msg) => self.message(conn, msg, &mut out),
        }
        out.0
    }

    /// The seat `conn` plays in itself (not a bot's).
    fn seat_of(&self, conn: ConnId) -> Option<Seat> {
        (0..2).find(|&s| self.seats[s].as_ref().is_some_and(|o| !o.bot && o.conn == Some(conn))).map(|s| s as Seat)
    }

    /// The seats of bots `conn` plays.
    fn bots_of(&self, conn: ConnId) -> Vec<usize> {
        (0..2).filter(|&s| self.seats[s].as_ref().is_some_and(|o| o.bot && o.conn == Some(conn))).collect()
    }

    /// The seat a game message from `conn` is for: `seat` if it's one of its bots', else its own.
    fn playing_seat(&self, conn: ConnId, seat: Option<Seat>) -> Option<Seat> {
        match seat {
            Some(s) => self.bots_of(conn).contains(&(s as usize)).then_some(s),
            None => self.seat_of(conn),
        }
    }

    /// Everyone who said `Hello` and is still here.
    fn conns(&self) -> Vec<ConnId> {
        self.seats.iter().flatten().filter(|o| !o.bot).filter_map(|o| o.conn).chain(self.spectators.iter().copied()).collect()
    }

    fn lobby(&self) -> FromRoom {
        FromRoom::Lobby {
            players: self.seats.clone().map(|o| o.map(|o| Player { name: o.name, ready: o.ready, id: o.id, bot: o.bot })),
            spectators: self.spectators.len() as u32,
            settings: self.settings,
            playing: self.game.is_some(),
        }
    }

    fn message(&mut self, conn: ConnId, msg: ToRoom, out: &mut Out) {
        if let ToRoom::Hello { name, watch } = msg {
            return self.hello(conn, &name, watch, out);
        }
        if !self.conns().contains(&conn) {
            return out.error(conn, "say Hello first");
        }
        let seat = self.seat_of(conn);
        match msg {
            ToRoom::Hello { .. } => unreachable!(),
            ToRoom::Settings { settings } => {
                if seat != Some(0) {
                    return out.error(conn, "only the host sets the settings");
                }
                if self.game.is_some() {
                    return out.error(conn, "a match is on");
                }
                if !settings.valid() {
                    return out.error(conn, "settings out of range");
                }
                // A change to the match has to be agreed to again (listing the room isn't one).
                if (Settings { public: settings.public, ..self.settings }) != settings {
                    for o in self.seats.iter_mut().flatten().filter(|o| !o.bot) {
                        o.ready = false;
                    }
                }
                self.settings = settings;
                out.all(self.conns(), self.lobby());
            }
            ToRoom::Kick => {
                if seat != Some(0) {
                    return out.error(conn, "only the host removes players");
                }
                if self.game.is_some() {
                    return out.error(conn, "a match is on");
                }
                if self.seats[1].as_ref().is_some_and(|o| o.bot) {
                    self.seats[1] = None;
                    return out.all(self.conns(), self.lobby());
                }
                let Some(kicked) = self.seats[1].as_ref().and_then(|o| o.conn) else {
                    return out.error(conn, "nobody to remove");
                };
                out.to(kicked, FromRoom::Kicked);
                self.left(kicked, out);
            }
            ToRoom::Resign => {
                let Some(seat) = seat else { return out.error(conn, "spectators don't play") };
                let Some(game) = &self.game else { return };
                // Decided already: the winner stopping playing on.
                let outcome = game.decided.unwrap_or(Outcome::Forfeit { seat });
                self.finish(outcome, out);
            }
            ToRoom::Pause { paused } => {
                // Only the host playing its own bot can stop the match: both games are its.
                if self.game.is_none() || seat.is_none() || self.bots_of(conn).is_empty() {
                    return out.error(conn, "only a match against your own bot pauses");
                }
                let others: Vec<ConnId> = self.conns().into_iter().filter(|&c| c != conn).collect();
                out.all(others, FromRoom::Paused { paused });
            }
            ToRoom::AddBot { name } => {
                if seat != Some(0) {
                    return out.error(conn, "only the host adds a bot");
                }
                if self.game.is_some() {
                    return out.error(conn, "a match is on");
                }
                let Some(s) = (0..2).find(|&s| self.seats[s].is_none()) else {
                    return out.error(conn, "no seat free");
                };
                self.next_player = self.next_player.max(1);
                self.seats[s] = Some(Occupant { id: self.next_player, bot: true, conn: Some(conn), name: clean_name(&name), ready: true });
                self.next_player += 1;
                out.all(self.conns(), self.lobby());
            }
            ToRoom::Ready { ready } => {
                let Some(seat) = seat else { return out.error(conn, "spectators don't ready up") };
                if self.game.is_some() {
                    return out.error(conn, "a match is on");
                }
                self.seats[seat as usize].as_mut().unwrap().ready = ready;
                if self.seats.iter().all(|o| o.as_ref().is_some_and(|o| o.ready)) {
                    self.start(out);
                } else {
                    out.all(self.conns(), self.lobby());
                }
            }
            ToRoom::Inputs { frame, buttons, score, over, seat: for_seat } => {
                let Some(seat) = self.playing_seat(conn, for_seat) else { return out.error(conn, "not a seat you play") };
                // Batches still on their way when a match was decided are of no use now.
                let Some(game) = self.game.as_mut() else { return };
                let play = &mut game.plays[seat as usize];
                if play.over || frame as usize != play.inputs.len() || buttons.len() > MAX_BATCH || (buttons.is_empty() && !over) {
                    return out.error(conn, "inputs out of order");
                }
                play.inputs.extend_from_slice(&buttons);
                play.score = score;
                play.over = over;
                let others: Vec<ConnId> = self.conns().into_iter().filter(|&c| c != conn).collect();
                out.all(others, FromRoom::Inputs { seat, frame, buttons, score, over });
                self.settle(out);
            }
            ToRoom::Checksum { frame, hash, seat: for_seat } => {
                let Some(seat) = self.playing_seat(conn, for_seat) else { return out.error(conn, "not a seat you play") };
                if self.game.is_none() {
                    return;
                }
                let others: Vec<ConnId> = self.conns().into_iter().filter(|&c| c != conn).collect();
                out.all(others, FromRoom::Checksum { seat, frame, hash });
            }
            ToRoom::Report { match_id, scores } => {
                let Some(a) = self.awaiting.as_mut() else { return };
                let Some(s) = a.players.iter().position(|&p| p == Some(conn)) else { return };
                if a.match_id != match_id {
                    return;
                }
                a.reports[s] = Some(scores);
                self.confirm_if_reported(out);
            }
        }
    }

    fn hello(&mut self, conn: ConnId, name: &str, watch: bool, out: &mut Out) {
        let Some(i) = self.pending.iter().position(|&c| c == conn) else {
            return out.error(conn, "already said Hello");
        };
        self.pending.remove(i);
        let free = (0..2).find(|&s| self.seats[s].is_none());
        let seat = match free {
            Some(s) if !watch && self.game.is_none() => {
                self.next_player = self.next_player.max(1);
                self.seats[s] = Some(Occupant { id: self.next_player, bot: false, conn: Some(conn), name: clean_name(name), ready: false });
                self.next_player += 1;
                Some(s as Seat)
            }
            _ => {
                self.spectators.push(conn);
                None
            }
        };
        out.to(conn, FromRoom::Welcome { seat });
        out.all(self.conns(), self.lobby());
        // A spectator arriving mid-match: the start, then everything so far.
        if let Some(game) = &self.game {
            out.to(conn, FromRoom::Start { match_id: game.id, settings: self.settings, seeds: game.seeds, countdown_ms: 0 });
            for (s, play) in game.plays.iter().enumerate() {
                if !play.inputs.is_empty() || play.over {
                    out.to(
                        conn,
                        FromRoom::Inputs { seat: s as Seat, frame: 0, buttons: play.inputs.clone(), score: play.score, over: play.over },
                    );
                }
                if play.gone {
                    out.to(conn, FromRoom::Gone { seat: s as Seat });
                }
            }
        }
    }

    fn start(&mut self, out: &mut Out) {
        let first = self.random();
        let seeds = match self.settings.pieces {
            Pieces::Identical => [first, first],
            Pieces::Separate => [first, self.random()],
        };
        let id = self.next_match;
        self.next_match += 1;
        self.game = Some(Match { id, seeds, plays: Default::default(), decided: None });
        self.awaiting = None;
        let start = FromRoom::Start { match_id: id, settings: self.settings, seeds, countdown_ms: crate::COUNTDOWN_MS };
        out.all(self.conns(), start);
        out.all(self.conns(), self.lobby());
    }

    fn left(&mut self, conn: ConnId, out: &mut Out) {
        self.pending.retain(|&c| c != conn);
        self.spectators.retain(|&c| c != conn);
        // Bots go with the host who plays them: between matches at once, during one as a player
        // who left (their board stops).
        for s in self.bots_of(conn) {
            match self.game.as_mut() {
                None => self.seats[s] = None,
                Some(game) => {
                    self.seats[s].as_mut().unwrap().conn = None;
                    game.plays[s].gone = true;
                }
            }
        }
        let Some(seat) = self.seat_of(conn) else {
            out.all(self.conns(), self.lobby());
            return;
        };
        let s = seat as usize;
        let Some(game) = self.game.as_mut() else {
            self.seats[s] = None;
            // A result waiting on this player's report goes without it.
            if let Some(a) = self.awaiting.as_mut() {
                for p in a.players.iter_mut().filter(|p| **p == Some(conn)) {
                    *p = None;
                }
                self.confirm_if_reported(out);
            }
            self.promote(out);
            out.all(self.conns(), self.lobby());
            return;
        };
        // During a match the seat is kept until the match ends.
        self.seats[s].as_mut().unwrap().conn = None;
        game.plays[s].gone = true;
        if self.seats.iter().all(|o| o.as_ref().is_some_and(|o| o.conn.is_none())) {
            let id = game.id;
            self.game = None;
            self.seats = [None, None];
            out.all(self.conns(), FromRoom::Abandoned { match_id: id });
            out.all(self.conns(), self.lobby());
            return;
        }
        out.all(self.conns(), FromRoom::Gone { seat });
        // Leaving before topping out forfeits (there's no coming back), unless the match is
        // decided already (the winner was playing on). After topping out, the score stands and the
        // match goes on: the other player can still pass it.
        let game = self.game.as_ref().unwrap();
        if game.decided.is_none() && !game.plays[s].over {
            return self.finish(Outcome::Forfeit { seat }, out);
        }
        self.settle(out);
    }

    /// Once the match is decided: over, if nobody's still playing (everyone topped out or gone);
    /// otherwise everyone's told, and the winner (a bot too) plays on.
    fn settle(&mut self, out: &mut Out) {
        let game = self.game.as_ref().unwrap();
        let Some(outcome) = game.decided.or_else(|| decide(&game.plays)) else { return };
        if game.plays.iter().all(|p| p.over || p.gone) {
            return self.finish(outcome, out);
        }
        if game.decided.is_none() {
            let match_id = game.id;
            self.game.as_mut().unwrap().decided = Some(outcome);
            out.all(self.conns(), FromRoom::Decided { match_id, outcome });
        }
    }

    fn finish(&mut self, outcome: Outcome, out: &mut Out) {
        let game = self.game.take().expect("a match");
        let scores = game.plays.clone().map(|p| p.score);
        let frames = game.plays.clone().map(|p| p.inputs.len() as u32);
        out.all(self.conns(), FromRoom::Result { match_id: game.id, outcome, scores, frames });
        // Reports are expected from the players (a bot's host reports once, for its own seat).
        let players = [0, 1].map(|s: usize| self.seats[s].as_ref().filter(|o| !o.bot).and_then(|o| o.conn));
        self.awaiting = Some(Awaiting { match_id: game.id, scores, reports: [None, None], players });
        // Players who left lose their seats now; everyone readies up again for a rematch.
        for seat in &mut self.seats {
            if seat.as_ref().is_some_and(|o| o.conn.is_none()) {
                *seat = None;
            }
            if let Some(o) = seat.as_mut().filter(|o| !o.bot) {
                o.ready = false;
            }
        }
        self.promote(out);
        out.all(self.conns(), self.lobby());
    }

    /// With the host's seat empty, the opponent becomes host.
    fn promote(&mut self, out: &mut Out) {
        if self.seats[0].is_none() && self.seats[1].is_some() {
            self.seats[0] = self.seats[1].take();
            if let Some(conn) = self.seats[0].as_ref().unwrap().conn {
                out.to(conn, FromRoom::Welcome { seat: Some(0) });
            }
        }
    }

    fn confirm_if_reported(&mut self, out: &mut Out) {
        let Some(a) = &self.awaiting else { return };
        if (0..2).any(|s| a.players[s].is_some() && a.reports[s].is_none()) {
            return;
        }
        let agreed = a.reports.iter().flatten().all(|r| *r == a.scores);
        let match_id = a.match_id;
        self.awaiting = None;
        out.all(self.conns(), FromRoom::Confirmed { match_id, agreed });
    }

    fn random(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 16) as u32
    }
}

/// The match's outcome once it's decided, CTWC style: when one player has topped out, the other
/// wins as soon as their score passes; when both have, the higher score wins.
fn decide(plays: &[Play; 2]) -> Option<Outcome> {
    let [a, b] = plays;
    match (a.over, b.over) {
        (true, true) => Some(match a.score.cmp(&b.score) {
            std::cmp::Ordering::Greater => Outcome::Win { seat: 0 },
            std::cmp::Ordering::Less => Outcome::Win { seat: 1 },
            std::cmp::Ordering::Equal => Outcome::Draw,
        }),
        (true, false) if b.score > a.score => Some(Outcome::Win { seat: 1 }),
        (false, true) if a.score > b.score => Some(Outcome::Win { seat: 0 }),
        _ => None,
    }
}

struct Out(Vec<Send>);

impl Out {
    fn to(&mut self, to: ConnId, msg: FromRoom) {
        self.0.push(Send { to, msg });
    }

    fn all(&mut self, to: Vec<ConnId>, msg: FromRoom) {
        for c in to {
            self.to(c, msg.clone());
        }
    }

    fn error(&mut self, to: ConnId, message: &str) {
        self.to(to, FromRoom::Error { message: message.to_string() });
    }
}
