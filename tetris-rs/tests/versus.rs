//! The VERSUS screens (with the `versus` feature): reached from the title menu, driven by keys,
//! fed a `VersusView` as the app would, and checked by the requests they make. Each screen is
//! also saved as a picture in `target/versus/`.
//!
//!     cargo test -p tetris-rs --features versus --test versus
#![cfg(feature = "versus")]

use std::sync::Arc;
use tetris_rs::versus::{Connection, RoomSettings, VersusRequest, VersusView};
use tetris_rs::{Config, FrameOut, Keys, Options, Rom, Runner, Vars};

struct App {
    runner: Runner,
    view: VersusView,
    requests: Vec<VersusRequest>,
    last: Option<FrameOut>,
}

impl App {
    fn new() -> Self {
        let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tetris.nes")).unwrap();
        let options = Options { menus: true, config: Config::default() };
        let runner = Runner::with_options(Arc::new(Rom::new(&rom).unwrap()), None, None, options);
        App { runner, view: VersusView { name: "ANA".into(), ..Default::default() }, requests: vec![], last: None }
    }

    fn frame(&mut self, held: &[&str], pressed: &[&str]) {
        let keys = Keys { held: held.iter().map(|s| s.to_string()).collect(), pressed: pressed.iter().map(|s| s.to_string()).collect(), ..Default::default() };
        let out = self.runner.next_frame_versus(keys, self.view.clone());
        self.requests.extend(out.versus.iter().cloned());
        self.last = Some(out);
    }

    fn idle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.frame(&[], &[]);
        }
    }

    fn tap(&mut self, key: &str, frames: usize) {
        self.frame(&[key], &[key]);
        self.idle(frames);
    }

    fn typed(&mut self, text: &str) {
        for c in text.chars() {
            let key = match c {
                ' ' => "Space".to_string(),
                c if c.is_ascii_digit() => format!("Digit{c}"),
                c => format!("Key{c}"),
            };
            self.tap(&key, 2);
        }
    }

    fn mode(&self) -> u8 {
        Vars::from_ram(&self.last.as_ref().unwrap().ram).game_mode
    }

    /// The title menu, with VERSUS on it.
    fn at_title() -> Self {
        let mut app = App::new();
        app.idle(10);
        app.tap("Enter", 20);
        assert_eq!(app.mode(), 1);
        app
    }

    fn take(&mut self) -> Vec<VersusRequest> {
        std::mem::take(&mut self.requests)
    }

    /// Save the picture as `target/versus/<name>.png`.
    fn shot(&self, name: &str) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/versus");
        std::fs::create_dir_all(&dir).unwrap();
        let out = self.last.as_ref().unwrap();
        let mut ppm = b"P6 256 240 255\n".to_vec();
        for &p in out.pixels.iter() {
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
}

/// Title → VERSUS menu (Down once to VERSUS).
fn at_versus_menu() -> App {
    let mut app = App::at_title();
    app.tap("ArrowDown", 3);
    app.tap("Enter", 20);
    app
}

fn in_room(seat: Option<u8>) -> VersusView {
    VersusView {
        name: "ANA".into(),
        connection: Connection::InRoom,
        code: "KQXR".into(),
        seat,
        players: [Some(("ANA".into(), true)), Some(("BO".into(), false))],
        watching: 2,
        settings: RoomSettings::default(),
        status: "WAITING FOR BOTH TO READY".into(),
        result: "BO WON 12000 TO 9400".into(),
    }
}

#[test]
fn title_has_versus() {
    let mut app = App::at_title();
    app.idle(3);
    app.shot("title");
    // Down to VERSUS, Enter: the VERSUS menu, no request yet.
    app.tap("ArrowDown", 3);
    app.tap("Enter", 20);
    app.shot("versus_menu");
    assert!(app.take().is_empty());
    // BACK (Escape) returns to the title, cursor still on VERSUS.
    app.tap("Escape", 20);
    assert_eq!(app.mode(), 1);
    app.tap("Enter", 20);
    app.tap("Escape", 20);
    // EXIT is now four down.
    for _ in 0..2 {
        app.tap("ArrowDown", 3);
    }
    app.tap("Enter", 2);
    assert!(app.last.as_ref().unwrap().quit);
}

/// SETTINGS has BACK on each page (the picture is for looking at).
#[test]
fn settings_back() {
    let mut app = App::at_title();
    app.tap("ArrowDown", 3);
    app.tap("ArrowDown", 3);
    app.tap("Enter", 20);
    app.tap("ArrowDown", 30);
    for _ in 0..7 {
        app.tap("ArrowDown", 3);
    }
    app.shot("settings_back");
    app.tap("Enter", 30);
    assert_eq!(app.mode(), 1, "BACK to the title");
}

/// MUSIC on the SYSTEM page sets the game's music choice and plays it until the panel is left.
#[test]
fn settings_music() {
    let mut app = App::at_title();
    app.tap("ArrowDown", 3);
    app.tap("ArrowDown", 3);
    app.tap("Enter", 20);
    // CONTROLS' page link, then GAME's: SYSTEM.
    for _ in 0..7 {
        app.tap("ArrowDown", 3);
    }
    app.tap("Enter", 5);
    for _ in 0..8 {
        app.tap("ArrowDown", 3);
    }
    app.tap("Enter", 5);
    app.tap("ArrowDown", 3);
    let vars = |app: &App| Vars::from_ram(&app.last.as_ref().unwrap().ram);
    assert_eq!(vars(&app).music_type, 0);
    app.tap("ArrowRight", 20);
    assert_eq!(vars(&app).music_type, 1);
    assert_ne!(vars(&app).currently_playing_music_track, 0, "music 2 plays");
    app.tap("ArrowLeft", 3);
    app.tap("ArrowLeft", 20);
    assert_eq!(vars(&app).music_type, 3, "wraps to OFF");
    app.tap("ArrowLeft", 20);
    app.shot("settings_system");
    app.tap("Escape", 30);
    assert_eq!(app.mode(), 1);
    assert_eq!(vars(&app).music_type, 2, "the choice stays");
    assert_eq!(vars(&app).currently_playing_music_track, 0, "and the title is quiet");
}

#[test]
fn naming() {
    let mut app = at_versus_menu();
    for _ in 0..3 {
        app.tap("ArrowDown", 3);
    }
    app.tap("Enter", 3);
    app.tap("Backspace", 2);
    app.tap("Backspace", 2);
    app.tap("Backspace", 2);
    app.typed("ZOE 99");
    // The cursor: an underline after the name (row 14, column 19 + 6), in the letters' white,
    // flickering as the arrows do.
    let pixels = |app: &App| app.last.as_ref().unwrap().pixels.clone();
    let white = 0x30;
    assert!(pixels(&app)[14 * 8 * 256 + 19 * 8..][..8].contains(&white), "the name is in white");
    let cursor = |app: &App| pixels(app)[(14 * 8 + 7) * 256 + 25 * 8..][..7].iter().all(|&p| p == white);
    let shown: Vec<bool> = (0..32)
        .map(|_| {
            app.idle(1);
            cursor(&app)
        })
        .collect();
    assert_eq!(shown.iter().filter(|&&s| s).count(), 24, "shown 3 frames in 4, as the arrows: {shown:?}");
    while !cursor(&app) {
        app.idle(1);
    }
    app.shot("naming");
    app.tap("Enter", 3);
    assert_eq!(app.take(), vec![VersusRequest::SetName("ZOE 99".into())]);
}

#[test]
fn hosting_and_the_lobby() {
    let mut app = at_versus_menu();
    app.tap("Enter", 2);
    assert_eq!(app.take(), vec![VersusRequest::Host]);
    app.view.connection = Connection::Connecting;
    app.idle(20);
    app.shot("lobby_connecting");
    app.view = in_room(Some(0));
    app.idle(5);
    app.shot("lobby_host");
    // The host's cursor starts on READY; ROOM SETTINGS is above it.
    app.tap("Enter", 2);
    assert_eq!(app.take(), vec![VersusRequest::Ready(false)], "ANA is ready: Enter unreadies");
    app.tap("ArrowUp", 2);
    app.tap("Enter", 5);
    app.shot("room_settings_host");
    app.tap("ArrowRight", 2);
    let level = RoomSettings { level: 19, ..RoomSettings::default() };
    assert_eq!(app.take(), vec![VersusRequest::Settings(level)]);
    app.tap("ArrowDown", 2);
    app.tap("Enter", 2);
    // Changes build on each other before the room confirms them.
    assert_eq!(app.take(), vec![VersusRequest::Settings(RoomSettings { level: 19, identical: false, ..RoomSettings::default() })]);
    // Escape: back to the lobby's main page, then out of the room.
    app.tap("Escape", 5);
    assert!(app.take().is_empty());
    app.tap("Escape", 20);
    assert_eq!(app.take(), vec![VersusRequest::Leave]);
    app.tap("Enter", 2);
    assert_eq!(app.take(), vec![VersusRequest::Host], "the VERSUS menu again");
}

#[test]
fn joining_by_code() {
    let mut app = at_versus_menu();
    app.tap("ArrowDown", 3);
    app.tap("Enter", 20);
    // I and O aren't in codes; Enter needs all four letters.
    app.typed("KIQ");
    app.tap("Enter", 2);
    assert!(app.take().is_empty());
    app.typed("OXR");
    app.shot("join_code");
    app.tap("Enter", 2);
    assert_eq!(app.take(), vec![VersusRequest::Join("KQXR".into())]);
    app.view = in_room(Some(1));
    app.idle(20);
    app.shot("lobby_opponent");
    app.tap("Enter", 2);
    assert_eq!(app.take(), vec![VersusRequest::Ready(true)]);
    // The opponent sees the room's settings but can't change them: only BACK.
    app.tap("ArrowUp", 2);
    app.tap("Enter", 5);
    app.shot("room_settings_opponent");
    app.tap("ArrowUp", 2);
    app.tap("ArrowRight", 2);
    assert!(app.take().is_empty());
    app.tap("Enter", 5);
    assert!(app.take().is_empty(), "BACK to the lobby, still in the room");
    app.tap("Escape", 5);
    assert_eq!(app.take(), vec![VersusRequest::Leave]);
}

#[test]
fn watching() {
    let mut app = at_versus_menu();
    app.tap("ArrowDown", 3);
    app.tap("ArrowDown", 3);
    app.tap("Enter", 20);
    app.typed("KQXR");
    app.tap("Enter", 2);
    assert_eq!(app.take(), vec![VersusRequest::Watch("KQXR".into())]);
    app.view = in_room(None);
    app.idle(20);
    app.shot("lobby_watching");
    // The cursor starts on LEAVE.
    app.tap("Enter", 2);
    assert_eq!(app.take(), vec![VersusRequest::Leave]);
}
