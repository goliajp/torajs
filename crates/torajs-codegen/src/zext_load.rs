//! Widenings that move no bits: `ZExtI32ToI64` of a 32-bit load. The
//! load is a W-form `LDR`, which already clears the top half of its
//! register, so the widening is a plain copy of the register — the
//! coalescer may give it the load's register, and the emitter then
//! writes nothing.
//!
//! Only a load with no other def qualifies: a φ cell holding it may be
//! written elsewhere by a call result, whose top half the AAPCS64
//! leaves undefined.

use torajs_core::ssa::id_hash::HashSet;
use torajs_core::ssa::{Function, InstKind, Operand, Type};

/// The result ids of every widening of a single-def 32-bit load.
pub(crate) fn widened_loads(func: &Function) -> HashSet<u32> {
    let mut widenings = Vec::new();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if let (InstKind::ZExtI32ToI64(Operand::Value(v)), Some(r)) = (&inst.kind, inst.result) {
            widenings.push((r.0, v.0));
        }
    }
    if widenings.is_empty() {
        return HashSet::default();
    }
    // per value: no def yet, one def that is a 32-bit load, or anything else
    const NONE: u8 = 0;
    const LOAD: u8 = 1;
    const OTHER: u8 = 2;
    let mut def = vec![NONE; func.values.len()];
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        let Some(r) = inst.result else { continue };
        let d = &mut def[r.0 as usize];
        let load = matches!(inst.kind, InstKind::Load(Type::I32, _, _));
        *d = if *d == NONE && load { LOAD } else { OTHER };
    }
    widenings
        .into_iter()
        .filter(|&(_, v)| def[v as usize] == LOAD)
        .map(|(w, _)| w)
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::compile::compile_function;
    use crate::enc;
    use crate::reg::Gpr;
    use torajs_core::ssa::{BinOp, Function, InstKind, Operand, Terminator, Type};

    fn words(f: &Function) -> Vec<u32> {
        compile_function(f)
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect()
    }

    /// `ret zext(load_i32(p + 8)) + 1`
    fn widened_length() -> Function {
        let mut f = Function::new("t", Type::I64);
        let p = f.add_param(Type::Ptr, "p");
        let b = f.add_block();
        let raw = f.append_inst(
            b,
            InstKind::Load(Type::I32, Operand::Value(p), 8),
            Type::I32,
            None,
        );
        let wide = f.append_inst(
            b,
            InstKind::ZExtI32ToI64(Operand::Value(raw)),
            Type::I64,
            None,
        );
        let sum = f.append_inst(
            b,
            InstKind::BinOp(BinOp::Add, Operand::Value(wide), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        f.set_term(b, Terminator::Ret(Some(Operand::Value(sum))));
        f
    }

    #[test]
    fn a_widened_32_bit_load_moves_nothing() {
        let w = words(&widened_length());
        // the load, then no `mov wd, wn` (ORR wd, wzr, wn) before the add
        assert_eq!(w[0] & !0x1F, enc::ldr_w_imm12(Gpr::X0, Gpr::X0, 8) & !0x1F);
        assert!(
            !w.iter().any(|&x| x & 0xFFE0_FFE0 == 0x2A00_03E0),
            "{w:08X?}"
        );
    }
}
