//! 6502 instruction semantics shared by the recompiled code and the fallback interpreter.
//!
//! Recompiled code calls [`Machine::exec`] with a constant opcode and operand for every
//! non-control-flow instruction; with `#[inline(always)]` the match folds away per call site.
//! Control flow (branches, jumps, calls, returns) is emitted by the generator directly and uses
//! the stack/cycle helpers at the bottom of this file.

use crate::machine::Machine;

pub const FLAG_C: u8 = 0x01;
pub const FLAG_Z: u8 = 0x02;
pub const FLAG_I: u8 = 0x04;
pub const FLAG_D: u8 = 0x08;
pub const FLAG_B: u8 = 0x10;
pub const FLAG_U: u8 = 0x20;
pub const FLAG_V: u8 = 0x40;
pub const FLAG_N: u8 = 0x80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Imp,
    Acc,
    Imm,
    Zp,
    ZpX,
    ZpY,
    Abs,
    AbsX,
    AbsY,
    Ind,
    IndX,
    IndY,
    Rel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Adc, And, Asl, Bcc, Bcs, Beq, Bit, Bmi, Bne, Bpl, Brk, Bvc, Bvs, Clc, Cld, Cli, Clv, Cmp, Cpx,
    Cpy, Dec, Dex, Dey, Eor, Inc, Inx, Iny, Jmp, Jsr, Lda, Ldx, Ldy, Lsr, Nop, Ora, Pha, Php, Pla,
    Plp, Rol, Ror, Rti, Rts, Sbc, Sec, Sed, Sei, Sta, Stx, Sty, Tax, Tay, Tsx, Txa, Txs, Tya,
}

/// (op, mode, base cycles, +1 on page cross) for each official opcode.
pub const fn decode(opcode: u8) -> Option<(Op, Mode, u8, bool)> {
    use Mode::*;
    use Op::*;
    Some(match opcode {
        0x69 => (Adc, Imm, 2, false), 0x65 => (Adc, Zp, 3, false), 0x75 => (Adc, ZpX, 4, false),
        0x6D => (Adc, Abs, 4, false), 0x7D => (Adc, AbsX, 4, true), 0x79 => (Adc, AbsY, 4, true),
        0x61 => (Adc, IndX, 6, false), 0x71 => (Adc, IndY, 5, true),
        0x29 => (And, Imm, 2, false), 0x25 => (And, Zp, 3, false), 0x35 => (And, ZpX, 4, false),
        0x2D => (And, Abs, 4, false), 0x3D => (And, AbsX, 4, true), 0x39 => (And, AbsY, 4, true),
        0x21 => (And, IndX, 6, false), 0x31 => (And, IndY, 5, true),
        0x0A => (Asl, Acc, 2, false), 0x06 => (Asl, Zp, 5, false), 0x16 => (Asl, ZpX, 6, false),
        0x0E => (Asl, Abs, 6, false), 0x1E => (Asl, AbsX, 7, false),
        0x90 => (Bcc, Rel, 2, false), 0xB0 => (Bcs, Rel, 2, false), 0xF0 => (Beq, Rel, 2, false),
        0x30 => (Bmi, Rel, 2, false), 0xD0 => (Bne, Rel, 2, false), 0x10 => (Bpl, Rel, 2, false),
        0x50 => (Bvc, Rel, 2, false), 0x70 => (Bvs, Rel, 2, false),
        0x24 => (Bit, Zp, 3, false), 0x2C => (Bit, Abs, 4, false),
        0x00 => (Brk, Imp, 7, false),
        0x18 => (Clc, Imp, 2, false), 0xD8 => (Cld, Imp, 2, false), 0x58 => (Cli, Imp, 2, false),
        0xB8 => (Clv, Imp, 2, false),
        0xC9 => (Cmp, Imm, 2, false), 0xC5 => (Cmp, Zp, 3, false), 0xD5 => (Cmp, ZpX, 4, false),
        0xCD => (Cmp, Abs, 4, false), 0xDD => (Cmp, AbsX, 4, true), 0xD9 => (Cmp, AbsY, 4, true),
        0xC1 => (Cmp, IndX, 6, false), 0xD1 => (Cmp, IndY, 5, true),
        0xE0 => (Cpx, Imm, 2, false), 0xE4 => (Cpx, Zp, 3, false), 0xEC => (Cpx, Abs, 4, false),
        0xC0 => (Cpy, Imm, 2, false), 0xC4 => (Cpy, Zp, 3, false), 0xCC => (Cpy, Abs, 4, false),
        0xC6 => (Dec, Zp, 5, false), 0xD6 => (Dec, ZpX, 6, false), 0xCE => (Dec, Abs, 6, false),
        0xDE => (Dec, AbsX, 7, false),
        0xCA => (Dex, Imp, 2, false), 0x88 => (Dey, Imp, 2, false),
        0x49 => (Eor, Imm, 2, false), 0x45 => (Eor, Zp, 3, false), 0x55 => (Eor, ZpX, 4, false),
        0x4D => (Eor, Abs, 4, false), 0x5D => (Eor, AbsX, 4, true), 0x59 => (Eor, AbsY, 4, true),
        0x41 => (Eor, IndX, 6, false), 0x51 => (Eor, IndY, 5, true),
        0xE6 => (Inc, Zp, 5, false), 0xF6 => (Inc, ZpX, 6, false), 0xEE => (Inc, Abs, 6, false),
        0xFE => (Inc, AbsX, 7, false),
        0xE8 => (Inx, Imp, 2, false), 0xC8 => (Iny, Imp, 2, false),
        0x4C => (Jmp, Abs, 3, false), 0x6C => (Jmp, Ind, 5, false),
        0x20 => (Jsr, Abs, 6, false),
        0xA9 => (Lda, Imm, 2, false), 0xA5 => (Lda, Zp, 3, false), 0xB5 => (Lda, ZpX, 4, false),
        0xAD => (Lda, Abs, 4, false), 0xBD => (Lda, AbsX, 4, true), 0xB9 => (Lda, AbsY, 4, true),
        0xA1 => (Lda, IndX, 6, false), 0xB1 => (Lda, IndY, 5, true),
        0xA2 => (Ldx, Imm, 2, false), 0xA6 => (Ldx, Zp, 3, false), 0xB6 => (Ldx, ZpY, 4, false),
        0xAE => (Ldx, Abs, 4, false), 0xBE => (Ldx, AbsY, 4, true),
        0xA0 => (Ldy, Imm, 2, false), 0xA4 => (Ldy, Zp, 3, false), 0xB4 => (Ldy, ZpX, 4, false),
        0xAC => (Ldy, Abs, 4, false), 0xBC => (Ldy, AbsX, 4, true),
        0x4A => (Lsr, Acc, 2, false), 0x46 => (Lsr, Zp, 5, false), 0x56 => (Lsr, ZpX, 6, false),
        0x4E => (Lsr, Abs, 6, false), 0x5E => (Lsr, AbsX, 7, false),
        0xEA => (Nop, Imp, 2, false),
        0x09 => (Ora, Imm, 2, false), 0x05 => (Ora, Zp, 3, false), 0x15 => (Ora, ZpX, 4, false),
        0x0D => (Ora, Abs, 4, false), 0x1D => (Ora, AbsX, 4, true), 0x19 => (Ora, AbsY, 4, true),
        0x01 => (Ora, IndX, 6, false), 0x11 => (Ora, IndY, 5, true),
        0x48 => (Pha, Imp, 3, false), 0x08 => (Php, Imp, 3, false), 0x68 => (Pla, Imp, 4, false),
        0x28 => (Plp, Imp, 4, false),
        0x2A => (Rol, Acc, 2, false), 0x26 => (Rol, Zp, 5, false), 0x36 => (Rol, ZpX, 6, false),
        0x2E => (Rol, Abs, 6, false), 0x3E => (Rol, AbsX, 7, false),
        0x6A => (Ror, Acc, 2, false), 0x66 => (Ror, Zp, 5, false), 0x76 => (Ror, ZpX, 6, false),
        0x6E => (Ror, Abs, 6, false), 0x7E => (Ror, AbsX, 7, false),
        0x40 => (Rti, Imp, 6, false), 0x60 => (Rts, Imp, 6, false),
        0xE9 => (Sbc, Imm, 2, false), 0xE5 => (Sbc, Zp, 3, false), 0xF5 => (Sbc, ZpX, 4, false),
        0xED => (Sbc, Abs, 4, false), 0xFD => (Sbc, AbsX, 4, true), 0xF9 => (Sbc, AbsY, 4, true),
        0xE1 => (Sbc, IndX, 6, false), 0xF1 => (Sbc, IndY, 5, true),
        0x38 => (Sec, Imp, 2, false), 0xF8 => (Sed, Imp, 2, false), 0x78 => (Sei, Imp, 2, false),
        0x85 => (Sta, Zp, 3, false), 0x95 => (Sta, ZpX, 4, false), 0x8D => (Sta, Abs, 4, false),
        0x9D => (Sta, AbsX, 5, false), 0x99 => (Sta, AbsY, 5, false), 0x81 => (Sta, IndX, 6, false),
        0x91 => (Sta, IndY, 6, false),
        0x86 => (Stx, Zp, 3, false), 0x96 => (Stx, ZpY, 4, false), 0x8E => (Stx, Abs, 4, false),
        0x84 => (Sty, Zp, 3, false), 0x94 => (Sty, ZpX, 4, false), 0x8C => (Sty, Abs, 4, false),
        0xAA => (Tax, Imp, 2, false), 0xA8 => (Tay, Imp, 2, false), 0xBA => (Tsx, Imp, 2, false),
        0x8A => (Txa, Imp, 2, false), 0x9A => (Txs, Imp, 2, false), 0x98 => (Tya, Imp, 2, false),
        _ => return None,
    })
}

impl Machine {
    #[inline(always)]
    fn set_zn(&mut self, v: u8) {
        self.z = v == 0;
        self.n = v & 0x80 != 0;
    }

    /// Effective address for a memory operand; also reports a page crossing.
    #[inline(always)]
    fn ea(&mut self, mode: Mode, operand: u16) -> (u16, bool) {
        match mode {
            Mode::Zp => (operand & 0xFF, false),
            Mode::ZpX => ((operand as u8).wrapping_add(self.x) as u16, false),
            Mode::ZpY => ((operand as u8).wrapping_add(self.y) as u16, false),
            Mode::Abs => (operand, false),
            Mode::AbsX => {
                let a = operand.wrapping_add(self.x as u16);
                (a, a & 0xFF00 != operand & 0xFF00)
            }
            Mode::AbsY => {
                let a = operand.wrapping_add(self.y as u16);
                (a, a & 0xFF00 != operand & 0xFF00)
            }
            Mode::IndX => {
                let zp = (operand as u8).wrapping_add(self.x);
                let lo = self.read(zp as u16) as u16;
                let hi = self.read(zp.wrapping_add(1) as u16) as u16;
                (hi << 8 | lo, false)
            }
            Mode::IndY => {
                let zp = operand as u8;
                let lo = self.read(zp as u16) as u16;
                let hi = self.read(zp.wrapping_add(1) as u16) as u16;
                let base = hi << 8 | lo;
                let a = base.wrapping_add(self.y as u16);
                (a, a & 0xFF00 != base & 0xFF00)
            }
            _ => unreachable!("no effective address for {mode:?}"),
        }
    }

    #[inline(always)]
    fn operand_value(&mut self, mode: Mode, operand: u16, page_penalty: bool) -> u8 {
        if mode == Mode::Imm {
            return operand as u8;
        }
        let (a, crossed) = self.ea(mode, operand);
        if crossed && page_penalty {
            self.extra_cycles += 1;
        }
        self.read(a)
    }

    /// Read-modify-write: the 6502 writes the unmodified value back before the result, which
    /// matters for MMC1 (the double write is ignored there) and PPU registers.
    #[inline(always)]
    fn rmw(&mut self, mode: Mode, operand: u16, f: impl FnOnce(&mut Self, u8) -> u8) {
        if mode == Mode::Acc {
            let v = f(self, self.a);
            self.a = v;
            return;
        }
        let (a, _) = self.ea(mode, operand);
        let old = self.read(a);
        self.write_dummy(a, old);
        let v = f(self, old);
        self.write(a, v);
    }

    #[inline(always)]
    fn adc_value(&mut self, v: u8) {
        let sum = self.a as u16 + v as u16 + self.c as u16;
        let r = sum as u8;
        self.v = (!(self.a ^ v) & (self.a ^ r) & 0x80) != 0;
        self.c = sum > 0xFF;
        self.a = r;
        self.set_zn(r);
    }

    #[inline(always)]
    fn compare(&mut self, reg: u8, v: u8) {
        let r = reg.wrapping_sub(v);
        self.c = reg >= v;
        self.set_zn(r);
    }

    /// Execute one non-control-flow instruction and account its cycles.
    #[inline]
    pub fn exec(&mut self, opcode: u8, operand: u16) {
        let Some((op, mode, cycles, page_penalty)) = decode(opcode) else {
            panic!("illegal opcode {opcode:02X}");
        };
        self.extra_cycles = 0;
        match op {
            Op::Adc => {
                let v = self.operand_value(mode, operand, page_penalty);
                self.adc_value(v);
            }
            Op::Sbc => {
                let v = self.operand_value(mode, operand, page_penalty);
                self.adc_value(!v);
            }
            Op::And => {
                let v = self.operand_value(mode, operand, page_penalty);
                self.a &= v;
                self.set_zn(self.a);
            }
            Op::Ora => {
                let v = self.operand_value(mode, operand, page_penalty);
                self.a |= v;
                self.set_zn(self.a);
            }
            Op::Eor => {
                let v = self.operand_value(mode, operand, page_penalty);
                self.a ^= v;
                self.set_zn(self.a);
            }
            Op::Bit => {
                let v = self.operand_value(mode, operand, false);
                self.z = self.a & v == 0;
                self.n = v & 0x80 != 0;
                self.v = v & 0x40 != 0;
            }
            Op::Cmp => {
                let v = self.operand_value(mode, operand, page_penalty);
                self.compare(self.a, v);
            }
            Op::Cpx => {
                let v = self.operand_value(mode, operand, false);
                self.compare(self.x, v);
            }
            Op::Cpy => {
                let v = self.operand_value(mode, operand, false);
                self.compare(self.y, v);
            }
            Op::Lda => {
                self.a = self.operand_value(mode, operand, page_penalty);
                self.set_zn(self.a);
            }
            Op::Ldx => {
                self.x = self.operand_value(mode, operand, page_penalty);
                self.set_zn(self.x);
            }
            Op::Ldy => {
                self.y = self.operand_value(mode, operand, page_penalty);
                self.set_zn(self.y);
            }
            Op::Sta => {
                let (a, _) = self.ea(mode, operand);
                self.write(a, self.a);
            }
            Op::Stx => {
                let (a, _) = self.ea(mode, operand);
                self.write(a, self.x);
            }
            Op::Sty => {
                let (a, _) = self.ea(mode, operand);
                self.write(a, self.y);
            }
            Op::Asl => self.rmw(mode, operand, |m, v| {
                m.c = v & 0x80 != 0;
                let r = v << 1;
                m.set_zn(r);
                r
            }),
            Op::Lsr => self.rmw(mode, operand, |m, v| {
                m.c = v & 0x01 != 0;
                let r = v >> 1;
                m.set_zn(r);
                r
            }),
            Op::Rol => self.rmw(mode, operand, |m, v| {
                let r = v << 1 | m.c as u8;
                m.c = v & 0x80 != 0;
                m.set_zn(r);
                r
            }),
            Op::Ror => self.rmw(mode, operand, |m, v| {
                let r = v >> 1 | (m.c as u8) << 7;
                m.c = v & 0x01 != 0;
                m.set_zn(r);
                r
            }),
            Op::Inc => self.rmw(mode, operand, |m, v| {
                let r = v.wrapping_add(1);
                m.set_zn(r);
                r
            }),
            Op::Dec => self.rmw(mode, operand, |m, v| {
                let r = v.wrapping_sub(1);
                m.set_zn(r);
                r
            }),
            Op::Inx => {
                self.x = self.x.wrapping_add(1);
                self.set_zn(self.x);
            }
            Op::Iny => {
                self.y = self.y.wrapping_add(1);
                self.set_zn(self.y);
            }
            Op::Dex => {
                self.x = self.x.wrapping_sub(1);
                self.set_zn(self.x);
            }
            Op::Dey => {
                self.y = self.y.wrapping_sub(1);
                self.set_zn(self.y);
            }
            Op::Tax => {
                self.x = self.a;
                self.set_zn(self.x);
            }
            Op::Tay => {
                self.y = self.a;
                self.set_zn(self.y);
            }
            Op::Txa => {
                self.a = self.x;
                self.set_zn(self.a);
            }
            Op::Tya => {
                self.a = self.y;
                self.set_zn(self.a);
            }
            Op::Tsx => {
                self.x = self.sp;
                self.set_zn(self.x);
            }
            Op::Txs => self.sp = self.x,
            Op::Pha => self.push(self.a),
            Op::Php => self.push(self.status() | FLAG_B | FLAG_U),
            Op::Pla => {
                self.a = self.pull();
                self.set_zn(self.a);
            }
            Op::Plp => {
                let p = self.pull();
                self.set_status(p);
            }
            Op::Clc => self.c = false,
            Op::Sec => self.c = true,
            Op::Cli => self.i = false,
            Op::Sei => self.i = true,
            Op::Cld => self.d = false,
            Op::Sed => self.d = true,
            Op::Clv => self.v = false,
            Op::Nop => {}
            Op::Bcc | Op::Bcs | Op::Beq | Op::Bmi | Op::Bne | Op::Bpl | Op::Bvc | Op::Bvs
            | Op::Brk | Op::Jmp | Op::Jsr | Op::Rti | Op::Rts => {
                panic!("control-flow opcode {opcode:02X} passed to exec")
            }
        }
        self.tick(cycles as u32 + self.extra_cycles);
    }

    // ---- control-flow helpers used by generated code ----

    /// Evaluate a branch condition by opcode.
    #[inline(always)]
    pub fn branch_cond(&self, opcode: u8) -> bool {
        match opcode {
            0x90 => !self.c,
            0xB0 => self.c,
            0xF0 => self.z,
            0xD0 => !self.z,
            0x30 => self.n,
            0x10 => !self.n,
            0x50 => !self.v,
            0x70 => self.v,
            _ => unreachable!(),
        }
    }

    /// Account a branch: 2 cycles, +1 if taken, +1 more if the target is on another page.
    #[inline(always)]
    pub fn branch(&mut self, opcode: u8, next: u16, target: u16) -> bool {
        let taken = self.branch_cond(opcode);
        let mut cycles = 2;
        if taken {
            cycles += 1;
            if next & 0xFF00 != target & 0xFF00 {
                cycles += 1;
            }
        }
        self.tick(cycles);
        taken
    }

    /// `jsr`: push the address of the call's last byte.
    #[inline(always)]
    pub fn jsr(&mut self, at: u16) {
        let ret = at.wrapping_add(2);
        self.push((ret >> 8) as u8);
        self.push(ret as u8);
        self.tick(6);
    }

    /// `rts`: pop the return address; the caller resumes at [`Machine::ret_to`].
    #[inline(always)]
    pub fn rts(&mut self) {
        let lo = self.pull() as u16;
        let hi = self.pull() as u16;
        self.ret_to = (hi << 8 | lo).wrapping_add(1);
        self.tick(6);
    }

    /// `rti`: pop status and return address.
    #[inline(always)]
    pub fn rti(&mut self) {
        let p = self.pull();
        self.set_status(p);
        let lo = self.pull() as u16;
        let hi = self.pull() as u16;
        self.ret_to = hi << 8 | lo;
        self.tick(6);
    }

    /// `jmp (ptr)` with the 6502's page-wrap bug.
    #[inline(always)]
    pub fn jmp_ind(&mut self, ptr: u16) -> u16 {
        let lo = self.read(ptr) as u16;
        let hi = self.read((ptr & 0xFF00) | (ptr.wrapping_add(1) & 0x00FF)) as u16;
        self.tick(5);
        hi << 8 | lo
    }

    #[inline(always)]
    pub fn push(&mut self, v: u8) {
        self.ram[0x100 | self.sp as usize] = v;
        self.sp = self.sp.wrapping_sub(1);
    }

    #[inline(always)]
    pub fn pull(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.ram[0x100 | self.sp as usize]
    }

    pub fn status(&self) -> u8 {
        (self.c as u8)
            | (self.z as u8) << 1
            | (self.i as u8) << 2
            | (self.d as u8) << 3
            | (self.v as u8) << 6
            | (self.n as u8) << 7
    }

    pub fn set_status(&mut self, p: u8) {
        self.c = p & FLAG_C != 0;
        self.z = p & FLAG_Z != 0;
        self.i = p & FLAG_I != 0;
        self.d = p & FLAG_D != 0;
        self.v = p & FLAG_V != 0;
        self.n = p & FLAG_N != 0;
    }
}

/// Instruction length in bytes for an addressing mode.
pub const fn length(mode: Mode) -> u16 {
    match mode {
        Mode::Imp | Mode::Acc => 1,
        Mode::Abs | Mode::AbsX | Mode::AbsY | Mode::Ind => 3,
        _ => 2,
    }
}
