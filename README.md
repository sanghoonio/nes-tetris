# NES Tetris → Rust

Two native ports of NES Tetris (USA), both needing your own ROM (`tetris.nes` here). Nothing from
the ROM is in this repository.

    tetris-rs/macos/bundle.sh --install                    # Tetris.app in ~/Applications
    cargo run --release -p tetris-rs --bin tetris          # readable reimplementation
    cargo run --release -p recomp --bin tetris-recomp      # static recompile

The readable game's binary embeds the ROM at build time (`tetris-rs/build.rs`, from `$TETRIS_ROM`
or `tetris.nes`; the copy stays under the gitignored `target/`), so the app runs on its own. Without
`--install` the app is left at `target/Tetris.app`. The recompile reads the ROM at run time.

The app's title screen has START / SETTINGS / EXIT (no attract demo). SETTINGS has three pages
in the game's own style: CONTROLS (every key, Back included), GAME (DAS delay and repeat,
Left+Right handling, the Down+Left+Right slowdown, hard drop on Up, ghost piece, saving high
scores) and SYSTEM (volume, fullscreen, pixel scale, reset). Every rule's default is the
original game. Settings, the last game's choices and the high scores are kept in
`~/Library/Application Support/Tetris/settings.toml`, written on first launch and saved as
they change. Escape is Back, not quit; quit with EXIT, Cmd+Q or by closing the window.

Default keys: arrows, X = A (rotate clockwise), Z = B (anticlockwise), Enter = Start, Right
Shift = Select, Escape = Back. Gamepads use the usual emulator layout (d-pad or left stick,
east button = A, south = B, Start, Select); only keys are remappable.

## What's here

| Crate | What |
|---|---|
| `recomp/` | Static recompile: `gen/gen.py` translates the ROM's 6502 code into `recomp/generated/tetris.rs` (gitignored, generated from your ROM), run on a small NES model (CPU ops, MMC1, joypads, PPU timing). |
| `tetris-rs/` | The game rewritten as readable Rust, routine by routine, keeping the original's variables (`vars.rs`) and hardware (`hw.rs`) so it can be checked against the recompile. The waits for vblank are `async`, so the game runs a frame per poll with no threads, and the library builds for `wasm32-unknown-unknown`. |
| `ppu/`, `apu/` | NES video and sound models shared by both ports. |
| `frontend/` | Window, keyboard, gamepads and audio, shared by both binaries. |
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

The app's additions (`tetris-rs/src/options.rs`, `pc_menus.rs`) are off in the runs above, which
are the original game. `tests/frames.rs` also runs with the title menu on (START chosen at once)
and checks that everything after the title still matches; `tests/menus.rs` and
`tests/rules.rs` check the menus and each rule change by behaviour.

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
