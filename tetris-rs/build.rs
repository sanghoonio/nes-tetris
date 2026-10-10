//! Copies your ROM into the build (`$OUT_DIR/tetris.nes`, under the gitignored `target/`) so the
//! `tetris` binary can embed it and run without a ROM path. The ROM is $TETRIS_ROM if set, else
//! `tetris_nes/tetris.nes`, and must be Tetris (USA).

use std::path::PathBuf;

/// SHA-1 of PRG + CHR (the file minus its 16-byte header) of Tetris (USA), as in `src/rom.rs`.
const BODY_SHA1: &str = "fd9079cb5e8479eb06d93c2ae5175bfce871746a";

fn main() {
    println!("cargo:rerun-if-env-changed=TETRIS_ROM");
    let path = std::env::var_os("TETRIS_ROM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tetris.nes"));
    println!("cargo:rerun-if-changed={}", path.display());
    let file = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("the game is built with your ROM: can't read {}: {e}", path.display()));
    let hash = sha1_smol::Sha1::from(file.get(16..).unwrap_or_default()).digest().to_string();
    assert_eq!(hash, BODY_SHA1, "{} isn't Tetris (USA)", path.display());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("tetris.nes");
    std::fs::write(&out, &file).expect("write the ROM into OUT_DIR");
}
