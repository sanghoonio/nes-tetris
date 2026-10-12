//! Play NES Tetris, reimplemented in readable Rust.
//!
//!     cargo run --release [-- path/to/tetris.nes]
//!     cargo run --release --features versus     # with VERSUS
//!
//! The data and graphics of your ROM (Tetris (USA)), not its code, are built into this program
//! (see `build.rs`), so it runs on its own, e.g. from the app bundle (`macos/bundle.sh`). A ROM
//! path given here is used instead.
//!
//! The app itself (the game, its menus, settings, VERSUS) is `client`; this is its window and its
//! `settings.toml` (`files.rs`), saved as the settings change.

mod files;

use std::sync::Arc;
use std::time::Instant;

use client::{App, Input};
use tetris_rs::Rom;

/// What this program keeps of the ROM it was built with.
const ROM_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rom_data.bin"));

/// The app in the window: settings kept in `settings.toml`.
struct Desktop {
    app: App,
    start: Instant,
    /// Bindings and volume for the window, after the settings screen changed them.
    options: Option<frontend::Options>,
}

impl frontend::Game for Desktop {
    fn frame(&mut self, input: &frontend::Input) -> (Vec<u8>, Vec<f32>) {
        let input = Input { held: input.held.clone(), pressed: input.pressed.clone(), unfocused: input.unfocused };
        let out = self.app.frame(&input, self.start.elapsed().as_millis() as u64);
        if self.app.take_dirty() {
            files::save(self.app.settings());
        }
        if self.app.take_config_changed() {
            self.options = Some(files::frontend(self.app.settings()));
        }
        out
    }

    fn quit(&self) -> bool {
        self.app.quit()
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

fn main() {
    let args = args();
    let rom = match &args.rom {
        Some(path) => {
            let file = std::fs::read(path).unwrap_or_else(|e| panic!("can't read ROM {}: {e}", path.display()));
            Rom::new(&file)
        }
        None => Rom::from_extracted(ROM_DATA),
    };
    let rom = Arc::new(rom.unwrap_or_else(|e| panic!("ROM: {e}")));
    let settings = files::load();
    // Write the file on first launch so it's there to edit.
    if !files::dir().join("settings.toml").exists() {
        files::save(&settings);
    }
    let options = files::frontend(&settings);
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_nanos() as u64) ^ std::process::id() as u64;
    frontend::run("Tetris", options, |rate| {
        #[allow(unused_mut)]
        let mut app = App::new(rom, Some(rate), settings, seed);
        #[cfg(feature = "versus")]
        app.set_versus(args.server.clone(), args.name.as_deref());
        Desktop { app, start: Instant::now(), options: None }
    });
}
