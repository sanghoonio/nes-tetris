//! A bare client of the game library, the way an online client uses it: the ROM is read at
//! runtime (nothing of it is built in), and one game is started directly, with no legal screen,
//! title or menus. It prints the result at game over and closes.
//!
//!     cargo run --release -p probe -- tetris.nes [--type a|b] [--level 0-19] [--height 0-5]
//!         [--music 0-3] [--seed N] [--pieces original|identical]
//!
//! Keys: arrows move and soft-drop, X and Z rotate, Escape quits.

use std::sync::Arc;
use tetris_rs::options::Rules;
use tetris_rs::{buttons, GameSetup, Pieces, Rom, Runner, Seed, Vars};

/// Frames the final screen stays up after game over.
const LINGER: u32 = 180;

struct Probe {
    runner: Runner,
    over: Option<u32>,
    quit: bool,
}

impl frontend::Game for Probe {
    fn frame(&mut self, input: &frontend::Input) -> (Box<[u8; 256 * 240]>, Vec<f32>) {
        let held = |k: &str| input.held.iter().any(|h| h == k);
        let mut b = 0;
        for (key, button) in [
            ("ArrowLeft", buttons::LEFT),
            ("ArrowRight", buttons::RIGHT),
            ("ArrowDown", buttons::DOWN),
            ("KeyX", buttons::A),
            ("KeyZ", buttons::B),
        ] {
            if held(key) {
                b |= button;
            }
        }
        let out = self.runner.next_frame(b);
        if out.game_over && self.over.is_none() {
            let v = Vars::from_ram(&out.ram);
            println!(
                "game over: score {}, lines {}, level {}",
                bcd(&v.player1.score.0),
                bcd(&v.player1.lines.0),
                v.player1.level_number
            );
            self.over = Some(0);
        }
        if let Some(n) = self.over.as_mut() {
            *n += 1;
            self.quit |= *n >= LINGER;
        }
        self.quit |= held("Escape");
        (out.pixels, out.samples)
    }

    fn quit(&self) -> bool {
        self.quit
    }
}

/// Little-endian BCD, two digits per byte.
fn bcd(bytes: &[u8]) -> u32 {
    bytes.iter().rev().fold(0, |acc, &b| acc * 100 + (b >> 4) as u32 * 10 + (b & 15) as u32)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: probe <tetris.nes> [--type a|b] [--level N] [--height N] [--music N] [--seed N] [--pieces original|identical]");
        std::process::exit(2);
    };
    let flag = |name: &str| args.windows(2).find(|w| w[0] == name).map(|w| w[1].as_str());
    let num = |name: &str, default: u32| flag(name).map_or(default, |v| v.parse().unwrap_or_else(|_| panic!("{name} {v}")));
    let seed = flag("--seed").map_or_else(
        || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos(),
        |v| v.parse().unwrap_or_else(|_| panic!("--seed {v}")),
    );
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
    let file = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let rom = Arc::new(Rom::new(&file).unwrap_or_else(|e| panic!("{path}: {e}")));
    println!("seed {seed}: {setup:?}");
    frontend::run("Probe", frontend::Options::default(), |rate| Probe {
        runner: Runner::start_game(rom, Some(rate), setup),
        over: None,
        quit: false,
    });
}
