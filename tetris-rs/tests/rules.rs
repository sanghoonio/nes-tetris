//! The rule changes, each against what it should do. These diverge from the original on purpose,
//! so they're checked by behaviour rather than against the recompile.

use std::sync::Arc;
use tetris_rs::options::{LeftRight, Rules};
use tetris_rs::{buttons, Config, FrameOut, Keys, Options, Rom, Runner, Vars};

/// A game of A-type, level 0, with `rules`, at the first frame the first piece is under
/// control. The menus are passed with Enter; the rules are then driven with NES buttons (the
/// buttons the game keys stand for).
fn game(rules: Rules) -> (Runner, Vars) {
    let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tetris.nes")).unwrap();
    let options = Options { menus: true, exit: true, config: Config { rules, ..Config::default() } };
    let mut game = Runner::with_options(Arc::new(Rom::new(&rom).unwrap()), None, None, options);
    for f in 0..2000 {
        let enter = [10, 40, 70, 100].contains(&f);
        let key = |on: bool| if on { vec!["Enter".to_string()] } else { vec![] };
        let v = Vars::from_ram(&game.next_frame_keys(Keys { held: key(enter), pressed: key(enter), ..Default::default() }).ram);
        if f > 100 && v.game_mode == 4 && v.player1.play_state == 1 && v.player1.current_piece < 0x13 {
            return (game, v);
        }
    }
    panic!("no game");
}

fn vars(out: &FrameOut) -> Vars {
    Vars::from_ram(&out.ram)
}

/// Frames (from the first press) at which the piece moved, holding `held`.
fn moves(rules: Rules, held: u8, frames: usize) -> Vec<usize> {
    let (mut g, v) = game(rules);
    let mut x = v.player1.tetrimino_x;
    let mut at = vec![];
    for f in 0..frames {
        let now = vars(&g.next_frame(held)).player1.tetrimino_x;
        if now != x {
            at.push(f);
            x = now;
        }
    }
    at
}

#[test]
fn das() {
    // The original: a move at once, then after 16 frames, then every 6.
    assert_eq!(moves(Rules::ORIGINAL, buttons::LEFT, 30)[..3], [0, 16, 22]);
    let fast = Rules { das_delay: 10, das_repeat: 2, ..Rules::ORIGINAL };
    assert_eq!(moves(fast, buttons::LEFT, 30)[..3], [0, 10, 12]);
}

#[test]
fn left_and_right_together() {
    let both = buttons::LEFT | buttons::RIGHT;
    // Original: Right wins (it's checked first).
    let (mut g, v) = game(Rules::ORIGINAL);
    let x = v.player1.tetrimino_x;
    assert!(vars(&g.next_frame(both)).player1.tetrimino_x > x);
    // Neither: no move.
    assert!(moves(Rules { left_right: LeftRight::Neither, ..Rules::ORIGINAL }, both, 40).is_empty());
    // Last: holding Left, then adding Right, goes right.
    let (mut g, v) = game(Rules { left_right: LeftRight::Last, ..Rules::ORIGINAL });
    let x = v.player1.tetrimino_x;
    let after_left = vars(&g.next_frame(buttons::LEFT)).player1.tetrimino_x;
    assert_eq!(after_left, x - 1);
    assert_eq!(vars(&g.next_frame(both)).player1.tetrimino_x, x);
}

#[test]
fn slowdown() {
    // Holding Down+Left+Right: the piece falls at half speed in the original.
    let fallen = |rules| {
        let (mut g, v) = game(rules);
        let y = v.player1.tetrimino_y;
        let mut out = None;
        for _ in 0..240 {
            out = Some(g.next_frame(buttons::DOWN | buttons::LEFT | buttons::RIGHT));
        }
        vars(&out.unwrap()).player1.tetrimino_y - y
    };
    let original = fallen(Rules::ORIGINAL);
    let fixed = fallen(Rules { slowdown: false, ..Rules::ORIGINAL });
    assert!(fixed >= 2 * original - 1 && fixed > original, "fell {original} rows originally, {fixed} without the slowdown");
}

#[test]
fn hard_drop() {
    // Off: Up does nothing.
    let (mut g, v) = game(Rules::ORIGINAL);
    let after = vars(&g.next_frame(buttons::UP));
    assert_eq!((after.player1.tetrimino_y, after.player1.play_state), (v.player1.tetrimino_y, 1));
    // On: the piece drops to the floor and locks, scoring the rows like a soft drop.
    let (mut g, v) = game(Rules { hard_drop: true, ..Rules::ORIGINAL });
    let after = vars(&g.next_frame(buttons::UP));
    assert_eq!(after.player1.play_state, 2, "locks");
    assert!(after.player1.tetrimino_y >= 17, "at the floor: row {}", after.player1.tetrimino_y);
    let rows = after.player1.tetrimino_y - v.player1.tetrimino_y;
    let mut out = after;
    for _ in 0..10 {
        out = vars(&g.next_frame(0));
    }
    let score = out.player1.score.0;
    assert!(score != [0, 0, 0], "{rows} rows dropped scored nothing");
}

#[test]
fn ghost() {
    let sprites = |out: &FrameOut| out.oam.chunks(4).filter(|s| s[0] < 0xEF).count();
    // From the second frame of the piece (OAM shows the previous frame's staging).
    let (mut g, _) = game(Rules::ORIGINAL);
    g.next_frame(0);
    let plain: Vec<usize> = (0..4).map(|_| sprites(&g.next_frame(0))).collect();
    let (mut g, _) = game(Rules { ghost: true, ..Rules::ORIGINAL });
    g.next_frame(0);
    let ghosted: Vec<usize> = (0..4).map(|_| sprites(&g.next_frame(0))).collect();
    // Four more sprites on alternate frames.
    let extra: Vec<usize> = ghosted.iter().zip(&plain).map(|(g, p)| g - p).collect();
    assert!(extra == [4, 0, 4, 0] || extra == [0, 4, 0, 4], "{plain:?} vs {ghosted:?}");
}
