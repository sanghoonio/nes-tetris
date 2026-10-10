#!/usr/bin/env python3
"""Static recompiler: NES Tetris 6502 code -> Rust.

Inputs:
  - the ROM (code bytes are read from it, so the output is exactly what the cartridge runs)
  - disasm/tetris.dbg from the reassembled disassembly (labels, and which addresses the
    source marks as instructions vs data)

Output: recomp/generated/tetris.rs, one Rust fn per call target. Each fn is a loop over a
`match pc` with one arm per reachable instruction, so fall-through, shared tails and jumps into
the middle of other routines all just work (code reachable from several entries is duplicated).

    python3 recomp/gen/gen.py [--rom tetris.nes] [--dbg disasm/tetris.dbg] [--out recomp/generated/tetris.rs]
"""

import argparse
import os
import re
import sys
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, "..", ".."))

# opcode -> (mnemonic, mode, length). Modes match recomp/src/cpu.rs.
MODES_LEN = {"imp": 1, "acc": 1, "imm": 2, "zp": 2, "zpx": 2, "zpy": 2, "abs": 3, "absx": 3,
             "absy": 3, "ind": 3, "indx": 2, "indy": 2, "rel": 2}
OPS = {}
for spec in """
69 adc imm 65 adc zp 75 adc zpx 6D adc abs 7D adc absx 79 adc absy 61 adc indx 71 adc indy
29 and imm 25 and zp 35 and zpx 2D and abs 3D and absx 39 and absy 21 and indx 31 and indy
0A asl acc 06 asl zp 16 asl zpx 0E asl abs 1E asl absx
90 bcc rel B0 bcs rel F0 beq rel 30 bmi rel D0 bne rel 10 bpl rel 50 bvc rel 70 bvs rel
24 bit zp 2C bit abs 00 brk imp 18 clc imp D8 cld imp 58 cli imp B8 clv imp
C9 cmp imm C5 cmp zp D5 cmp zpx CD cmp abs DD cmp absx D9 cmp absy C1 cmp indx D1 cmp indy
E0 cpx imm E4 cpx zp EC cpx abs C0 cpy imm C4 cpy zp CC cpy abs
C6 dec zp D6 dec zpx CE dec abs DE dec absx CA dex imp 88 dey imp
49 eor imm 45 eor zp 55 eor zpx 4D eor abs 5D eor absx 59 eor absy 41 eor indx 51 eor indy
E6 inc zp F6 inc zpx EE inc abs FE inc absx E8 inx imp C8 iny imp
4C jmp abs 6C jmp ind 20 jsr abs
A9 lda imm A5 lda zp B5 lda zpx AD lda abs BD lda absx B9 lda absy A1 lda indx B1 lda indy
A2 ldx imm A6 ldx zp B6 ldx zpy AE ldx abs BE ldx absy
A0 ldy imm A4 ldy zp B4 ldy zpx AC ldy abs BC ldy absx
4A lsr acc 46 lsr zp 56 lsr zpx 4E lsr abs 5E lsr absx EA nop imp
09 ora imm 05 ora zp 15 ora zpx 0D ora abs 1D ora absx 19 ora absy 01 ora indx 11 ora indy
48 pha imp 08 php imp 68 pla imp 28 plp imp
2A rol acc 26 rol zp 36 rol zpx 2E rol abs 3E rol absx
6A ror acc 66 ror zp 76 ror zpx 6E ror abs 7E ror absx
40 rti imp 60 rts imp
E9 sbc imm E5 sbc zp F5 sbc zpx ED sbc abs FD sbc absx F9 sbc absy E1 sbc indx F1 sbc indy
38 sec imp F8 sed imp 78 sei imp
85 sta zp 95 sta zpx 8D sta abs 9D sta absx 99 sta absy 81 sta indx 91 sta indy
86 stx zp 96 stx zpy 8E stx abs 84 sty zp 94 sty zpx 8C sty abs
AA tax imp A8 tay imp BA tsx imp 8A txa imp 9A txs imp 98 tya imp
""".split("\n"):
    t = spec.split()
    for i in range(0, len(t), 3):
        OPS[int(t[i], 16)] = (t[i + 1], t[i + 2], MODES_LEN[t[i + 2]])
MNEMONICS = {m for m, _, _ in OPS.values()}
BRANCHES = {"bcc", "bcs", "beq", "bmi", "bne", "bpl", "bvc", "bvs"}


def parse_dbg(path):
    recs = defaultdict(list)
    with open(path) as f:
        for line in f:
            kind, _, rest = line.rstrip("\n").partition("\t")
            fields = {}
            for m in re.finditer(r'(\w+)=("([^"]*)"|[^,]*)', rest):
                fields[m.group(1)] = m.group(3) if m.group(3) is not None else m.group(2)
            recs[kind].append(fields)
    return recs


class Program:
    def __init__(self, rom, dbg_path):
        self.prg = rom[16:16 + 0x8000]
        dbg = parse_dbg(dbg_path)
        disasm_dir = os.path.dirname(dbg_path)
        segs = {int(s["id"]): s for s in dbg["seg"]}
        spans = {int(s["id"]): s for s in dbg["span"]}
        files = {int(f["id"]): f["name"] for f in dbg["file"]}
        sources = {}

        def source_line(file_id, n):
            if file_id not in sources:
                with open(os.path.join(disasm_dir, files[file_id]), errors="replace") as fh:
                    sources[file_id] = fh.read().split("\n")
            lines = sources[file_id]
            return lines[n - 1] if 0 < n <= len(lines) else ""

        # Address -> source text, and which addresses the source says are instructions / .addr.
        self.source = {}
        self.instr = set()
        self.addr_tables = []  # (address, byte length) of .addr/.word data
        for ln in dbg["line"]:
            if "span" not in ln or ln.get("type"):  # skip macro-expansion records
                continue
            name = files[int(ln["file"])]
            if not name.endswith(".asm"):
                continue
            for sid in ln["span"].split("+"):
                sp = spans[int(sid)]
                seg = segs[int(sp["seg"])]
                if not seg["name"].startswith(("PRG", "unreferenced", "VECTORS")):
                    continue
                addr = int(seg["start"], 16) + int(sp["start"])
                size = int(sp["size"])
                text = source_line(int(ln["file"]), int(ln["line"]))
                body = re.sub(r";.*", "", text)
                body = re.sub(r"^\s*[@\w]+:", "", body).strip()
                tok = body.split()[0].lower() if body else ""
                if tok in MNEMONICS:
                    self.instr.add(addr)
                    self.source[addr] = text.strip()
                elif tok in (".addr", ".word"):
                    self.addr_tables.append((addr, size))

        # Labels: prefer global names over cheap locals at the same address.
        self.labels = {}
        for s in dbg["sym"]:
            if s.get("type") != "lab" or "val" not in s:
                continue
            a = int(s["val"], 16)
            if a < 0x8000:
                continue
            if a not in self.labels or (self.labels[a].startswith("@") and not s["name"].startswith("@")):
                self.labels[a] = s["name"]

        self.warnings = []

    def byte(self, a):
        return self.prg[a - 0x8000]

    def word(self, a):
        return self.byte(a) | self.byte(a + 1) << 8

    def decode(self, a):
        op = self.byte(a)
        if op not in OPS:
            raise ValueError(f"illegal opcode {op:02X} at {a:04X}")
        mnem, mode, length = OPS[op]
        operand = 0
        if length == 2:
            operand = self.byte(a + 1)
        elif length == 3:
            operand = self.word(a + 1)
        return op, mnem, mode, length, operand

    def seeds(self):
        """Entry points: interrupt vectors plus every .addr/.word value that points at code."""
        out = {self.word(0xFFFA): "vector", self.word(0xFFFC): "vector", self.word(0xFFFE): "vector"}
        for addr, size in self.addr_tables:
            for i in range(0, size, 2):
                v = self.word(addr + i)
                if v in self.instr:
                    out.setdefault(v, "table")
        return out

    def continuation_after_call(self, a, callee):
        """Where execution resumes after `jsr callee` at `a`, or None if it never does locally.

        Plain calls resume at a+3. Some callees consume inline data after the jsr (they rewrite
        their return address); resume at the next instruction the source marks. The jump-table
        helper never returns to its caller's body at all.
        """
        if self.labels.get(callee) == "switch_s_plus_2a":
            return None
        nxt = a + 3
        if nxt in self.instr:
            return nxt
        for skip in range(1, 5):
            if nxt + skip in self.instr:
                return nxt + skip
        self.warnings.append(f"no continuation after jsr at {a:04X}")
        return None

    def explore(self, entry):
        """All instructions reachable from `entry` without entering callees."""
        seen = {}
        work = [entry]
        callees = set()
        while work:
            a = work.pop()
            if a in seen:
                continue
            if a not in self.instr:
                self.warnings.append(f"decoding {a:04X}, which the source doesn't mark as an instruction")
            ins = self.decode(a)
            seen[a] = ins
            op, mnem, mode, length, operand = ins
            nxt = (a + length) & 0xFFFF
            if mnem in BRANCHES:
                target = (nxt + (operand - 256 if operand >= 128 else operand)) & 0xFFFF
                work += [nxt, target]
            elif mnem == "jmp" and mode == "abs":
                work.append(operand)
            elif mnem == "jsr":
                callees.add(operand)
                cont = self.continuation_after_call(a, operand)
                if cont is not None:
                    work.append(cont)
            elif mnem in ("rts", "rti", "brk") or (mnem == "jmp" and mode == "ind"):
                pass
            else:
                work.append(nxt)
        return seen, callees


def fn_name(prog, a):
    label = prog.labels.get(a, "")
    label = re.sub(r"\W", "_", label.lstrip("@"))
    return f"f_{a:04x}" + (f"_{label}" if label else "")


def rust_mode(mode):
    return {"imp": "Imp", "acc": "Acc"}.get(mode)


def emit_instruction(prog, a, ins, out):
    op, mnem, mode, length, operand = ins
    nxt = (a + length) & 0xFFFF
    src = prog.source.get(a, "")
    label = prog.labels.get(a)
    comment = f"  // {label}: {src}" if label and not src.startswith(label) else f"  // {src}"
    out.append(f"            0x{a:04X} => {{{comment}")
    if mnem in BRANCHES:
        target = (nxt + (operand - 256 if operand >= 128 else operand)) & 0xFFFF
        out.append(f"                pc = if m.branch(0x{op:02X}, 0x{nxt:04X}, 0x{target:04X}) {{ 0x{target:04X} }} else {{ 0x{nxt:04X} }};")
    elif mnem == "jmp" and mode == "abs":
        out.append(f"                m.tick(3);")
        out.append(f"                pc = 0x{operand:04X};")
    elif mnem == "jmp":
        out.append(f"                let target = m.jmp_ind(0x{operand:04X});")
        out.append(f"                call(m, target);")
        out.append(f"                return;")
    elif mnem == "jsr":
        out.append(f"                let sp0 = m.sp;")
        out.append(f"                m.jsr(0x{a:04X});")
        out.append(f"                {fn_name(prog, operand)}(m);")
        out.append(f"                if m.unwound(sp0) {{ return; }}")
        out.append(f"                pc = m.ret_to;")
    elif mnem == "rts":
        out.append(f"                m.rts();")
        out.append(f"                return;")
    elif mnem == "rti":
        out.append(f"                m.rti();")
        out.append(f"                return;")
    elif mnem == "brk":
        out.append(f"                panic!(\"brk at {a:04X}\");")
    else:
        out.append(f"                m.exec(0x{op:02X}, 0x{operand:04X});")
        out.append(f"                pc = 0x{nxt:04X};")
    out.append("            }")


def generate(prog):
    entries = dict(prog.seeds())
    bodies = {}
    work = list(entries)
    while work:
        e = work.pop()
        if e in bodies:
            continue
        seen, callees = prog.explore(e)
        bodies[e] = seen
        for c in callees:
            entries.setdefault(c, "call")
            work.append(c)

    out = [
        "// @generated by recomp/gen/gen.py from the ROM and disasm/tetris.dbg. Do not edit.",
        "// Derived from the ROM's code: keep out of version control and anything shared.",
        "#![allow(unused_variables, unreachable_code, clippy::all)]",
        "use crate::machine::Machine;",
        "",
        "/// Call the routine at `addr`: a recompiled fn if there is one, else the interpreter.",
        "pub fn call(m: &mut Machine, addr: u16) {",
        "    match addr {",
    ]
    for e in sorted(bodies):
        out.append(f"        0x{e:04X} => {fn_name(prog, e)}(m),")
    out += ["        _ => crate::interp::interpret(m, addr),", "    }", "}", ""]
    out.append("/// Every recompiled entry point, for tests and the routine oracle.")
    out.append("pub const ENTRIES: &[(u16, &str)] = &[")
    for e in sorted(bodies):
        out.append(f"    (0x{e:04X}, \"{prog.labels.get(e, '')}\"),")
    out += ["];", ""]

    for e in sorted(bodies):
        seen = bodies[e]
        label = prog.labels.get(e, "")
        out.append(f"/// {label or 'unnamed'} (${e:04X}), {len(seen)} instructions reachable")
        out.append(f"pub fn {fn_name(prog, e)}(m: &mut Machine) {{")
        out.append(f"    if m.watched(0x{e:04X}) {{")
        out.append(f"        crate::shadow::watch(m, 0x{e:04X}, {fn_name(prog, e)}_body);")
        out.append(f"    }} else {{")
        out.append(f"        {fn_name(prog, e)}_body(m);")
        out.append(f"    }}")
        out.append(f"}}")
        out.append("")
        out.append(f"fn {fn_name(prog, e)}_body(m: &mut Machine) {{")
        out.append(f"    let mut pc: u16 = 0x{e:04X};")
        out.append("    loop {")
        out.append("        m.poll(pc);")
        out.append("        match pc {")
        for a in sorted(seen):
            emit_instruction(prog, a, seen[a], out)
        out.append("            _ => { crate::interp::interpret_inline(m, pc); return; }")
        out.append("        }")
        out.append("    }")
        out.append("}")
        out.append("")
    return "\n".join(out), bodies


def main():
    ap = argparse.ArgumentParser(description="Recompile NES Tetris to Rust")
    ap.add_argument("--rom", default=os.environ.get("TETRIS_ROM", os.path.join(REPO, "tetris.nes")))
    ap.add_argument("--dbg", default=os.path.join(REPO, "disasm", "tetris.dbg"))
    ap.add_argument("--out", default=os.path.join(REPO, "recomp", "generated", "tetris.rs"))
    args = ap.parse_args()

    with open(args.rom, "rb") as f:
        rom = f.read()
    prog = Program(rom, args.dbg)
    code, bodies = generate(prog)
    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, "w") as f:
        f.write(code)
    total = sum(len(b) for b in bodies.values())
    unique = len(set().union(*[set(b) for b in bodies.values()]))
    print(f"{len(bodies)} functions, {unique} distinct instructions ({total} arms incl. duplication)")
    print(f"source marks {len(prog.instr)} instructions; {len(prog.instr) - unique} never reached statically")
    for w in sorted(set(prog.warnings)):
        print("warning:", w, file=sys.stderr)


if __name__ == "__main__":
    main()
