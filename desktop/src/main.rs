//! Play NES Tetris, reimplemented in readable Rust.
//!
//!     cargo run --release [-- path/to/tetris.nes]
//!     cargo run --release --features versus     # with VERSUS
//!
//! The data and graphics of your ROM (Tetris (USA)), not its code, are built into this program
//! (see `build.rs`), so it runs on its own, e.g. from the app bundle (`macos/bundle.sh`). A ROM
//! path given here is used instead.
//!
//! Settings, the last game's choices and the high scores are kept in `settings.toml`
//! (`settings.rs`), saved as they change. The title screen has START / SETTINGS / EXIT
//! (`tetris_rs::Options::menus`).

mod settings;
#[cfg(feature = "versus")]
mod versus;

use std::sync::Arc;
use settings::{LastGame, Settings, Table};
use tetris_rs::{Keys, Options, Rom, Runner, Saved};

/// What this program keeps of the ROM it was built with.
const ROM_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rom_data.bin"));

/// `gameMode` once a game (not the demo) is under way.
const MODE_PLAY: u8 = 4;
/// Where `initRam` leaves its marker once the high score table is set up.
const INIT_MAGIC: (usize, [u8; 5]) = (0x750, [0x12, 0x34, 0x56, 0x78, 0x9A]);

struct Readable {
    runner: Runner,
    settings: Settings,
    /// Write `settings.toml` when the settings change (not in tests).
    persist: bool,
    quit: bool,
    /// Bindings and volume for the window, after the settings screen changed them.
    options: Option<frontend::Options>,
    #[cfg(feature = "versus")]
    versus: versus::Versus,
    /// Frames left in which the lobby ignores key presses, after a match hands the window back
    /// (an Enter meant for the result shouldn't press READY or LEAVE).
    #[cfg(feature = "versus")]
    guard: u32,
}

/// Frames the lobby ignores presses for after a match: a third of a second.
#[cfg(feature = "versus")]
const GUARD_FRAMES: u32 = 20;

impl Readable {
    /// Save whatever this frame changed: the high score table, or the choices a game started with.
    fn save_changes(&mut self, ram: &[u8; 0x800]) {
        let (at, magic) = INIT_MAGIC;
        if ram[at..at + 5] != magic {
            return; // still powering on
        }
        let saved = Saved::from_ram(ram);
        let mut next = self.settings.clone();
        if self.settings.save_high_scores {
            next.high_scores = saved.high_scores.as_ref().map(Table::from_game);
        }
        if ram[0xC0] == MODE_PLAY {
            next.last_game = LastGame::from_game(&saved.choices);
        } else {
            // The music can be chosen on the settings screen too.
            next.last_game.music = LastGame::from_game(&saved.choices).music;
        }
        if next != self.settings {
            self.settings = next;
            self.save();
        }
    }

    fn save(&self) {
        if self.persist {
            self.settings.save();
        }
    }
}

impl frontend::Game for Readable {
    fn frame(&mut self, input: &frontend::Input) -> (Vec<u8>, Vec<f32>) {
        // A VERSUS match has the window; the game waits at the lobby meanwhile.
        #[cfg(feature = "versus")]
        {
            self.versus.poll();
            self.versus.music = self.settings.last_game.to_game().music_type;
            if self.versus.in_match() {
                let config = self.settings.config();
                let frame = versus::Frame { held: &input.held, pressed: &input.pressed, config: &config };
                let out = self.versus.match_frame(frame);
                self.versus.flush(&self.settings.versus);
                self.guard = GUARD_FRAMES;
                return out;
            }
        }
        #[allow(unused_mut)]
        let mut keys = Keys { held: input.held.clone(), pressed: input.pressed.clone(), unfocused: input.unfocused };
        #[cfg(feature = "versus")]
        if self.guard > 0 {
            self.guard -= 1;
            keys.pressed.clear();
        }
        #[cfg(feature = "versus")]
        let out = self.runner.next_frame_versus(keys, self.versus.view(&self.settings.versus));
        #[cfg(not(feature = "versus"))]
        let out = self.runner.next_frame_keys(keys);
        if let Some(config) = &out.config {
            self.settings.apply(config);
            self.save();
            self.options = Some(self.settings.frontend());
        }
        #[cfg(feature = "versus")]
        {
            let mut changed = false;
            for request in out.versus.iter().cloned() {
                changed |= self.versus.request(request, &mut self.settings.versus);
            }
            if changed {
                self.save();
            }
            self.versus.flush(&self.settings.versus);
        }
        self.save_changes(&out.ram);
        self.quit = out.quit;
        ((out.pixels as Box<[u8]>).into_vec(), out.samples)
    }

    fn quit(&self) -> bool {
        self.quit
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
    let settings = Settings::load();
    // Write the file on first launch so it's there to edit.
    if !Settings::dir().join("settings.toml").exists() {
        settings.save();
    }
    let saved = settings.saved();
    let options = Options { menus: true, config: settings.config() };
    frontend::run("Tetris", settings.frontend(), |rate| {
        #[cfg(feature = "versus")]
        let versus = versus::Versus::new(
            rom.clone(),
            Some(rate),
            args.server.clone().unwrap_or_else(|| settings.versus.server.clone()),
            args.name.as_deref().map(protocol::clean_name).unwrap_or_else(|| settings.versus.name.clone()),
        );
        Readable {
            runner: Runner::with_options(rom, Some(rate), Some(saved), options),
            settings,
            persist: true,
            quit: false,
            options: None,
            #[cfg(feature = "versus")]
            versus,
            #[cfg(feature = "versus")]
            guard: 0,
        }
    });
}

/// Copies of the app, headless, driven by keys through the real menus, playing each other through
/// a local room server over websockets.
#[cfg(all(test, feature = "versus"))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tetris_rs::versus::Connection;

    struct Copy {
        app: Readable,
        width: usize,
        last: Vec<u8>,
    }

    impl Copy {
        fn new(server: &str, name: &str) -> Self {
            let rom = Arc::new(Rom::from_extracted(ROM_DATA).unwrap());
            let settings = Settings::default();
            let saved = settings.saved();
            let options = Options { menus: true, config: settings.config() };
            let app = Readable {
                runner: Runner::with_options(rom.clone(), None, Some(saved), options),
                settings,
                persist: false,
                quit: false,
                options: None,
                versus: versus::Versus::new(rom, None, server.to_string(), name.to_string()),
                guard: 0,
            };
            Copy { app, width: 0, last: vec![] }
        }

        fn frame(&mut self, held: &[&str], pressed: &[&str]) {
            use frontend::Game;
            let input = frontend::Input {
                held: held.iter().map(|s| s.to_string()).collect(),
                pressed: pressed.iter().map(|s| s.to_string()).collect(),
                unfocused: false,
            };
            let (pixels, _) = self.app.frame(&input);
            self.width = pixels.len() / 240;
            self.last = pixels;
        }

        /// Save the last picture as `target/versus/<name>.png`.
        fn shot(&self, name: &str) {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/versus");
            std::fs::create_dir_all(&dir).unwrap();
            let mut ppm = format!("P6 {} 240 255\n", self.width).into_bytes();
            for &p in &self.last {
                let rgb = frontend::PALETTE[(p & 0x3F) as usize];
                ppm.extend_from_slice(&[(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]);
            }
            let ppm_path = dir.join(format!("{name}.ppm"));
            std::fs::write(&ppm_path, ppm).unwrap();
            let png = dir.join(format!("{name}.png"));
            let converted = std::process::Command::new("sips").args(["-s", "format", "png"]).arg(&ppm_path).arg("--out").arg(&png).output();
            if converted.is_ok_and(|o| o.status.success()) {
                let _ = std::fs::remove_file(ppm_path);
            }
        }

        fn idle(&mut self, n: usize) {
            for _ in 0..n {
                self.frame(&[], &[]);
            }
        }

        fn tap(&mut self, key: &str, after: usize) {
            self.frame(&[key], &[key]);
            self.idle(after);
        }

        fn view(&self) -> tetris_rs::versus::VersusView {
            self.app.versus.view(&self.app.settings.versus)
        }

        /// Power on, skip the legal screen, VERSUS, then `item` (0 HOST, 1 JOIN, 2 WATCH).
        fn open_versus(&mut self, item: usize) {
            self.idle(10);
            self.tap("Enter", 20);
            self.tap("ArrowDown", 3);
            self.tap("Enter", 20);
            for _ in 0..item {
                self.tap("ArrowDown", 3);
            }
            self.tap("Enter", 20);
        }

        fn type_code(&mut self, code: &str) {
            for c in code.chars() {
                self.tap(&format!("Key{c}"), 2);
            }
            self.tap("Enter", 5);
        }
    }

    /// Run every copy a frame at a time until `done`, playing randomly in matches.
    fn run(what: &str, copies: &mut [Copy], secs: u64, mut done: impl FnMut(&mut [Copy]) -> bool) {
        let start = Instant::now();
        let mut rng = 0x2545_F491_4F6C_DD1Du64;
        let mut held: Vec<&str> = vec![];
        let mut f = 0u64;
        while !done(copies) {
            if start.elapsed() > Duration::from_secs(secs) {
                let views: Vec<_> = copies.iter().map(|c| (c.view(), c.app.versus.in_match(), c.width)).collect();
                panic!("timed out waiting for {what}: {views:#?}");
            }
            if f.is_multiple_of(8) {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                held = match rng % 6 {
                    0 => vec!["ArrowLeft"],
                    1 => vec!["ArrowRight"],
                    2 => vec!["KeyX"],
                    3 => vec!["ArrowDown"],
                    _ => vec![],
                };
            }
            for c in copies.iter_mut() {
                if c.app.versus.in_match() {
                    c.frame(&held, &[]);
                } else {
                    c.idle(1);
                }
            }
            f += 1;
            std::thread::sleep(Duration::from_micros(300));
        }
    }

    fn local_room() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || netplay::local::serve(listener));
        format!("ws://127.0.0.1:{port}")
    }

    #[test]
    fn host_join_watch_play_rematch() {
        let server = local_room();
        let mut host = Copy::new(&server, "ANA");
        host.open_versus(0);
        let mut copies = vec![host];
        run("host in room", &mut copies, 10, |c| c[0].view().connection == Connection::InRoom);
        let code = copies[0].view().code.clone();
        assert_eq!(copies[0].view().seat, Some(0));
        assert_eq!(copies[0].width, 256);

        let mut joiner = Copy::new(&server, "BO");
        joiner.open_versus(1);
        joiner.type_code(&code);
        let mut watcher = Copy::new(&server, "CY");
        watcher.open_versus(2);
        watcher.type_code(&code);
        copies.push(joiner);
        copies.push(watcher);
        run("all in room", &mut copies, 10, |c| c.iter().all(|c| c.view().connection == Connection::InRoom) && c[0].view().watching == 1);
        assert_eq!(copies[1].view().seat, Some(1));
        assert_eq!(copies[2].view().seat, None);

        // The host makes it level 15 under ROOM SETTINGS (above READY, where the cursors start),
        // goes back, and both ready up.
        copies[0].tap("ArrowUp", 2);
        copies[0].tap("Enter", 2);
        for _ in 0..3 {
            copies[0].tap("ArrowLeft", 2);
        }
        run("level 15", &mut copies, 10, |c| c.iter().all(|c| c.view().settings.level == 15));
        copies[0].shot("room_settings");
        copies[0].tap("Escape", 2);
        copies[0].tap("ArrowDown", 2);
        copies[0].tap("Enter", 2);
        copies[1].tap("Enter", 2);

        // The match: every copy goes wide, plays to a result, and comes back to the lobby.
        run("match", &mut copies, 20, |c| c.iter().all(|c| c.app.versus.in_match()));
        run("wide", &mut copies, 20, |c| c.iter().all(|c| c.width == versus::MATCH_WIDTH));
        copies[1].shot("match_countdown");
        let started = Instant::now();
        run("into play", &mut copies, 20, |_| started.elapsed() > Duration::from_millis(4000));
        copies[0].shot("match_playing");
        copies[2].shot("match_watching");
        run("confirmed", &mut copies, 120, |c| c.iter().all(|c| c.app.versus.confirmed()));
        copies[0].shot("match_result");
        for c in copies.iter_mut() {
            c.tap("Enter", 2);
        }
        run("back to lobby", &mut copies, 10, |c| c.iter().all(|c| !c.app.versus.in_match() && c.width == 256));
        copies[0].idle(3);
        copies[0].shot("lobby_after");
        let result = copies[2].view().result;
        assert!(result.contains("WON") || result.contains("DRAW"), "{:#?}", copies.iter().map(|c| c.view()).collect::<Vec<_>>());
        assert!(copies.iter().all(|c| c.view().result == result), "everyone saw the same result");
        assert_eq!(copies[0].view().status, "READY FOR A REMATCH");
        // The level was saved for next time.
        assert_eq!(copies[0].app.settings.versus.room.level, 15);

        // A rematch (once the lobby takes presses again).
        for c in copies.iter_mut() {
            c.idle(GUARD_FRAMES as usize);
        }
        copies[0].tap("Enter", 2);
        copies[1].tap("Enter", 2);
        run("rematch", &mut copies, 20, |c| c.iter().all(|c| c.app.versus.in_match()));
    }

    #[test]
    fn escape_twice_leaves_a_match() {
        let server = local_room();
        let mut host = Copy::new(&server, "ANA");
        host.open_versus(0);
        let mut copies = vec![host];
        run("host in room 2", &mut copies, 10, |c| c[0].view().connection == Connection::InRoom);
        let code = copies[0].view().code.clone();
        let mut joiner = Copy::new(&server, "BO");
        joiner.open_versus(1);
        joiner.type_code(&code);
        copies.push(joiner);
        run("both seated", &mut copies, 10, |c| c.iter().all(|c| c.view().players.iter().all(|p| p.is_some())));
        copies[0].tap("Enter", 2);
        copies[1].tap("Enter", 2);
        run("match 2", &mut copies, 20, |c| c.iter().all(|c| c.app.versus.in_match()));
        copies[1].tap("Escape", 2);
        assert!(copies[1].app.versus.in_match(), "one Escape only warns");
        copies[1].tap("Escape", 2);
        assert!(!copies[1].app.versus.in_match());
        assert_eq!(copies[1].view().connection, Connection::Lost("YOU LEFT THE MATCH".into()));
        // The host's match ends at once: BO forfeits.
        run("host sees the forfeit", &mut copies, 10, |c| c[0].view().result == "ANA WON BY FORFEIT");
        copies[0].idle(1);
        assert_eq!(copies[0].view().status, "WAITING FOR A PLAYER");
    }

    /// The host leaves mid-match: the opponent wins by forfeit and becomes host, and the result
    /// still shows their own board on the left and the host's (stopped) on the right.
    #[test]
    fn host_leaving_hands_over() {
        let server = local_room();
        let mut host = Copy::new(&server, "ANA");
        host.open_versus(0);
        let mut copies = vec![host];
        run("host in room", &mut copies, 10, |c| c[0].view().connection == Connection::InRoom);
        let code = copies[0].view().code.clone();
        let mut joiner = Copy::new(&server, "BO");
        joiner.open_versus(1);
        joiner.type_code(&code);
        copies.push(joiner);
        run("both seated", &mut copies, 10, |c| c.iter().all(|c| c.view().players.iter().all(|p| p.is_some())));
        copies[0].tap("Enter", 2);
        copies[1].tap("Enter", 2);
        run("match", &mut copies, 20, |c| c.iter().all(|c| c.app.versus.in_match()));
        // A moment into play (random play at level 18 can top out within a second).
        run("into play", &mut copies, 20, |c| c.iter().all(|c| c.app.versus.playing()));
        let started = Instant::now();
        run("a moment in", &mut copies, 20, |_| started.elapsed() > Duration::from_millis(300));
        copies[0].tap("Escape", 2);
        copies[0].tap("Escape", 2);
        let mut bo = copies.remove(1);
        let mut only = vec![bo];
        run("the forfeit", &mut only, 10, |c| c[0].view().result == "BO WON BY FORFEIT");
        bo = only.remove(0);
        assert_eq!(bo.view().seat, Some(0), "BO hosts now");
        assert!(bo.app.versus.in_match(), "the result is showing");
        bo.idle(1);
        bo.shot("host_left");
        // Both boards drawn: BO's own on the left, ANA's stopped one on the right.
        let lit = |x0: usize| (0..240).any(|y| (x0..x0 + versus::MATCH_WIDTH / 2).any(|x| bo.last[y * bo.width + x] != 0x0F));
        assert_eq!(bo.width, versus::MATCH_WIDTH);
        assert!(lit(0) && lit(versus::MATCH_WIDTH / 2));
    }
}
