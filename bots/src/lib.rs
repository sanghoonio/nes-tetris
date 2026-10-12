//! Computer opponents. A bot is a player at a controller: each frame it looks at the game (its
//! RAM, as `Vars`: the playfield, the falling piece, the next one) and says which buttons are
//! down. It never changes the game.
//!
//! When a piece appears it chooses where the piece should rest: every place the piece can reach
//! (`search`) is scored by the board it would leave (`eval`), optionally with the best place for
//! the next piece after it; then it works the controller towards that place (`Bot::buttons`):
//! rotating, moving by tapping or by holding (the game's DAS), then dropping.
//!
//! How strong a bot is comes from its `Profile`: how fast it taps, how long it takes to react,
//! how much noise is in its judgement, and whether it looks ahead.

pub mod eval;
pub mod search;
pub mod shapes;

use std::sync::Arc;

use tetris_rs::options::Rules;
use tetris_rs::{buttons, Rom, Vars};

pub use eval::Weights;
use search::{Board, Placement};
pub use shapes::Shapes;

/// How a bot plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    /// Frames between taps of Left or Right; none to hold the direction instead (the game's DAS).
    /// 4 is 15 taps a second, the most a bot does.
    pub tap_frames: Option<u32>,
    /// Frames after a piece appears before the bot touches it.
    pub reaction: u32,
    /// Spread of the random error added to each place's score, as a share of the scores' spread:
    /// 0 plays its best.
    pub noise: f64,
    /// Choose with the next piece in mind (when the next piece is shown).
    pub look_ahead: bool,
    pub weights: Weights,
}

impl Profile {
    /// The strongest: 15 taps a second, quick, no mistakes, looking ahead.
    pub fn best() -> Profile {
        ROSTER[ROSTER.len() - 1].1
    }
}

/// A piece being worked: where it's going, and how it's getting there.
struct Plan {
    target: Placement,
    /// Frames since the piece appeared.
    age: u32,
    /// Frames since Left or Right was last pressed.
    since_tap: u32,
}

pub struct Bot {
    profile: Profile,
    shapes: Arc<Shapes>,
    rng: u64,
    /// The spawn count when the piece being worked appeared.
    spawned: Option<u8>,
    plan: Option<Plan>,
    /// Last frame's buttons (the game acts on presses, so a button must be let go between them).
    last: u8,
}

impl Bot {
    pub fn new(rom: &Rom, profile: Profile, seed: u64) -> Bot {
        Bot { profile, shapes: Arc::new(Shapes::from_rom(rom)), rng: seed | 1, spawned: None, plan: None, last: 0 }
    }

    /// This frame's buttons, from the game as it stands (`ram`, after last frame) and the rules
    /// it's played under.
    pub fn buttons(&mut self, ram: &[u8; 0x800], rules: &Rules) -> u8 {
        let b = self.decide(ram, rules);
        self.last = b;
        b
    }

    fn decide(&mut self, ram: &[u8; 0x800], rules: &Rules) -> u8 {
        let vars = Vars::from_ram(ram);
        let p = &vars.player1;
        // Only while a piece is falling under control.
        if p.play_state != 1 || p.current_piece as usize >= shapes::ORIENTATIONS {
            self.plan = None;
            return 0;
        }
        let board = Board::from_ram(ram);
        if self.spawned != Some(vars.spawn_count) {
            self.spawned = Some(vars.spawn_count);
            let next = (vars.display_next_piece == 0).then_some(vars.next_piece).filter(|_| self.profile.look_ahead);
            let target = self.choose(&board, p.current_piece, p.tetrimino_x, p.tetrimino_y, next);
            self.plan = target.map(|target| Plan { target, age: 0, since_tap: u32::MAX / 2 });
        }
        let Some(plan) = self.plan.as_mut() else { return 0 };
        plan.age += 1;
        plan.since_tap += 1;
        if plan.age <= self.profile.reaction {
            return 0;
        }
        let (o, x) = (p.current_piece, p.tetrimino_x);
        let mut out = 0;
        // Turn towards the target orientation: a fresh press of A (clockwise) or B each time.
        if o != plan.target.orientation {
            let cw = self.shapes.turns(o, plan.target.orientation);
            let button = if cw { buttons::A } else { buttons::B };
            if self.last & button == 0 {
                out |= button;
            }
        }
        // Move towards the target column.
        if x != plan.target.x {
            let dir = if plan.target.x < x { buttons::LEFT } else { buttons::RIGHT };
            match self.profile.tap_frames {
                None => out |= dir,
                Some(every) => {
                    if self.last & dir == 0 && plan.since_tap >= every {
                        out |= dir;
                        plan.since_tap = 0;
                    }
                }
            }
        }
        // In place: drop (holding Down would block moving, so only once there).
        if o == plan.target.orientation && x == plan.target.x {
            if rules.hard_drop {
                if self.last & buttons::UP == 0 {
                    out |= buttons::UP;
                }
            } else {
                out |= buttons::DOWN;
            }
        }
        out
    }

    /// The best place for the piece (orientation `o` at `x`, `y`) on `board`: with `next`, by the
    /// best place for the next piece after it.
    fn choose(&mut self, board: &Board, o: u8, x: u8, y: u8, next: Option<u8>) -> Option<Placement> {
        let w = &self.profile.weights;
        let places = search::placements(&self.shapes, board, o, x, y);
        let mut scored: Vec<(f64, Placement)> = places
            .into_iter()
            .map(|place| {
                let (after, lines) = board.with(&self.shapes, &place);
                let mut score = w.score(&after, lines, place.y);
                if let Some(next) = next {
                    let spawn = self.shapes.spawn(next);
                    let best = search::placements(&self.shapes, &after, spawn.0, spawn.1, spawn.2)
                        .into_iter()
                        .map(|p2| {
                            let (after2, lines2) = after.with(&self.shapes, &p2);
                            w.score(&after2, lines2, p2.y)
                        })
                        .fold(f64::NEG_INFINITY, f64::max);
                    // Nowhere for the next piece: this place ends the game.
                    score += if best.is_finite() { best } else { -1e9 };
                }
                (score, place)
            })
            .collect();
        if self.profile.noise > 0.0 && scored.len() > 1 {
            let (lo, hi) = scored.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), s| (lo.min(s.0), hi.max(s.0)));
            let spread = (hi - lo).max(1.0);
            for s in &mut scored {
                s.0 += self.gaussian() * self.profile.noise * spread;
            }
        }
        scored.into_iter().max_by(|a, b| a.0.total_cmp(&b.0)).map(|s| s.1)
    }

    /// A standard normal number (Box-Muller over xorshift).
    fn gaussian(&mut self) -> f64 {
        let mut uniform = || {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            ((self.rng >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        let (u, v) = (uniform(), uniform());
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

/// The bots, weakest to strongest (named after the made-up players of Fighting Baseball for the
/// Super Famicom): one set of weights, held back by speed, reaction, mistakes and look-ahead.
pub const ROSTER: [(&str, Profile); 5] = [
    ("SLEVE", Profile { tap_frames: None, reaction: 20, noise: 0.30, look_ahead: false, weights: Weights::DEFAULT }),
    ("BOBSON", Profile { tap_frames: Some(8), reaction: 12, noise: 0.12, look_ahead: false, weights: Weights::DEFAULT }),
    ("JEROMY", Profile { tap_frames: Some(6), reaction: 7, noise: 0.04, look_ahead: true, weights: Weights::DEFAULT }),
    ("KEVIN", Profile { tap_frames: Some(5), reaction: 4, noise: 0.01, look_ahead: true, weights: Weights::DEFAULT }),
    ("ANATOLI", Profile { tap_frames: Some(4), reaction: 2, noise: 0.0, look_ahead: true, weights: Weights::DEFAULT }),
];

/// A bot on the roster by name.
pub fn profile(name: &str) -> Option<Profile> {
    ROSTER.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
}
