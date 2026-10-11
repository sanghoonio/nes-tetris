//! Extracts what the game uses from your ROM, its data and graphics but not its code
//! (`src/rom_extract.rs`), into the build (`$OUT_DIR/rom_data.bin`, under the gitignored
//! `target/`), so the `tetris` binary can embed it and run without a ROM path. The ROM is
//! $TETRIS_ROM if set, else `nes-tetris/tetris.nes`, and must be Tetris (USA). Only with the
//! `embedded-rom` feature (on by default); without it the package builds with no ROM.

#[path = "src/rom_data.rs"]
mod rom_data;
#[allow(dead_code)]
#[path = "src/rom_extract.rs"]
mod rom_extract;

use std::path::PathBuf;

/// SHA-1 of PRG + CHR (the file minus its 16-byte header) of Tetris (USA), as in `src/rom.rs`.
const BODY_SHA1: &str = "fd9079cb5e8479eb06d93c2ae5175bfce871746a";

fn main() {
    if std::env::var_os("CARGO_FEATURE_EMBEDDED_ROM").is_none() {
        return;
    }
    println!("cargo:rerun-if-env-changed=TETRIS_ROM");
    println!("cargo:rerun-if-changed=src/rom_data.rs");
    println!("cargo:rerun-if-changed=src/rom_extract.rs");
    let path = std::env::var_os("TETRIS_ROM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"));
    println!("cargo:rerun-if-changed={}", path.display());
    let file = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("the game is built with your ROM: can't read {}: {e}", path.display()));
    let hash = sha1_smol::Sha1::from(file.get(16..).unwrap_or_default()).digest().to_string();
    assert_eq!(hash, BODY_SHA1, "{} isn't Tetris (USA)", path.display());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("rom_data.bin");
    std::fs::write(&out, rom_extract::extract(&file[16..])).expect("write the ROM's data into OUT_DIR");
}
