//! The app around the game, the same on every platform: the game with its menus, the settings
//! and saving them, and VERSUS (rooms, the list of rooms, matches, the match view). A shell runs
//! it: the desktop's window (`desktop`) or a web page (`web/engine`), which supply the keys, the
//! time, the screen and sound, and keep the settings where they keep things.

#[cfg(feature = "versus")]
pub mod backdrop;
pub mod settings;
#[cfg(feature = "versus")]
pub mod versus;

use std::sync::Arc;

pub use settings::Settings;
use settings::{LastGame, Table};
use tetris_rs::frame::HEIGHT;
use tetris_rs::{Keys, Options, Rom, Runner, Saved};

/// `gameMode` once a game (not the demo) is under way.
const MODE_PLAY: u8 = 4;
/// Where `initRam` leaves its marker once the high score table is set up.
const INIT_MAGIC: (usize, [u8; 5]) = (0x750, [0x12, 0x34, 0x56, 0x78, 0x9A]);

/// The keys this frame, by name (`KeyX`, `ArrowLeft`, `PadSouth`...).
#[derive(Clone, Debug, Default)]
pub struct Input {
    pub held: Vec<String>,
    pub pressed: Vec<String>,
    /// The window doesn't have the keyboard.
    pub unfocused: bool,
}

pub struct App {
    runner: Runner,
    settings: Settings,
    /// The settings changed since the shell last saved them.
    dirty: bool,
    /// The settings screen changed something the shell shows (volume, fullscreen, scaling).
    config_changed: bool,
    quit: bool,
    /// The last frame had a game in play (`FrameOut::in_play`).
    in_play: bool,
    /// The last picture's height (taller during a VERSUS match).
    height: usize,
    #[cfg(feature = "versus")]
    versus: versus::Versus,
    /// Frames left in which the lobby ignores key presses, after a match hands the window back
    /// (an Enter meant for the result shouldn't press READY or LEAVE).
    #[cfg(feature = "versus")]
    guard: u32,
}

/// Frames the lobby ignores presses for after a match: a third of a second.
#[cfg(feature = "versus")]
pub const GUARD_FRAMES: u32 = 20;

impl App {
    /// The game from power-on with `settings`; `sample_rate` for its sound (none for silence),
    /// `seed` for anything random the app makes up (room codes).
    pub fn new(rom: Arc<Rom>, sample_rate: Option<u32>, settings: Settings, seed: u64) -> Self {
        let options = Options { menus: true, config: settings.config() };
        #[cfg(feature = "versus")]
        let versus = versus::Versus::new(rom.clone(), sample_rate, settings.versus.server.clone(), settings.versus.name.clone(), seed);
        #[cfg(not(feature = "versus"))]
        let _ = seed;
        App {
            runner: Runner::with_options(rom, sample_rate, Some(settings.saved()), options),
            settings,
            dirty: false,
            config_changed: false,
            quit: false,
            in_play: false,
            height: HEIGHT,
            #[cfg(feature = "versus")]
            versus,
            #[cfg(feature = "versus")]
            guard: 0,
        }
    }

    /// For this run only (not saved): the room server, and the player's name.
    #[cfg(feature = "versus")]
    pub fn set_versus(&mut self, server: Option<String>, name: Option<&str>) {
        if let Some(server) = server {
            self.versus.server = server;
        }
        if let Some(name) = name {
            self.versus.name = protocol::clean_name(name);
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Whether the settings changed since this was last asked (to save them).
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Whether the settings screen changed the volume, fullscreen or scaling since this was
    /// last asked.
    pub fn take_config_changed(&mut self) -> bool {
        std::mem::take(&mut self.config_changed)
    }

    /// Whether in a VERSUS room, where the game can't stop (the others play on).
    pub fn online(&self) -> bool {
        #[cfg(feature = "versus")]
        return self.versus.in_room();
        #[cfg(not(feature = "versus"))]
        false
    }

    /// The last picture's height in pixels (its width is its length over this).
    pub fn height(&self) -> usize {
        self.height
    }

    /// EXIT was chosen.
    /// Whether a game is running that losing focus would pause, and hasn't yet: a single-player
    /// game in play, or a match against your own CPU. (A shell that stops calling `frame` when
    /// hidden, as a browser tab does, runs a few unfocused frames first, until this is false.)
    pub fn pausable(&self) -> bool {
        #[cfg(feature = "versus")]
        if self.versus.in_match() {
            return self.versus.bot_match_running();
        }
        self.in_play
    }

    pub fn quit(&self) -> bool {
        self.quit
    }

    /// One frame at `now` (milliseconds on any steady clock): the picture (palette indices,
    /// `height()` rows, as wide as the slice says) and the sound.
    pub fn frame(&mut self, input: &Input, now: u64) -> (Vec<u8>, Vec<f32>) {
        #[cfg(not(feature = "versus"))]
        let _ = now;
        // A VERSUS match has the window; the game waits at the lobby meanwhile.
        #[cfg(feature = "versus")]
        {
            self.versus.now = now;
            self.versus.poll();
            self.versus.music = self.settings.last_game.to_game().music_type;
            if self.versus.in_match() {
                let config = self.settings.config();
                let frame = versus::Frame { held: &input.held, pressed: &input.pressed, unfocused: input.unfocused, config: &config };
                let out = self.versus.match_frame(frame);
                self.versus.flush(&self.settings.versus);
                self.guard = GUARD_FRAMES;
                // (Leaving with Escape gives a plain screen's black.)
                self.height = if out.0.len() == versus::MATCH_WIDTH * versus::MATCH_HEIGHT { versus::MATCH_HEIGHT } else { HEIGHT };
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
            self.dirty = true;
            self.config_changed = true;
        }
        #[cfg(feature = "versus")]
        {
            for request in out.versus.iter().cloned() {
                self.dirty |= self.versus.request(request, &mut self.settings.versus);
            }
            self.versus.flush(&self.settings.versus);
        }
        self.save_changes(&out.ram);
        self.quit = out.quit;
        self.in_play = out.in_play;
        self.height = HEIGHT;
        ((out.pixels as Box<[u8]>).into_vec(), out.samples)
    }

    /// Keep whatever this frame changed: the high score table, or the choices a game started with.
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
            self.dirty = true;
        }
    }
}

/// Copies of the app, headless, driven by keys through the real menus, playing each other through
/// a local room server over websockets.
#[cfg(all(test, feature = "versus"))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tetris_rs::versus::{Connection, ListedState, RoomList};

    pub(crate) fn rom() -> Arc<Rom> {
        let file = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tetris.nes")).expect("tetris.nes beside the workspace");
        Arc::new(Rom::new(&file).unwrap())
    }

    struct Copy {
        app: App,
        start: Instant,
        width: usize,
        height: usize,
        last: Vec<u8>,
    }

    impl Copy {
        fn new(server: &str, name: &str) -> Self {
            let mut app = App::new(rom(), None, Settings::default(), name.len() as u64 * 7919 + server.len() as u64);
            app.set_versus(Some(server.to_string()), Some(name));
            Copy { app, start: Instant::now(), width: 0, height: 0, last: vec![] }
        }

        fn frame(&mut self, held: &[&str], pressed: &[&str]) {
            self.frame_focus(held, pressed, true);
        }

        fn frame_focus(&mut self, held: &[&str], pressed: &[&str], focused: bool) {
            let input = Input {
                held: held.iter().map(|s| s.to_string()).collect(),
                pressed: pressed.iter().map(|s| s.to_string()).collect(),
                unfocused: !focused,
            };
            let (pixels, _) = self.app.frame(&input, self.start.elapsed().as_millis() as u64);
            self.height = self.app.height();
            self.width = pixels.len() / self.height;
            self.last = pixels;
        }

        /// Save the last picture as `target/versus/<name>.png`.
        fn shot(&self, name: &str) {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/versus");
            std::fs::create_dir_all(&dir).unwrap();
            let mut ppm = format!("P6 {} {} 255\n", self.width, self.height).into_bytes();
            for &p in &self.last {
                let rgb = ppu::PALETTE[(p & 0x3F) as usize];
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

        /// On JOIN or WATCH: wait for the room `code` to be listed (it's the only room, so the
        /// cursor lands on it), then choose it.
        fn choose(&mut self, code: &str) {
            let start = Instant::now();
            while !matches!(&self.view().rooms, RoomList::Rooms(r) if r.iter().any(|r| r.code == code)) {
                assert!(start.elapsed() < Duration::from_secs(5), "{code} never listed: {:?}", self.view().rooms);
                self.idle(1);
                std::thread::sleep(Duration::from_millis(2));
            }
            self.idle(3);
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

    /// Screenshots of JOIN and WATCH with more rooms than fit (`target/versus/list_*.png`): 18
    /// open rooms, two full, one playing; at the top, then turning pages with Right and Left.
    #[test]
    #[ignore = "makes screenshots"]
    fn crowded_room_lists() {
        let server = local_room();
        let names = [
            "ANA", "BO", "CYRILLIC", "DEE", "EVE", "FINN", "GUS", "HAL", "IVY", "JO", "KAI", "LEO", "MAX", "NED", "OZ", "PIA", "QUIN", "ROY",
            "SUE", "TED", "UMA", "VIC", "WES", "XAN",
        ];
        let mut links = vec![];
        let mut next = names.iter();
        let mut seat = |code: &str, ready: bool| {
            let link = netplay::Link::open(&format!("{server}/room/{code}"));
            link.send(protocol::ToRoom::Hello { name: next.next().unwrap().to_string(), watch: false });
            std::thread::sleep(Duration::from_millis(30));
            if ready {
                link.send(protocol::ToRoom::Ready { ready: true });
            }
            link
        };
        let open = ["AAAA", "BBBB", "CCCC", "DDDD", "EEEE", "FFFF", "GGGG", "HHHH", "JJJJ", "KKKK", "PPPP", "QQQQ", "RRRR", "SSSS", "TTTT", "UUUU", "VVVV", "WWWW"];
        for (i, code) in open.iter().enumerate() {
            let host = seat(code, false);
            let settings = protocol::Settings { level: (i * 3 % 20) as u8, ..protocol::Settings::default() };
            host.send(protocol::ToRoom::Settings { settings });
            links.push(host);
        }
        for code in ["LLLL", "MMMM"] {
            links.push(seat(code, false));
            links.push(seat(code, false));
        }
        links.push(seat("NNNN", true));
        links.push(seat("NNNN", true));
        for (item, name) in [(1, "join"), (2, "watch")] {
            let mut copy = Copy::new(&server, "ZED");
            copy.open_versus(item);
            run("listed", std::slice::from_mut(&mut copy), 10, |c| matches!(&c[0].view().rooms, RoomList::Rooms(r) if r.len() == 21 && r.iter().any(|r| r.state == ListedState::Playing)));
            // Off a frame where the cursor's arrows blink out.
            copy.idle(6);
            copy.shot(&format!("list_{name}_top"));
            // A page at a time: a middle page (arrows both ways), the last, ENTER A CODE; and back.
            for (i, key) in ["ArrowRight", "ArrowRight", "ArrowRight", "ArrowLeft"].into_iter().enumerate() {
                copy.tap(key, 3);
                // Off a frame where the cursor's arrows blink out.
                copy.idle(6);
                copy.shot(&format!("list_{name}_page_{}", i + 1));
            }
        }
        drop(links);
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
        joiner.choose(&code);
        let mut watcher = Copy::new(&server, "CY");
        watcher.open_versus(2);
        watcher.choose(&code);
        copies.push(joiner);
        copies.push(watcher);
        run("all in room", &mut copies, 10, |c| c.iter().all(|c| c.view().connection == Connection::InRoom) && c[0].view().watching == 1);
        assert_eq!(copies[1].view().seat, Some(1));
        assert_eq!(copies[2].view().seat, None);

        // The host makes it level 15 under ROOM SETTINGS (above READY, where the cursors start),
        // goes back, and both ready up.
        copies[0].tap("ArrowUp", 2);
        copies[0].tap("Enter", 2);
        for _ in 0..15 {
            copies[0].tap("ArrowRight", 2);
        }
        run("level 15", &mut copies, 10, |c| c.iter().all(|c| c.view().settings.level == 15));
        copies[0].shot("room_settings");
        // The opponent sees them too, greyed out (only the host changes them).
        copies[1].tap("ArrowUp", 2);
        copies[1].tap("Enter", 2);
        copies[1].idle(3);
        copies[1].shot("room_settings_guest");
        copies[1].tap("Escape", 2);
        copies[1].tap("ArrowDown", 2);
        copies[0].tap("Escape", 2);
        copies[0].tap("ArrowDown", 2);
        copies[0].tap("Enter", 2);
        copies[1].tap("Enter", 2);

        // The match: every copy goes wide, plays to a result, and comes back to the lobby.
        run("match", &mut copies, 20, |c| c.iter().all(|c| c.app.versus.in_match()));
        run("wide", &mut copies, 20, |c| c.iter().all(|c| c.width == versus::MATCH_WIDTH));
        // Another finds the match under WATCH, marked LIVE, and watches it too.
        let mut late = Copy::new(&server, "DEE");
        late.open_versus(2);
        let start = Instant::now();
        while !matches!(&late.view().rooms, RoomList::Rooms(r) if r.iter().any(|r| r.state == ListedState::Playing)) {
            assert!(start.elapsed() < Duration::from_secs(5), "no live match listed: {:?}", late.view().rooms);
            late.idle(1);
        }
        late.shot("watch_live");
        late.choose(&code);
        copies.push(late);
        run("a second watcher", &mut copies, 10, |c| c[3].view().connection == Connection::InRoom && c[3].app.versus.in_match());

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
        let series = copies[2].view().series;
        let one = ["ANA 1 - 0 BO", "ANA 0 - 1 BO", "ANA 0 - 0 BO"];
        assert!(series.len() == 1 && one.contains(&series[0].as_str()), "{:#?}", copies.iter().map(|c| c.view()).collect::<Vec<_>>());
        assert!(copies.iter().all(|c| c.view().series == series), "everyone saw the same score");
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

    /// Two in a room, seated (the joiner from JOIN's list).
    fn two_in_a_room(host: &str, joiner: &str) -> (Vec<Copy>, String) {
        let server = local_room();
        let mut a = Copy::new(&server, host);
        a.open_versus(0);
        let mut copies = vec![a];
        run("host in room", &mut copies, 10, |c| c[0].view().connection == Connection::InRoom);
        let code = copies[0].view().code.clone();
        let mut b = Copy::new(&server, joiner);
        b.open_versus(1);
        b.choose(&code);
        copies.push(b);
        run("both seated", &mut copies, 10, |c| c.iter().all(|c| c.view().players.iter().all(|p| p.is_some())));
        (copies, code)
    }

    /// The host removes the opponent: they're back at the VERSUS menu, told so, and the seat
    /// is free.
    #[test]
    fn the_host_kicks() {
        let (mut copies, _) = two_in_a_room("ANA", "BO");
        copies[0].tap("ArrowUp", 2);
        copies[0].tap("ArrowUp", 2);
        copies[0].tap("Enter", 2);
        copies[0].tap("ArrowRight", 2);
        copies[0].tap("Enter", 2);
        run("BO removed", &mut copies, 10, |c| c[1].view().connection == Connection::Lost("THE HOST REMOVED YOU".into()));
        run("the seat free", &mut copies, 10, |c| c[0].view().players[1].is_none() && c[0].view().status == "WAITING FOR A PLAYER");
        copies[1].idle(5);
        copies[1].shot("kicked");
    }

    /// Two players called PLAYER: the score between them, then which of them left, still work.
    #[test]
    fn one_name_for_two() {
        let (mut copies, _) = two_in_a_room("PLAYER", "PLAYER");
        copies[0].tap("Enter", 2);
        copies[1].tap("Enter", 2);
        run("match", &mut copies, 20, |c| c.iter().all(|c| c.app.versus.in_match()));
        run("confirmed", &mut copies, 120, |c| c.iter().all(|c| c.app.versus.confirmed()));
        for c in copies.iter_mut() {
            c.tap("Enter", 2);
        }
        run("back to lobby", &mut copies, 10, |c| c.iter().all(|c| !c.app.versus.in_match()));
        let one = ["PLAYER 1 - 0 PLAYER", "PLAYER 0 - 1 PLAYER", "PLAYER 0 - 0 PLAYER"];
        assert!(one.contains(&copies[0].view().series[0].as_str()), "{:?}", copies[0].view().series);
        // The joiner leaves.
        for c in copies.iter_mut() {
            c.idle(GUARD_FRAMES as usize);
        }
        copies[1].tap("Escape", 5);
        copies.truncate(1);
        run("PLAYER left", &mut copies, 10, |c| c[0].view().series == ["PLAYER LEFT"]);
    }

    /// The host adds a bot from the lobby and plays it: the bot's game runs on the host's
    /// machine, the result is confirmed, and the score between them counts it; the host kicks it.
    #[test]
    fn a_match_against_a_bot() {
        let server = local_room();
        let mut host = Copy::new(&server, "ANA");
        host.open_versus(0);
        let mut copies = vec![host];
        run("host in room", &mut copies, 10, |c| c[0].view().connection == Connection::InRoom);
        // Up from READY past ROOM SETTINGS to ADD CPU, beside the empty seat's dash.
        copies[0].tap("ArrowUp", 2);
        copies[0].tap("ArrowUp", 2);
        copies[0].tap("Enter", 2);
        run("a bot seated", &mut copies, 10, |c| c[0].view().players[1].as_ref().is_some_and(|p| p.0.ends_with(" (CPU)") && p.1));
        let bot = copies[0].view().players[1].clone().unwrap().0;
        // The cursor's back on READY.
        copies[0].idle(5);
        copies[0].tap("Enter", 2);
        run("the match", &mut copies, 20, |c| c[0].app.versus.in_match());
        // Over, or decided with the winner playing on (the bot, after the host tops out, can play
        // on for minutes): then the host ends it with Escape twice.
        run("decided", &mut copies, 120, |c| c[0].app.versus.confirmed() || c[0].app.versus.decided());
        if !copies[0].app.versus.confirmed() {
            copies[0].tap("Escape", 2);
            copies[0].tap("Escape", 2);
        }
        run("confirmed", &mut copies, 20, |c| c[0].app.versus.confirmed());
        copies[0].shot("match_bot");
        copies[0].tap("Enter", 2);
        run("back to lobby", &mut copies, 10, |c| !c[0].app.versus.in_match());
        let name = bot.trim_end_matches(" (CPU)");
        let series = &copies[0].view().series;
        assert!(series.len() == 1 && series[0].starts_with("ANA ") && series[0].ends_with(name), "{series:?}");
        // Another, quit with Escape twice: a forfeit, and still in the room with the bot.
        copies[0].idle(GUARD_FRAMES as usize);
        copies[0].tap("Enter", 2);
        run("a second match", &mut copies, 20, |c| c[0].app.versus.playing());
        copies[0].tap("Escape", 2);
        copies[0].tap("Escape", 2);
        run("the forfeit", &mut copies, 20, |c| c[0].app.versus.confirmed());
        copies[0].tap("Enter", 2);
        run("back to lobby again", &mut copies, 10, |c| !c[0].app.versus.in_match());
        assert_eq!(copies[0].view().connection, Connection::InRoom);
        assert!(copies[0].view().players[1].as_ref().is_some_and(|p| p.0.ends_with(" (CPU)")));
        // KICK? YES removes it (Up past ROOM SETTINGS to its name).
        copies[0].idle(GUARD_FRAMES as usize);
        copies[0].tap("ArrowUp", 2);
        copies[0].tap("ArrowUp", 2);
        copies[0].tap("Enter", 2);
        copies[0].tap("ArrowRight", 2);
        copies[0].tap("Enter", 2);
        run("the bot gone", &mut copies, 10, |c| c[0].view().players[1].is_none());
    }

    /// OFFLINE: no server at all (an address that goes nowhere); a bot is seated at once, and
    /// the match is played and confirmed in the room run here.
    #[test]
    fn vs_bot_offline() {
        let mut c = Copy::new("ws://127.0.0.1:1", "ANA");
        c.idle(10);
        c.tap("Enter", 20);
        c.tap("ArrowDown", 3);
        c.tap("Enter", 20);
        for _ in 0..3 {
            c.tap("ArrowDown", 3);
        }
        c.tap("Enter", 20);
        let mut copies = vec![c];
        run("a bot seated", &mut copies, 10, |c| c[0].view().players[1].as_ref().is_some_and(|p| p.0.ends_with(" (CPU)")));
        assert!(copies[0].view().offline);
        copies[0].idle(5);
        copies[0].shot("vs_bot_lobby");
        copies[0].tap("Enter", 2);
        run("the match", &mut copies, 20, |c| c[0].app.versus.in_match());
        run("decided", &mut copies, 120, |c| c[0].app.versus.confirmed() || c[0].app.versus.decided());
        if !copies[0].app.versus.confirmed() {
            copies[0].tap("Escape", 2);
            copies[0].tap("Escape", 2);
        }
        run("confirmed", &mut copies, 20, |c| c[0].app.versus.confirmed());
        copies[0].tap("Enter", 2);
        run("back to the lobby", &mut copies, 10, |c| !c[0].app.versus.in_match());
        assert_eq!(copies[0].view().series.len(), 1);
    }

    /// Offline against a CPU: Escape pauses (both games stand still, the pause screen shows);
    /// RESUME carries on; QUIT resigns, and we're still in the lobby.
    #[test]
    fn pausing_against_a_cpu() {
        let mut c = Copy::new("ws://127.0.0.1:1", "ANA");
        c.idle(10);
        c.tap("Enter", 20);
        c.tap("ArrowDown", 3);
        c.tap("Enter", 20);
        for _ in 0..3 {
            c.tap("ArrowDown", 3);
        }
        c.tap("Enter", 20);
        let mut copies = vec![c];
        run("a CPU seated", &mut copies, 10, |c| c[0].view().players[1].is_some());
        copies[0].idle(5);
        copies[0].tap("Enter", 2);
        run("playing", &mut copies, 20, |c| c[0].app.versus.playing());
        copies[0].idle(120);
        copies[0].tap("Escape", 2);
        let frozen = copies[0].app.versus.scores();
        copies[0].idle(240);
        assert_eq!(copies[0].app.versus.scores(), frozen, "both games stand still");
        copies[0].shot("paused");
        // RESUME: the games go on.
        copies[0].tap("Enter", 2);
        copies[0].idle(600);
        assert!(copies[0].app.versus.playing());
        // Pause again; Down to QUIT, Enter: resigned, the result, then the lobby.
        copies[0].tap("Escape", 2);
        copies[0].tap("ArrowDown", 2);
        copies[0].tap("Enter", 2);
        run("the result", &mut copies, 10, |c| c[0].app.versus.confirmed());
        copies[0].tap("Enter", 2);
        run("back to the lobby", &mut copies, 10, |c| !c[0].app.versus.in_match());
        assert_eq!(copies[0].view().connection, Connection::InRoom);
    }

    /// Losing focus pauses a single-player game, and a match against your own CPU, within the
    /// frames a hidden browser tab is given (`App::pausable`).
    #[test]
    fn losing_focus_pauses() {
        // Single-player: through the menus into an A-type game.
        let mut c = Copy::new("ws://127.0.0.1:1", "ANA");
        c.idle(10);
        for _ in 0..5 {
            c.tap("Enter", 40);
        }
        c.idle(60);
        assert!(c.app.pausable(), "a game in play");
        let frames = (0..60).take_while(|_| {
            c.frame_focus(&[], &[], false);
            c.app.pausable()
        });
        assert!(frames.count() < 59, "paused within a second");
        c.idle(5);
        assert!(!c.app.pausable(), "still paused with focus back");

        // Against the CPU (OFFLINE), with sound: the match pauses too, stands still, and plays the
        // game's pause jingle.
        let mut c = Copy::new("ws://127.0.0.1:1", "ANA");
        c.app = App::new(rom(), Some(48_000), Settings::default(), 1);
        c.app.set_versus(Some("ws://127.0.0.1:1".into()), Some("ANA"));
        c.open_versus(3);
        let mut copies = vec![c];
        run("a CPU seated", &mut copies, 10, |c| c[0].view().players[1].is_some());
        copies[0].idle(5);
        copies[0].tap("Enter", 2);
        run("playing the CPU", &mut copies, 20, |c| c[0].app.versus.playing());
        copies[0].idle(60);
        assert!(copies[0].app.pausable());
        let input = Input { held: vec![], pressed: vec![], unfocused: true };
        let mut peaks = vec![];
        for _ in 0..24 {
            let now = copies[0].start.elapsed().as_millis() as u64;
            let (_, samples) = copies[0].app.frame(&input, now);
            peaks.push(samples.iter().fold(0f32, |m, s| m.max(s.abs())));
            assert!(!copies[0].app.pausable(), "paused at once");
        }
        // As single-player's pause: quiet, then a blip every 4 frames for 16, then quiet.
        assert!(peaks[..4].iter().all(|&p| p < 0.01), "quiet first: {peaks:?}");
        assert!([4, 8, 12, 16].iter().all(|&i| peaks[i] > 0.1), "the jingle: {peaks:?}");
        assert!(peaks[20..].iter().all(|&p| p < 0.01), "then quiet: {peaks:?}");
        let frozen = copies[0].app.versus.scores();
        copies[0].idle(120);
        assert_eq!(copies[0].app.versus.scores(), frozen, "both games stand still");
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
        joiner.choose(&code);
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
        run("host sees the forfeit", &mut copies, 10, |c| c[0].view().series == ["BO LEFT"]);
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
        joiner.choose(&code);
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
        run("the forfeit", &mut only, 10, |c| c[0].view().series == ["ANA LEFT"]);
        bo = only.remove(0);
        assert_eq!(bo.view().seat, Some(0), "BO hosts now");
        assert!(bo.app.versus.in_match(), "the result is showing");
        bo.idle(1);
        bo.shot("host_left");
        // Both boards drawn: BO's own on the left, ANA's stopped one on the right.
        let lit = |x0: usize| (0..bo.height).any(|y| (x0..x0 + versus::MATCH_WIDTH / 2).any(|x| bo.last[y * bo.width + x] != 0x0F));
        assert_eq!(bo.width, versus::MATCH_WIDTH);
        assert!(lit(0) && lit(versus::MATCH_WIDTH / 2));
    }
}
