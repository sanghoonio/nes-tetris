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
//! how often it misjudges a piece, whether it looks ahead, and how it drops.

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
    /// The chance, each piece, of a mistake: taking the second-best place instead of the best
    /// (now and then the third), as a person misjudges, not at random. 0 plays its best.
    pub mistakes: f64,
    /// Choose with the next piece in mind (when the next piece is shown).
    pub look_ahead: bool,
    /// What it does once the piece is in place.
    pub drop: Drop,
    pub weights: Weights,
}

/// Once a piece is turned and moved where it's going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    /// Lets it fall.
    Gravity,
    /// Lets it fall this many frames, then holds Down.
    After(u32),
    /// Holds Down (or hard drops, where the room allows) at once.
    Eager,
}

impl Profile {
    /// The strongest: 15 taps a second, quick, no mistakes, looking ahead.
    pub fn best() -> Profile {
        ROSTER[ROSTER.len() - 1].1
    }
}

/// A piece being worked: where it's going, and how it's getting there.
struct Plan {
    /// Chosen once the bot reacts, from where the piece is by then.
    target: Option<Placement>,
    /// Frames the piece has stayed put (neither turned nor moved) short of its target, and how
    /// often it's been replanned from there.
    stuck: u32,
    replans: u32,
    was: (u8, u8),
    /// Frames since the piece appeared.
    age: u32,
    /// Frames since Left or Right was last pressed.
    since_tap: u32,
    /// Frames since the piece was in place.
    placed: u32,
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
        if self.spawned != Some(vars.spawn_count) {
            self.spawned = Some(vars.spawn_count);
            self.plan = Some(Plan { target: None, stuck: 0, replans: 0, was: (0xFF, 0xFF), age: 0, since_tap: u32::MAX / 2, placed: 0 });
        }
        let (o, x, y) = (p.current_piece, p.tetrimino_x, p.tetrimino_y);
        {
            let Some(plan) = self.plan.as_mut() else { return 0 };
            plan.age += 1;
            plan.since_tap += 1;
            if plan.age <= self.profile.reaction {
                return 0;
            }
            // Short of the target and not getting closer (the way's blocked lower down): look
            // again from here, a few times at most.
            let short = plan.target.is_some_and(|t| (t.orientation, t.x) != (o, x));
            plan.stuck = if short && plan.was == (o, x) { plan.stuck + 1 } else { 0 };
            plan.was = (o, x);
            if plan.stuck > 20 && plan.replans < 3 {
                plan.target = None;
                plan.replans += 1;
                plan.stuck = 0;
            }
        }
        // Reacting: choose where it goes, from where it is now.
        if self.plan.as_ref().is_some_and(|plan| plan.target.is_none()) {
            let board = Board::from_ram(ram);
            let next = (vars.display_next_piece == 0).then_some(vars.next_piece).filter(|_| self.profile.look_ahead);
            let target = self.choose(&board, o, x, y, next);
            match (self.plan.as_mut(), target) {
                (Some(plan), Some(target)) => plan.target = Some(target),
                _ => {
                    self.plan = None;
                    return 0;
                }
            }
        }
        let Some(plan) = self.plan.as_mut() else { return 0 };
        let target = plan.target.unwrap();
        let mut out = 0;
        // Turn towards the target orientation: a fresh press of A (clockwise) or B each time.
        if o != target.orientation {
            let cw = self.shapes.turns(o, target.orientation);
            let button = if cw { buttons::A } else { buttons::B };
            if self.last & button == 0 {
                out |= button;
            }
        }
        // Move towards the target column.
        if x != target.x {
            let dir = if target.x < x { buttons::LEFT } else { buttons::RIGHT };
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
        // In place: drop as the bot does (holding Down would block moving, so only once there).
        if o == target.orientation && x == target.x {
            plan.placed += 1;
            let now = match self.profile.drop {
                Drop::Gravity => false,
                Drop::After(n) => plan.placed > n,
                Drop::Eager => true,
            };
            if now && rules.hard_drop && self.profile.drop == Drop::Eager {
                if self.last & buttons::UP == 0 {
                    out |= buttons::UP;
                }
            } else if now {
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
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        // A mistake: the second best, or now and then the third.
        let pick = if scored.len() > 1 && self.uniform() < self.profile.mistakes {
            if scored.len() > 2 && self.uniform() < 0.25 { 2 } else { 1 }
        } else {
            0
        };
        scored.get(pick).map(|s| s.1)
    }

    /// A number in 0..1 (xorshift).
    fn uniform(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The bots, weakest to strongest (named after the made-up players of Fighting Baseball for the
/// Super Famicom): one set of tuned weights, played slower, later, more carelessly and with less
/// foresight down the roster. Only the strongest taps at the top human rate.
pub const ROSTER: [(&str, Profile); 5] = [
    ("SLEVE", Profile { tap_frames: None, reaction: 24, mistakes: 0.30, look_ahead: false, drop: Drop::After(60), weights: Weights::TUNED }),
    ("BOBSON", Profile { tap_frames: Some(10), reaction: 16, mistakes: 0.18, look_ahead: false, drop: Drop::After(30), weights: Weights::TUNED }),
    ("JEROMY", Profile { tap_frames: Some(8), reaction: 10, mistakes: 0.08, look_ahead: true, drop: Drop::After(12), weights: Weights::TUNED }),
    ("KEVIN", Profile { tap_frames: Some(6), reaction: 6, mistakes: 0.03, look_ahead: true, drop: Drop::After(4), weights: Weights::TUNED }),
    ("ANATOLI", Profile { tap_frames: Some(4), reaction: 3, mistakes: 0.0, look_ahead: true, drop: Drop::Eager, weights: Weights::TUNED }),
];

/// A bot on the roster by name.
pub fn profile(name: &str) -> Option<Profile> {
    ROSTER.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
}
