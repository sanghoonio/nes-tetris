//! Fallback 6502 interpreter, for code the generator didn't reach statically.
//!
//! Shares instruction semantics with the recompiled code (`Machine::exec`) but shares nothing with
//! the reference emulator. Every entry is logged in `Machine::fallback_log`; the goal is none.

use crate::cpu::{decode, Op};
use crate::machine::Machine;

/// Run the routine at `addr` until it returns.
pub fn interpret(m: &mut Machine, addr: u16) {
    m.fallback_log.push(addr);
    run(m, addr);
}

/// A recompiled fn reached a `pc` it has no arm for (e.g. a return address rewritten to somewhere
/// unexpected). Carry on interpreting in the same frame.
pub fn interpret_inline(m: &mut Machine, pc: u16) {
    m.fallback_log.push(pc);
    run(m, pc);
}

fn run(m: &mut Machine, mut pc: u16) {
    loop {
        m.poll(pc);
        let opcode = m.read(pc);
        let Some((op, mode, _, _)) = decode(opcode) else {
            panic!("illegal opcode {opcode:02X} at {pc:04X}");
        };
        let len = crate::cpu::length(mode);
        let operand = match len {
            2 => m.read(pc.wrapping_add(1)) as u16,
            3 => u16::from_le_bytes([m.read(pc.wrapping_add(1)), m.read(pc.wrapping_add(2))]),
            _ => 0,
        };
        let next = pc.wrapping_add(len);
        match op {
            Op::Bcc | Op::Bcs | Op::Beq | Op::Bmi | Op::Bne | Op::Bpl | Op::Bvc | Op::Bvs => {
                let target = next.wrapping_add(operand as u8 as i8 as u16);
                pc = if m.branch(opcode, next, target) { target } else { next };
            }
            Op::Jmp if opcode == 0x4C => {
                m.tick(3);
                pc = operand;
            }
            Op::Jmp => {
                let target = m.jmp_ind(operand);
                crate::generated::call(m, target);
                return;
            }
            Op::Jsr => {
                let sp0 = m.sp;
                m.jsr(pc);
                crate::generated::call(m, operand);
                if m.unwound(sp0) {
                    return;
                }
                pc = m.ret_to;
            }
            Op::Rts => {
                m.rts();
                return;
            }
            Op::Rti => {
                m.rti();
                return;
            }
            Op::Brk => panic!("brk at {pc:04X}"),
            _ => {
                m.exec(opcode, operand);
                pc = next;
            }
        }
    }
}
