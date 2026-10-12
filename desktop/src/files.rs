//! The settings on disk: `settings.toml` in the system's settings folder, so they outlast the app
//! (and rebuilds of it); and the window's view of them.

use std::path::PathBuf;

use client::Settings;

/// `Tetris` in the system's settings folder: `~/Library/Application Support` on macOS,
/// `~/.config` (or `$XDG_CONFIG_HOME`) on Linux, `%APPDATA%` on Windows.
pub fn dir() -> PathBuf {
    dirs::config_dir().expect("no settings folder for this user").join("Tetris")
}

fn path() -> PathBuf {
    dir().join("settings.toml")
}

/// Read the file, or the defaults if there isn't one. A file that doesn't parse is kept as
/// `settings.invalid.toml` (so it isn't overwritten) and the defaults are used.
pub fn load() -> Settings {
    let path = path();
    let Ok(text) = std::fs::read_to_string(&path) else { return Settings::default() };
    match toml::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            let keep = dir().join("settings.invalid.toml");
            eprintln!("{}: {e}\nkept as {}; using defaults", path.display(), keep.display());
            let _ = std::fs::rename(&path, keep);
            Settings::default()
        }
    }
}

pub fn save(settings: &Settings) {
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(dir())?;
        let text = toml::to_string_pretty(settings).map_err(std::io::Error::other)?;
        // Write then rename, so a crash mid-write can't leave half a file.
        let tmp = dir().join("settings.toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path())
    };
    if let Err(e) = write() {
        eprintln!("saving {}: {e}", path().display());
    }
}

/// The window's view of the settings.
pub fn frontend(settings: &Settings) -> frontend::Options {
    frontend::Options {
        volume: settings.volume,
        fullscreen: settings.fullscreen,
        integer_scale: settings.integer_scale,
        width: frontend::WIDTH,
    }
}
