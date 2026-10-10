#!/usr/bin/env bash
# Prepare a git worktree of tetris_nes for a worker: link in the untracked inputs (the ROM and
# the disassembly) from the main checkout and generate the recompile.
set -euo pipefail
MAIN="${TETRIS_MAIN:-/Users/sam/Documents/Projects/tetris/tetris_nes}"
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
[ "$ROOT" = "$MAIN" ] || {
    ln -sfn "$MAIN/tetris.nes" tetris.nes
    ln -sfn "$MAIN/disasm" disasm
}
python3 recomp/gen/gen.py
echo "ready: $ROOT"
