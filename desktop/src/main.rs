//! Play NES Tetris, reimplemented in readable Rust.
//!
//!     cargo run --release [-- path/to/tetris.nes]
//!     cargo run --release --features versus     # with VERSUS
//!
//! The ROM (Tetris (USA)) is the player's: given on the command line, or asked for on first
//! launch (INSERT ROM, `insert.rs`) and kept in the settings folder after that. With the
//! `embedded-rom` feature its data and graphics, not its code, are built into this program instead
//! (`build.rs`), so it never asks; such a build isn't to be shared.
//!
//! The app itself (the game, its menus, settings, VERSUS) is `client`; this is its window and its
//! `settings.toml` (`files.rs`), saved as the settings change.

mod files;
mod insert;

use std::sync::Arc;
use std::time::Instant;

use client::{App, Input};
use tetris_rs::Rom;

/// What this program keeps of the ROM it was built with.
#[cfg(feature = "embedded-rom")]
const ROM_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rom_data.bin"));

/// INSERT ROM, until there's a ROM; then the app.
enum Screen {
    Insert(insert::Insert),
    App(Box<App>),
}

/// The app in the window: settings kept in `settings.toml`.
struct Desktop {
    screen: Screen,
    #[cfg_attr(not(feature = "versus"), allow(dead_code))]
    args: Args,
    rate: u32,
    seed: u64,
    start: Instant,
    /// Bindings and volume for the window, after the settings screen changed them.
    options: Option<frontend::Options>,
    /// Escape on INSERT ROM.
    quit: bool,
}

impl Desktop {
    /// The app from power-on with `rom`.
    fn app(&self, rom: Arc<Rom>) -> Box<App> {
        #[allow(unused_mut)]
        let mut app = App::new(rom, Some(self.rate), files::load(), self.seed);
        #[cfg(feature = "versus")]
        app.set_versus(self.args.server.clone(), self.args.name.as_deref());
        Box::new(app)
    }
}

impl frontend::Game for Desktop {
    fn frame(&mut self, input: &frontend::Input) -> (Vec<u8>, Vec<f32>) {
        let app = match &mut self.screen {
            Screen::App(app) => app,
            Screen::Insert(insert) => {
                let (picture, outcome) = insert.frame(input);
                match outcome {
                    insert::Outcome::Waiting => {}
                    insert::Outcome::Quit => self.quit = true,
                    insert::Outcome::Rom(rom, file) => {
                        files::keep_rom(&file);
                        self.screen = Screen::App(self.app(rom));
                        self.start = Instant::now();
                    }
                }
                return (picture, vec![]);
            }
        };
        let input = Input { held: input.held.clone(), pressed: input.pressed.clone(), unfocused: input.unfocused };
        let out = app.frame(&input, self.start.elapsed().as_millis() as u64);
        if app.take_dirty() {
            files::save(app.settings());
        }
        if app.take_config_changed() {
            self.options = Some(files::frontend(app.settings()));
        }
        out
    }

    fn height(&self) -> usize {
        match &self.screen {
            Screen::App(app) => app.height(),
            Screen::Insert(_) => frontend::HEIGHT,
        }
    }

    fn quit(&self) -> bool {
        self.quit || matches!(&self.screen, Screen::App(app) if app.quit())
    }

    fn take_options(&mut self) -> Option<frontend::Options> {
        self.options.take()
    }
}

/// The command line: an optional ROM path (instead of the one built in), and with VERSUS
/// `--name NAME` and `--server URL`, which override the saved ones for this run without saving
/// (two copies on one machine share `settings.toml`).
struct Args {
    rom: Option<std::path::PathBuf>,
    name: Option<String>,
    server: Option<String>,
}

fn args() -> Args {
    let mut args = Args { rom: None, name: None, server: None };
    let mut it = std::env::args_os().skip(1);
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--name") => args.name = it.next().and_then(|v| v.into_string().ok()),
            Some("--server") => args.server = it.next().and_then(|v| v.into_string().ok()),
            _ => args.rom = Some(a.into()),
        }
    }
    if cfg!(not(feature = "versus")) && (args.name.is_some() || args.server.is_some()) {
        eprintln!("--name and --server are for VERSUS, which this build doesn't have (--features versus)");
    }
    args
}

/// The ROM to start with: the one on the command line, the one built in, or the one kept from an
/// earlier INSERT ROM; none if there's none yet.
fn rom(args: &Args) -> Option<Arc<Rom>> {
    if let Some(path) = &args.rom {
        let file = std::fs::read(path).unwrap_or_else(|e| panic!("can't read ROM {}: {e}", path.display()));
        return Some(Arc::new(Rom::new(&file).unwrap_or_else(|e| panic!("ROM {}: {e}", path.display()))));
    }
    #[cfg(feature = "embedded-rom")]
    return Some(Arc::new(Rom::from_extracted(ROM_DATA).unwrap_or_else(|e| panic!("ROM: {e}"))));
    #[allow(unreachable_code)]
    let file = std::fs::read(files::rom_path()).ok()?;
    Rom::new(&file).map(Arc::new).map_err(|e| eprintln!("{}: {e}", files::rom_path().display())).ok()
}

fn main() {
    let args = args();
    let rom = rom(&args);
    let settings = files::load();
    // Write the file on first launch so it's there to edit.
    if !files::dir().join("settings.toml").exists() {
        files::save(&settings);
    }
    let options = files::frontend(&settings);
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_nanos() as u64) ^ std::process::id() as u64;
    frontend::run("Tetris", options, |rate| {
        let mut desktop = Desktop { screen: Screen::Insert(insert::Insert::new()), args, rate, seed, start: Instant::now(), options: None, quit: false };
        if let Some(rom) = rom {
            desktop.screen = Screen::App(desktop.app(rom));
        }
        desktop
    });
}
