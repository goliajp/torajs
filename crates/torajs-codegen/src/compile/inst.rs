//! Per-instruction dispatch: one `InstKind` to its emitter.

use torajs_core::ssa::{Function, InstKind, Operand, Type};

use super::ModuleFacts;
use super::call_carry::{self, CallCarry};
use super::slot;
use super::{
    emit_alloca, emit_binop, emit_bitcast_f64_to_i64, emit_bitcast_i64_to_f64,
    emit_boxed_entry_addr, emit_call, emit_call_indirect, emit_copy, emit_ctpop,
    emit_ctpop_range_sum, emit_cttz, emit_fcmp, emit_fn_addr, emit_fp_to_int32, emit_fp_to_si,
    emit_global_ref, emit_icmp, emit_int_to_ptr, emit_load, emit_load_dyn, emit_load_dyn_scaled8,
    emit_load_u8_dyn, emit_load_u16_dyn, emit_neg, emit_ptr_to_int, emit_si_to_fp,
    emit_static_str_ref, emit_store, emit_store_dyn, emit_store_dyn_scaled8, emit_string_ref,
    emit_trunc_i64_to_bool, emit_zext_bool_to_i64, emit_zext_i32_to_i64,
};
use crate::reg::Reg;
use crate::regalloc::Assignment;
use crate::reloc::Reloc;

pub(super) fn emit_plain(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    func: &Function,
    inst: &torajs_core::ssa::Inst,
    alloc: &Assignment,
    fn_sigs: &[Vec<Type>],
) {
    match &inst.kind {
        InstKind::BinOp(op, lhs, rhs) => emit_binop(bytes, relocs, inst, op, lhs, rhs, alloc),
        InstKind::ICmp(pred, lhs, rhs) => emit_icmp(bytes, inst, *pred, lhs, rhs, alloc),
        InstKind::FCmp(pred, lhs, rhs) => emit_fcmp(bytes, inst, *pred, lhs, rhs, alloc),
        InstKind::BitCastF64ToI64(src) => emit_bitcast_f64_to_i64(bytes, inst, src, alloc),
        InstKind::BitCastI64ToF64(src) => emit_bitcast_i64_to_f64(bytes, inst, src, alloc),
        InstKind::SiToFp(src) => emit_si_to_fp(bytes, inst, src, alloc),
        InstKind::FpToSi(src) => emit_fp_to_si(bytes, inst, src, alloc),
        InstKind::FpToInt32(src) => emit_fp_to_int32(bytes, inst, src, alloc),
        InstKind::ZExtBoolToI64(src) => emit_zext_bool_to_i64(bytes, inst, src, alloc),
        InstKind::ZExtI32ToI64(src) => emit_zext_i32_to_i64(bytes, inst, src, alloc),
        InstKind::TruncI64ToBool(src) => emit_trunc_i64_to_bool(bytes, inst, src, alloc),
        InstKind::PtrToInt(src) => emit_ptr_to_int(bytes, inst, src, alloc),
        InstKind::IntToPtr(src) => emit_int_to_ptr(bytes, inst, src, alloc),
        InstKind::Alloca(_) | InstKind::AllocaBytes(_) => emit_alloca(bytes, inst, alloc),
        InstKind::Load(Type::Vec(_), ptr, offset) => {
            super::vec::emit_vec_load(bytes, inst, ptr, *offset, alloc)
        }
        InstKind::Load(ty, ptr, offset) => emit_load(bytes, inst, ty, ptr, *offset, alloc),
        InstKind::Store(val @ Operand::Value(v), ptr, offset)
            if matches!(func.value_type(*v), Type::Vec(_)) =>
        {
            super::vec::emit_vec_store(bytes, val, ptr, *offset, alloc)
        }
        InstKind::Store(val, ptr, offset) => {
            let w32 = match val {
                Operand::ConstI32(_) => true,
                Operand::Value(v) => func.value_type(*v) == Type::I32,
                _ => false,
            };
            emit_store(bytes, val, ptr, *offset, w32, alloc)
        }
        InstKind::LoadDyn(ty, base, dyn_offset) => {
            emit_load_dyn(bytes, inst, ty, base, dyn_offset, alloc)
        }
        InstKind::StoreDyn(val, base, dyn_offset) => {
            emit_store_dyn(bytes, val, base, dyn_offset, alloc)
        }
        InstKind::LoadDynScaled8(ty, base, idx) => {
            emit_load_dyn_scaled8(bytes, inst, ty, base, idx, alloc)
        }
        InstKind::LoadU8Dyn(base, idx) => emit_load_u8_dyn(bytes, inst, base, idx, alloc),
        InstKind::LoadU16Dyn(base, idx) => emit_load_u16_dyn(bytes, inst, base, idx, alloc),
        InstKind::StoreDynScaled8(val, base, idx) => {
            emit_store_dyn_scaled8(bytes, val, base, idx, alloc)
        }
        InstKind::Call(func_id, args) => {
            let param_types = fn_sigs.get(func_id.0 as usize).map(|v| v.as_slice());
            emit_call(bytes, relocs, inst, *func_id, args, alloc, param_types)
        }
        InstKind::CallIndirect(sig_id, fn_ptr, args) => {
            emit_call_indirect(bytes, inst, *sig_id, fn_ptr, args, alloc)
        }
        InstKind::GlobalRef(name) => emit_global_ref(bytes, relocs, inst, name, alloc),
        InstKind::StringRef(string_id) => emit_string_ref(bytes, relocs, inst, *string_id, alloc),
        InstKind::StaticStrRef(string_id) => {
            emit_static_str_ref(bytes, relocs, inst, *string_id, alloc)
        }
        InstKind::FnAddr(func_id) => emit_fn_addr(bytes, relocs, inst, *func_id, alloc),
        InstKind::BoxedEntryAddr(func_id) => {
            emit_boxed_entry_addr(bytes, relocs, inst, *func_id, alloc)
        }
        InstKind::Identity(_) => unreachable!(
            "InstKind::Identity reached aarch64 emit — egraph elaborate must \
             drop it via set_opt_value before this point"
        ),
        InstKind::Neg(op) => emit_neg(bytes, inst, op, alloc),
        InstKind::Ctpop(op) => emit_ctpop(bytes, inst, op, alloc),
        InstKind::Cttz(op) => emit_cttz(bytes, inst, op, alloc),
        InstKind::Copy(Type::Vec(_), op) => super::vec::emit_vec_copy(bytes, inst, op, alloc),
        InstKind::Copy(ty, op) => emit_copy(bytes, inst, *ty, op, alloc),
        InstKind::Vec(vi) => super::vec::emit_vec(bytes, inst, vi, alloc),
        InstKind::Select(..) => super::cmp::emit_select(
            bytes,
            inst,
            alloc,
            &mut super::cond_flags::CondFlags::default(),
        ),
        InstKind::CtpopRangeSum(start, bound, acc_init) => {
            emit_ctpop_range_sum(bytes, inst, start, bound, acc_init, alloc)
        }
        InstKind::RcInc(ptr) => super::rc::emit_rc_inc(bytes, ptr, alloc),
        InstKind::RcIncAny(v) => super::rc::emit_rc_inc_any(bytes, v, alloc),
        InstKind::BoxAny(tag, v) => super::box_any::emit_box_any(bytes, func, inst, *tag, v, alloc),
        InstKind::RcDecAny(v) => super::rc_dec::emit_rc_dec_any(bytes, relocs, v, alloc, &[]),
        InstKind::AnyIsNumber(_)
        | InstKind::AnyIsInt32(_)
        | InstKind::AnyToInt32(_)
        | InstKind::AnyToF64(_, _)
        | InstKind::BoxInt32(_)
        | InstKind::BoxF64(_) => unreachable!(
            "a typed number-box node reached aarch64 emit — the egraph's \
             anynum expansion lowers every one to bit operations"
        ),
    }
}

/// `emit_inst` for the instruction at slot index `site`, with every
/// register the allocator keeps live across this call site stored
/// before it and reloaded after. A vector is stored whole around the
/// instruction, whichever arm of it runs. `carry` is what the
/// instruction emitted just before left in its slots (`call_carry`),
/// and comes back as what this one leaves.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_inst_at(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    func: &Function,
    inst: &torajs_core::ssa::Inst,
    alloc: &Assignment,
    facts: &ModuleFacts,
    site: u32,
    carry: &mut Option<CallCarry>,
) {
    let wide = alloc
        .vec_call_saves
        .get(&site)
        .map_or(&[][..], Vec::as_slice);
    if !wide.is_empty() {
        *carry = None;
    }
    for &(f, off) in wide {
        slot::sp_str_q(bytes, f, off);
    }
    *carry = emit_saved(bytes, relocs, func, inst, alloc, facts, site, carry.take());
    for &(f, off) in wide {
        slot::sp_ldr_q(bytes, f, off);
    }
    if !wide.is_empty() {
        *carry = None;
    }
}

/// The instruction with its caller-saved scalar registers stored
/// before the call and reloaded after. The store precedes the
/// argument setup, which may overwrite an argument register; the
/// reload follows the result move. Answers the reloads it ends with.
#[allow(clippy::too_many_arguments)]
fn emit_saved(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    func: &Function,
    inst: &torajs_core::ssa::Inst,
    alloc: &Assignment,
    facts: &ModuleFacts,
    site: u32,
    carry: Option<CallCarry>,
) -> Option<CallCarry> {
    // a call-defined member of a coalesced class sits inside the class
    // hull: its result lands in the saved register, which must not be
    // reloaded over it
    let result_home = inst
        .result
        .filter(|&r| alloc.contains(r))
        .map(|r| alloc.of(r));
    let saves: Vec<(Reg, u32)> = alloc
        .call_saves
        .get(&site)
        .into_iter()
        .flatten()
        .copied()
        .filter(|&(reg, _)| Some(reg) != result_home)
        .collect();
    // the release calls only on its slow arm, which stores and reloads
    // these itself
    if let InstKind::RcDecAny(v) = &inst.kind {
        super::rc_dec::emit_rc_dec_any(bytes, relocs, v, alloc, &saves);
        return None;
    }
    // likewise an intrinsic whose common case opens inline
    if super::inline_sites::try_emit_saved(bytes, relocs, func, inst, alloc, &saves, facts) {
        return None;
    }
    let kept = call_carry::take(bytes, carry, inst, alloc, &saves);
    for &s in saves.iter().filter(|s| !kept.contains(s)) {
        call_carry::store(bytes, s);
    }
    emit_plain(bytes, relocs, func, inst, alloc, &facts.fn_sigs);
    for &s in &saves {
        call_carry::reload(bytes, s);
    }
    CallCarry::after(bytes, saves)
}

#[cfg(test)]
mod tests {
    use torajs_core::ssa::id_hash::HashMap;

    use super::emit_inst_at;
    use crate::compile::{ModuleFacts, slot};
    use crate::enc::bl_imm26;
    use crate::reg::{Gpr, Reg};
    use crate::regalloc::Assignment;
    use torajs_core::ssa::{FuncId, Function, Inst, InstKind, Type, ValueId};

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    /// A call whose result lands in a saved register (a call-defined
    /// member of a coalesced class) keeps its result: that register
    /// is neither stored nor reloaded around the call; another saved
    /// register still is.
    #[test]
    fn call_result_register_is_not_reloaded_over_the_result() {
        let mut alloc = Assignment::from_parts(
            HashMap::from_iter([(0, Reg::Gpr(Gpr::X5))]),
            HashMap::default(),
            0,
            32,
            true,
        );
        alloc.call_saves =
            HashMap::from_iter([(7, vec![(Reg::Gpr(Gpr::X5), 16), (Reg::Gpr(Gpr::X6), 24)])]);
        let inst = Inst {
            result: Some(ValueId(0)),
            kind: InstKind::Call(FuncId(0), Vec::new()),
            origin: None,
        };
        let mut bytes = Vec::new();
        let func = Function::new("f", Type::I64);
        let facts = ModuleFacts::default();
        emit_inst_at(
            &mut bytes,
            &mut Vec::new(),
            &func,
            &inst,
            &alloc,
            &facts,
            7,
            &mut None,
        );

        let mut expect = Vec::new();
        slot::sp_str_x(&mut expect, Gpr::X6, 24);
        expect.extend_from_slice(&bl_imm26(0).to_le_bytes());
        expect.extend_from_slice(&crate::enc::mov_x_reg(Gpr::X5, Gpr::X0).to_le_bytes());
        slot::sp_ldr_x(&mut expect, Gpr::X6, 24);
        assert_eq!(words(&bytes), words(&expect));
    }
}
