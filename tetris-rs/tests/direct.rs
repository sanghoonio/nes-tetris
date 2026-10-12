//! Starting a game directly (`Runner::start_game`) against reaching it through the menus.
//!
//! A run goes through the original's menus, waiting a varied while on each screen, until play
//! begins. A second game is started directly with what the menus chose and the seed they left,
//! and both get the same input from then on. Every frame until game over, the picture, video
//! memory and variables must match, except the variables the menus leave behind
//! (`MENU_LEFTOVERS`, `is_sound_engine`). Then the run carries on through the menus to its next game.
//!
//!     cargo test -p tetris-rs --test direct -- --nocapture

use recomp::machine::{BTN_A, BTN_LEFT, BTN_RIGHT, BTN_START};
use recomp::scripts::{self, Script};
use std::sync::Arc;
use tetris_rs::options::Rules;
use tetris_rs::vars::{is_sound_engine, Player};
use tetris_rs::{buttons, FrameOut, GameSetup, Pieces, Rom, Runner, Seed, Vars};

/// Left by the menus and not set by a direct start; none of them feeds play.
const MENU_LEFTOVERS: &[&str] = &[
    // Scratch the menus' last frame left (the routines that read them set them first).
    "tmp1",
    "tmp2",
    "tmp3",
    "general_counter2",
    "sprite_x_offset",
    "sprite_y_offset",
    "tmp_bulk_copy_to_ppu_return_addr",
    // The level menu's copy of the buttons pressed, which play sets before reading.
    "newly_pressed_buttons",
    // Whether the level menu's cursor was on the level or the height grid.
    "selecting_level_or_height",
    "original_y",
    // The level menu keeps drawing garbage holes; only two-player games read them.
    "player1.garbage_hole",
    "player2.garbage_hole",
    "active.garbage_hole",
];

/// What the original carries from one game into the next and a direct start begins without, as
/// the first game after power-on does. For games after the first, these are copied into the direct
/// game as play begins, so that everything else can be compared. Two of them matter: a Left or
/// Right held as the last game ended is already part-charged (`autorepeat_x`), and soft-drop points
/// counted then can be added to this game's first piece (`hold_down_points`).
const CARRIED_OVER: &[&str] = &[
    "active.autorepeat_x",
    "player1.autorepeat_x",
    "active.hold_down_points",
    "player1.hold_down_points",
    "active.line_index",
    "player1.line_index",
    "active.curtain_row",
    "player1.curtain_row",
    "frame_counter",
    "high_score_entry_raw_pos",
];

/// Where a variable (or `player.field`) is in RAM.
fn ram_range(name: &str) -> std::ops::Range<usize> {
    let (var, field) = name.split_once('.').map_or((name, None), |(v, f)| (v, Some(f)));
    let &(_, addr, size) = Vars::LAYOUT.iter().find(|v| v.0 == var).expect("a variable");
    match field {
        None => addr..addr + size,
        Some(field) => {
            let &(_, off, fsize) = Player::LAYOUT.iter().find(|f| f.0 == field).expect("a field");
            addr + off..addr + off + fsize
        }
    }
}

fn rom() -> Arc<Rom> {
    let path = std::env::var_os("TETRIS_ROM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"));
    let file = std::fs::read(&path).unwrap_or_else(|e| panic!("ROM {}: {e}", path.display()));
    Arc::new(Rom::new(&file).unwrap())
}

/// One step of the game's random number generator (`rng.rs`).
fn lfsr(s: u16) -> u16 {
    ((s >> 1) ^ (s >> 9)) << 15 | s >> 1
}

fn vars(out: &FrameOut) -> Vars {
    Vars::from_ram(&out.ram)
}

/// Named variables that differ, skipping the menus' leftovers.
fn var_diffs(a: &[u8; 0x800], b: &[u8; 0x800]) -> Vec<String> {
    let mut out = Vec::new();
    for &(name, addr, size) in Vars::LAYOUT {
        if MENU_LEFTOVERS.contains(&name) || is_sound_engine(name) {
            continue;
        }
        if ["active", "player1", "player2"].contains(&name) {
            for &(field, off, fsize) in Player::LAYOUT {
                let r = addr + off..addr + off + fsize;
                let full = format!("{name}.{field}");
                if a[r.clone()] != b[r.clone()] && !MENU_LEFTOVERS.contains(&full.as_str()) {
                    out.push(format!("{full}: menus {:02X?} direct {:02X?}", &a[r.clone()], &b[r]));
                }
            }
            continue;
        }
        let r = addr..addr + size;
        if a[r.clone()] != b[r.clone()] {
            out.push(format!("{name}: menus {:02X?} direct {:02X?}", &a[r.clone()], &b[r]));
        }
    }
    out
}

struct Xorshift(u64);
impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// The menus, as a player would go through them: a varied wait on each screen, then the game type,
/// music, level (10 and up with A+Start) and height, chosen afresh for every game. `play` drives
/// the games.
fn player(seed: u64, mut play: Script) -> Script {
    let mut rng = Xorshift(seed);
    let mut wait = 0u64;
    let mut want = (false, 0u8, 0u8, 0u8); // B-type, music, level, height
    let mut last = 0u8;
    let mut last_mode = 0xFF;
    Box::new(move |f, ram| {
        let v = Vars::from_ram(ram);
        if v.game_mode == 4 {
            last_mode = 4;
            return play(f, ram);
        }
        if v.game_mode != last_mode {
            last_mode = v.game_mode;
            wait = rng.next() % 90;
            if v.game_mode == 2 {
                want = (rng.next().is_multiple_of(3), (rng.next() % 4) as u8, (rng.next() % 20) as u8, (rng.next() % 6) as u8);
            } else if v.game_mode == 3 {
                want.2 = (rng.next() % 20) as u8;
                want.3 = (rng.next() % 6) as u8;
            }
        }
        // Release between presses.
        let press = |b: u8, last: &mut u8| {
            let b = if *last != 0 && b & !*last == 0 { 0 } else { b };
            *last = b;
            (b, vec![])
        };
        if wait > 0 {
            wait -= 1;
            return press(0, &mut last);
        }
        let (b_type, music, level, height) = want;
        let b = match v.game_mode {
            1 => BTN_START,
            2 if (v.game_type != 0) != b_type => if b_type { BTN_RIGHT } else { BTN_LEFT },
            2 if v.music_type != music => if v.music_type < music { buttons::DOWN } else { buttons::UP },
            2 => BTN_START,
            3 => {
                let on_height = v.selecting_level_or_height != 0;
                let (now, target) = if on_height { (v.active.start_height, height) } else { (v.active.start_level, level % 10) };
                if now != target {
                    if now < target { BTN_RIGHT } else { BTN_LEFT }
                } else if b_type && !on_height {
                    BTN_A
                } else if level >= 10 {
                    // A held, then Start.
                    if last == BTN_A { BTN_A | BTN_START } else { BTN_A }
                } else {
                    BTN_START
                }
            }
            _ => 0,
        };
        press(b, &mut last)
    })
}

/// Games of a run through the menus (`player(seed, play)`), each compared with the same game started
/// directly, up to `max_frames` of play each. Returns each game's setup and whether it ended.
fn compare(name: &str, seed: u64, play: Script, games: usize, max_frames: usize) -> Vec<(GameSetup, bool)> {
    let rom = rom();
    let mut script = player(seed, play);
    let mut menus = Runner::new(rom.clone(), None);
    let mut ram = [0u8; 0x800];
    let mut f = 0;
    let mut pressed = vec![];
    let mut results = vec![];
    let mut next = |menus: &mut Runner, ram: &mut [u8; 0x800], pressed: &mut Vec<u8>| {
        let (b, pokes) = script(f, ram);
        f += 1;
        pressed.push(b);
        let out = menus.next_frame_with(b, pokes.clone());
        *ram = out.ram;
        (out, b, pokes)
    };
    for game in 0..games {
        let name = format!("{name} game {game}");
        // Through the menus, to the first frame of play.
        let mut steps = 0;
        let first = loop {
            let was = Vars::from_ram(&ram).game_mode;
            let (out, ..) = next(&mut menus, &mut ram, &mut pressed);
            if vars(&out).game_mode == 4 && was != 4 {
                break out;
            }
            steps += 1;
            assert!(steps < 20000, "{name}: no game");
        };
        let v = vars(&first);
        let setup = |seed| GameSetup {
            b_type: v.game_type != 0,
            level: v.player1.start_level,
            height: v.player1.start_height,
            music: v.music_type,
            seed,
            pieces: Pieces::Original,
            rules: Rules::ORIGINAL,
            next_piece: v.display_next_piece == 0,
        };

        // How many frames a direct start runs before its first frame of play: the seed the menus
        // left as play began is that many generator steps before what this frame shows.
        let mut probe = Runner::start_game(rom.clone(), None, setup(Seed { rng: 0x1234, spawn_count: v.spawn_count, frame: 0 }));
        let mut lead_in = 0;
        let ran = loop {
            let out = probe.next_frame(0);
            lead_in += 1;
            if vars(&out).game_mode == 4 {
                break vars(&out).frame_counter.0 as u8;
            }
        };
        let rng = (1..=u16::MAX).find(|&s| (0..ran).fold(s, |s, _| lfsr(s)) == v.rng_seed.0).expect("a seed");
        let seed = Seed { rng, spawn_count: v.spawn_count, frame: (v.frame_counter.0 as u8).wrapping_sub(ran) };
        let mut direct = Runner::start_game(rom.clone(), None, setup(seed));
        // The menus' last buttons, so that the press that started the game is held as play begins.
        let mut out = None;
        for i in 0..lead_in {
            let back = lead_in - i;
            out = Some(direct.next_frame(if back <= pressed.len() { pressed[pressed.len() - back] } else { 0 }));
        }
        let mut out = out.unwrap();
        assert_eq!(vars(&out).game_mode, 4);

        // Play, to game over.
        let mut carry: Option<Vec<(u16, u8)>> = None;
        let mut frames = 0;
        let mut from_menus = first;
        let ended = loop {
            let mut diffs = var_diffs(&from_menus.ram, &out.ram);
            let picture = from_menus.pixels[8 * 256..232 * 256] == out.pixels[8 * 256..232 * 256];
            let video = from_menus.vram == out.vram && from_menus.palette == out.palette && from_menus.oam == out.oam;
            // Until the game screen is drawn (state 0), the menus' run still holds the level menu's
            // screen and palette with rendering off, and the direct start a blank one.
            // So are the level menu's cursor sprites, staged but not shown.
            let drawn = vars(&from_menus).game_mode_state != 0;
            if !drawn {
                diffs.retain(|d| !d.starts_with("oam_staging"));
            }
            // After the first game, RAM still holds the last one until this game's setup (states
            // 0 and 1) has overwritten it.
            if game > 0 && vars(&from_menus).game_mode_state < 2 {
                diffs.clear();
            }
            if game > 0 && carry.is_none() && vars(&from_menus).game_mode_state >= 1 {
                diffs.retain(|d| !CARRIED_OVER.iter().any(|c| d.starts_with(&format!("{c}:"))));
                let ram = from_menus.ram;
                carry = Some(CARRIED_OVER.iter().flat_map(|c| ram_range(c).map(move |a| (a as u16, ram[a]))).collect());
            }
            assert!(diffs.is_empty() && (!drawn || picture && video), "{name}, play frame {frames}: picture {picture}, video {video}, {diffs:#?}");
            let m = vars(&from_menus);
            let a_type_over = m.player1.play_state == 10 && m.player1.curtain_row == 0x14;
            // B-type's goal: "SUCCESS" is up and the menus' run is on to the ending.
            let b_type_over = m.game_type != 0 && m.render_mode == 0;
            if a_type_over || b_type_over || frames == max_frames {
                break a_type_over || b_type_over;
            }
            let (o, b, mut pokes) = next(&mut menus, &mut ram, &mut pressed);
            from_menus = o;
            if let Some(c) = carry.as_mut() {
                pokes.append(c);
            }
            out = direct.next_frame_with(b, pokes);
            frames += 1;
        };
        if ended {
            // The original now waits for Start or plays an ending; the direct game has ended.
            let over = (0..30).any(|_| direct.next_frame(0).game_over);
            assert!(over, "{name}: no game over from the direct start");
        }
        println!("{name}: {:?}, {frames} frames, ended {ended}", setup(seed));
        results.push((setup(seed), ended));
    }
    results
}

#[test]
fn random_play() {
    let mut all = vec![];
    for seed in 1..=8 {
        all.extend(compare(&format!("random-{seed}"), seed, scripts::random_play(seed), 3, 20000));
    }
    assert!(all.iter().all(|(_, ended)| *ended));
    assert!(all.iter().any(|(s, _)| s.b_type) && all.iter().any(|(s, _)| !s.b_type), "both game types");
    assert!(all.iter().any(|(s, _)| s.level >= 10), "a level from A+Start");
    assert!(all.iter().any(|(s, _)| s.seed.spawn_count != 0), "a game after the first");
    let mut seeds: Vec<_> = all.iter().map(|(s, _)| s.seed).collect();
    seeds.dedup();
    assert!(seeds.len() > all.len() / 2, "varied seeds");
}

/// A Tetris per piece from level 18: line clears, level-ups, and play past level 29.
#[test]
fn line_clears() {
    compare("line-clears", 99, scripts::line_clears(3), 1, 15000);
}

/// A direct game of `pieces`, played by `play`: its first `n` pieces.
fn pieces(pieces: Pieces, mut play: Script, n: usize) -> Vec<u8> {
    let setup = GameSetup {
        b_type: false,
        level: 5,
        height: 0,
        music: 3,
        seed: Seed::from_u32(0xBEEF_2A11),
        pieces,
        rules: Rules::ORIGINAL,
        next_piece: true,
    };
    let mut game = Runner::start_game(rom(), None, setup);
    let mut ram = [0u8; 0x800];
    let mut dealt = vec![];
    let mut last_count = None;
    for f in 0..40000 {
        let (b, pokes) = play(f, &ram);
        let out = game.next_frame_with(b, pokes);
        ram = out.ram;
        let v = vars(&out);
        if v.game_mode == 4 && v.game_mode_state != 0 && last_count != Some(v.spawn_count) {
            last_count = Some(v.spawn_count);
            dealt.push(v.next_piece);
        }
        if dealt.len() == n || out.game_over {
            break;
        }
    }
    dealt
}

/// Play that never tops out: Down tapped (or not), and the playfield emptied every frame.
fn endless(drop: bool) -> Script {
    Box::new(move |f, _| (if drop && f % 4 < 2 { buttons::DOWN } else { 0 }, (0x400..0x4C8).map(|a| (a, 0xEF)).collect()))
}

#[test]
fn identical_pieces_ignore_timing() {
    // Dropping fast and letting pieces fall deal at different times.
    let a = pieces(Pieces::Identical, endless(true), 50);
    let b = pieces(Pieces::Identical, endless(false), 50);
    assert_eq!(a.len(), 50);
    assert_eq!(a, b);
    // The original's deal depends on timing.
    let a = pieces(Pieces::Original, endless(true), 50);
    let b = pieces(Pieces::Original, endless(false), 50);
    assert_ne!(a, b);
}

#[test]
fn start_does_not_pause() {
    let setup = GameSetup {
        b_type: false,
        level: 0,
        height: 0,
        music: 0,
        seed: Seed::from_u32(7),
        pieces: Pieces::Original,
        rules: Rules::ORIGINAL,
        next_piece: true,
    };
    let mut game = Runner::start_game(rom(), None, setup);
    for f in 0..300 {
        let out = game.next_frame(if f % 20 == 10 { buttons::START } else { 0 });
        let v = vars(&out);
        if f > 50 {
            assert_eq!(v.render_mode, 3, "paused at frame {f}");
        }
    }
}
