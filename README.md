# NES Tetris → Rust

Two native ports of NES Tetris (USA), both needing your own ROM at run time (`tetris.nes` here, or
pass a path). Nothing from the ROM is in this repository.

    cargo run --release -p tetris-rs --bin tetris          # readable reimplementation
    cargo run --release -p recomp --bin tetris-recomp      # static recompile

Keys: arrows, X = A (rotate clockwise), Z = B (anticlockwise), Enter = Start, Right Shift =
Select, Escape = quit.

## What's here

| Crate | What |
|---|---|
| `recomp/` | Static recompile: `gen/gen.py` translates the ROM's 6502 code into `recomp/generated/tetris.rs` (gitignored, generated from your ROM), run on a small NES model (CPU ops, MMC1, joypads, PPU timing). |
| `tetris-rs/` | The game rewritten as readable Rust, routine by routine, keeping the original's variables (`vars.rs`) and hardware (`hw.rs`) so it can be checked against the recompile. The waits for vblank are `async`, so the game runs a frame per poll with no threads, and the library builds for `wasm32-unknown-unknown`. |
| `ppu/`, `apu/` | NES video and sound models shared by both ports. |
| `frontend/` | Window, keyboard and audio, shared by both binaries. |
| `disasm/` | (gitignored) the community disassembly the work is based on. |

## How it's checked

Each layer is checked against the one below it:

1. **Recompile vs the original ROM** (`recomp/tests/oracle.rs`): both run in lockstep with
   [tetanes-core](https://crates.io/crates/tetanes-core) running the ROM. All of RAM and the
   visible picture are compared at every frame, over attract mode, random play, level 29, a
   1080-line run and every ending.
2. **Each readable routine vs the recompile** (`tetris-rs/tests/shadow.rs`): every call the game
   makes to a rewritten routine during those runs is replayed through the readable version and
   compared (variables, PPU, mapper, joypads, APU writes, return value).
3. **The whole readable game vs the recompile** (`tetris-rs/tests/frames.rs`): every named variable
   and the picture at every frame.

Run them with `cargo test` (about two minutes). Setting up from a fresh clone:

    # ROM at tetris_nes/tetris.nes, then:
    git clone --depth 1 https://github.com/CelestialAmber/TetrisNESDisasm.git disasm
    (cd disasm && git submodule update --init --depth 1 && \
        make compare nesChrEncode="uv run --quiet --with pillow python3 tools/nes-util/nes_chr_encode.py")
    python3 recomp/gen/gen.py
    python3 tetris-rs/gen/layout.py   # only if the disassembly's labels change

`tetris-rs/workers/` holds the instructions and setup script given to the AI workers that rewrote
many of the routines; `plans/2026-10-10-nes-tetris-rust.md` (in the parent folder) has the plan and
the log.
