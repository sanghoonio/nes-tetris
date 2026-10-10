# Worker instructions

You are rewriting routines from the NES Tetris disassembly (`disasm/main.asm`) as readable Rust in
`tetris-rs`. Every call the original game makes to a routine is replayed through your version and
compared against the original (the shadow oracle). A routine is done when every call matches.

## Setup (once)

    bash tetris-rs/workers/setup.sh

## Loop

1. Read the routine in `disasm/main.asm` (`grep -n '^routineName' disasm/main.asm`). Labels starting
   with `@` are local to the routine above them.
2. Write it in your assigned file and add it to that file's `SHADOW` list.
3. Run the oracle for your routines (comma-separated names, as in the disassembly):

       ROUTINE=nameA,nameB cargo test -q -p tetris-rs --test shadow -- --nocapture

   Each routine prints `matched/total calls match`, then up to 3 mismatches: which variable
   differs (ours vs original) and the registers at entry.
4. Fix the first mismatch, rerun. Stop after 8 oracle runs that still show mismatches and report
   what you tried.

## Rules

- Edit only your assigned file. Never edit `tests/`, `vars.rs`, `hw.rs`, `shadow.rs`, `lib.rs`,
  `rom.rs`, `layout.rs`, `recomp/`, `disasm/` or the ROM. If you need something changed there
  (a new type, a helper), say so in your report.
- Reproduce the original exactly, quirks included: 8-bit wraparound (`wrapping_add`), signed
  compares (`cmp` then `bmi`/`bpl` → compare `x.wrapping_sub(y) as i8`), carries that leak from one
  add into the next. Comment each quirk in one line.
- Read data tables from the ROM (`self.rom.byte(layout::SOME_TABLE + i)`); never copy table
  contents into the code.
- `scratch` lists name variables the routine uses only as temporaries (written before read, never
  read after it returns). Justify each one in a comment. Never add a variable there to hide a
  real mismatch.
- No special cases for particular inputs, nothing the original doesn't do.
- Use the names from the disassembly in snake_case for functions (`shift_tetrimino`), and say in
  the doc comment which original routine it is.

## Style

`tetris-rs/src/controls.rs` is the worked example: a doc comment saying what the routine does in
game terms, plain control flow, named constants instead of magic numbers, the hardware through
`Game` methods (`self.ppu_data(..)`, `self.apu_write(..)`, `self.set_chr_bank0(..)`).

## Report

End with: the files you changed, the final oracle line for each routine, quirks you found, and
anything you couldn't finish.
