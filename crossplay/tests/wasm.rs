//! The same games natively and as WebAssembly under Node: every checksum must agree. Builds this
//! crate for `wasm32-unknown-unknown` (with no ROM; it's handed over at runtime) into its own
//! target directory, and needs `node` on the PATH.

use std::path::PathBuf;
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rom_path() -> PathBuf {
    std::env::var_os("TETRIS_ROM").map(PathBuf::from).unwrap_or_else(|| manifest_dir().join("../tetris.nes"))
}

const NODE: &str = r#"
const fs = require("fs");
const [wasmPath, romPath, inputsPath, seed, level, identical] = process.argv.slice(2);
const instance = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(wasmPath)), {});
const e = instance.exports;
const put = (bytes) => { const p = e.alloc(bytes.length); new Uint8Array(e.memory.buffer, p, bytes.length).set(bytes); return p; };
const rom = fs.readFileSync(romPath), inputs = fs.readFileSync(inputsPath);
const out = e.replay(put(rom), rom.length, put(inputs), inputs.length, Number(seed), Number(level), Number(identical));
const n = Math.floor(inputs.length / 60);
const sums = new BigUint64Array(e.memory.buffer, out, n);
console.log(Array.from(sums, (x) => x.toString(16)).join(" "));
"#;

#[test]
fn native_and_wasm_agree() {
    let target = manifest_dir().join("../target/crossplay");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "-q", "-p", "crossplay", "--target", "wasm32-unknown-unknown", "--target-dir"])
        .arg(&target)
        .env("TETRIS_ROM", "/nonexistent")
        .status()
        .expect("cargo");
    assert!(status.success(), "wasm build");
    let wasm = target.join("wasm32-unknown-unknown/release/crossplay.wasm");
    let rom = std::fs::read(rom_path()).expect("ROM");

    // Random play: a new choice every 8 frames.
    let mut rng = 0x1234_5678_9ABC_DEF1u64;
    let inputs: Vec<u8> = (0..6000)
        .scan(0u8, |held, f| {
            if f % 8 == 0 {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                *held = [0, 0, 0x04, 0x02, 0x01, 0x80, 0x40, 0x02, 0x01][(rng % 9) as usize];
            }
            Some(*held)
        })
        .collect();
    let dir = target.join("node");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("replay.js"), NODE).unwrap();
    std::fs::write(dir.join("inputs.bin"), &inputs).unwrap();

    for (seed, level, identical) in [(42u32, 0u8, false), (0xBEEF_0001, 9, true), (7, 18, false)] {
        let native: Vec<String> = crossplay::checksums(&rom, &inputs, seed, level, identical).iter().map(|x| format!("{x:x}")).collect();
        let out = Command::new("node")
            .arg(dir.join("replay.js"))
            .arg(&wasm)
            .arg(rom_path())
            .arg(dir.join("inputs.bin"))
            .args([seed.to_string(), level.to_string(), (identical as u8).to_string()])
            .output()
            .expect("node");
        assert!(out.status.success(), "node: {}", String::from_utf8_lossy(&out.stderr));
        let wasm_sums: Vec<String> = String::from_utf8(out.stdout).unwrap().split_whitespace().map(String::from).collect();
        assert_eq!(native.len(), 100);
        assert_eq!(native, wasm_sums, "seed {seed}, level {level}");
    }
}
