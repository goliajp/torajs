//! Saves carried from one call to the next.
//!
//! A value kept in a caller-saved register across a call is stored to
//! its slot before the `bl` and reloaded after it. When the very next
//! instruction is another call that saves the same register to the same
//! slot, its store writes what the slot already holds, and the reload
//! in between is dead too unless that call passes the register as an
//! argument: the second `bl` clobbers it again and the reload after it
//! brings the value back. Back-to-back runtime calls on a cold path
//! (`arr_get_any_tag` then `arr_get_any_value`) are where this shows.

use torajs_core::ssa::{Inst, visit_value_operands};

use super::slot;
use crate::reg::Reg;
use crate::regalloc::Assignment;

/// The reloads the last call emitted, ending at byte `end`.
pub(crate) struct CallCarry {
    end: usize,
    reloads: Vec<(Reg, u32)>,
}

impl CallCarry {
    /// The reloads `saves` just emitted, the last words of `bytes`.
    pub(crate) fn after(bytes: &[u8], saves: Vec<(Reg, u32)>) -> Option<Self> {
        (!saves.is_empty()).then_some(CallCarry {
            end: bytes.len(),
            reloads: saves,
        })
    }
}

/// The saves of `inst` whose store `carry` makes dead: the instruction
/// emitted last was a call whose reloads end `bytes` and which stored
/// the same register to the same slot, so the slot still holds it.
/// Where `inst` does not read that register either, the reload is
/// taken back out of `bytes` too.
pub(crate) fn take(
    bytes: &mut Vec<u8>,
    carry: Option<CallCarry>,
    inst: &Inst,
    alloc: &Assignment,
    saves: &[(Reg, u32)],
) -> Vec<(Reg, u32)> {
    let Some(prev) = carry.filter(|c| c.end == bytes.len()) else {
        return Vec::new();
    };
    let held: Vec<(Reg, u32)> = prev
        .reloads
        .iter()
        .copied()
        .filter(|s| saves.contains(s))
        .collect();
    if held.is_empty() {
        return held;
    }
    let mut read: Vec<Reg> = Vec::new();
    visit_value_operands(&inst.kind, |v| {
        if alloc.contains(v) {
            read.push(alloc.of(v));
        }
    });
    let start = bytes.len() - prev.reloads.len() * 4;
    let mut tail = Vec::new();
    for &s in &prev.reloads {
        reload(&mut tail, s);
    }
    debug_assert_eq!(&bytes[start..], &tail[..], "a carry ends in its reloads");
    bytes.truncate(start);
    for &s in &prev.reloads {
        if !held.contains(&s) || read.contains(&s.0) {
            reload(bytes, s);
        }
    }
    held
}

pub(crate) fn store(bytes: &mut Vec<u8>, (reg, off): (Reg, u32)) {
    match reg {
        Reg::Gpr(g) => slot::sp_str_x(bytes, g, off),
        Reg::Fpr(f) => slot::sp_str_d(bytes, f, off),
        other => unreachable!("saved value not in a register: {other:?}"),
    }
}

pub(crate) fn reload(bytes: &mut Vec<u8>, (reg, off): (Reg, u32)) {
    match reg {
        Reg::Gpr(g) => slot::sp_ldr_x(bytes, g, off),
        Reg::Fpr(f) => slot::sp_ldr_d(bytes, f, off),
        other => unreachable!("saved value not in a register: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use torajs_core::ssa::id_hash::HashMap;
    use torajs_core::ssa::{FuncId, Function, Inst, InstKind, Operand, Type, ValueId};

    use super::super::ModuleFacts;
    use super::super::inst::emit_inst_at;
    use super::*;
    use crate::enc::{bl_imm26, mov_x_reg};
    use crate::reg::Gpr;

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn call(args: Vec<Operand>) -> Inst {
        Inst {
            result: None,
            kind: InstKind::Call(FuncId(0), args),
            origin: None,
        }
    }

    /// Two calls back to back, each saving X5 and X6 (sites 7 and 8).
    fn run(second: Inst) -> Vec<u32> {
        let mut alloc = Assignment::from_parts(
            HashMap::from_iter([(3, Reg::Gpr(Gpr::X5))]),
            HashMap::default(),
            0,
            32,
            true,
        );
        let saves = vec![(Reg::Gpr(Gpr::X5), 16), (Reg::Gpr(Gpr::X6), 24)];
        alloc.call_saves = HashMap::from_iter([(7, saves.clone()), (8, saves)]);
        let func = Function::new("f", Type::I64);
        let facts = ModuleFacts::default();
        let (mut bytes, mut relocs, mut carry) = (Vec::new(), Vec::new(), None);
        for (site, inst) in [(7, call(Vec::new())), (8, second)] {
            emit_inst_at(
                &mut bytes,
                &mut relocs,
                &func,
                &inst,
                &alloc,
                &facts,
                site,
                &mut carry,
            );
        }
        words(&bytes)
    }

    #[test]
    fn a_second_call_keeps_what_the_first_stored() {
        let mut expect = Vec::new();
        store(&mut expect, (Reg::Gpr(Gpr::X5), 16));
        store(&mut expect, (Reg::Gpr(Gpr::X6), 24));
        expect.extend_from_slice(&bl_imm26(0).to_le_bytes());
        expect.extend_from_slice(&bl_imm26(0).to_le_bytes());
        reload(&mut expect, (Reg::Gpr(Gpr::X5), 16));
        reload(&mut expect, (Reg::Gpr(Gpr::X6), 24));
        assert_eq!(run(call(Vec::new())), words(&expect));
    }

    #[test]
    fn a_register_the_second_call_passes_is_reloaded_for_it() {
        let mut expect = Vec::new();
        store(&mut expect, (Reg::Gpr(Gpr::X5), 16));
        store(&mut expect, (Reg::Gpr(Gpr::X6), 24));
        expect.extend_from_slice(&bl_imm26(0).to_le_bytes());
        reload(&mut expect, (Reg::Gpr(Gpr::X5), 16));
        expect.extend_from_slice(&mov_x_reg(Gpr::X0, Gpr::X5).to_le_bytes());
        expect.extend_from_slice(&bl_imm26(0).to_le_bytes());
        reload(&mut expect, (Reg::Gpr(Gpr::X5), 16));
        reload(&mut expect, (Reg::Gpr(Gpr::X6), 24));
        let passes = call(vec![Operand::Value(ValueId(3))]);
        assert_eq!(run(passes), words(&expect));
    }
}
