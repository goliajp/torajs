//! Function-level compile driver.
//!
//! Walks `torajs_core::ssa::Function` once, dispatches each `InstKind`
//! to the matching aarch64 instruction sequence via `enc::*`, and
//! emits the little-endian byte stream + any `Reloc` descriptors.
//!
//! Split into sub-modules per InstKind family to keep each file (and
//! its inline test block) under the 500-LOC hard limit:
//!
//!   - [`operand`]: `materialize_operand_{gpr,fpr}` +
//!     `materialize_const_i64` (MOVZ+MOVK quadrant chain). All
//!     constant-to-register lowering lives here.
//!   - [`binop`]: `emit_binop` for the full integer + float BinOp
//!     surface (`BinOp::Add..LShr` + `FAdd..FDiv`).
//!   - [`cmp`]: `emit_icmp` / `emit_fcmp` lowering ICmp/FCmp to
//!     CMP/FCMP + CSET with `ipred_to_cond` / `fpred_to_cond` mapping
//!     tables.
//!   - [`cast`]: `emit_bitcast_{f64_to_i64,i64_to_f64}` — single FMOV
//!     between Xn and Dn, no bit conversion.
//!
//! S1 baseline test (`fn one_plus_two() -> i64 { 1 + 2 }`) lives here
//! in `mod tests` because it's a driver-level acceptance test, not
//! tied to any one InstKind family.
//!
//! Phase-level coverage notes (history, kept for reference):
//!
//! S1 — Add only. S2-A — full integer BinOp + 64-bit ConstI64 via
//! MOVZ+MOVK. S2-B1 — FP encoders + Fpr enum. S2-B2 — FP binop wire-
//! up + BitCast + Operand::ConstF64 via FMOV-d-from-x. S2-C — ICmp +
//! FCmp via CMP/FCMP + CSET. S2-D pending: FRem (libm `fmod` call)
//! and FPred::One (CCMP fold or NE+VC+AND).

mod binop;
mod call;
mod cast;
mod cmp;
mod mem;
mod operand;
mod refs;

#[cfg(test)]
pub(crate) mod test_fixtures;

use torajs_core::ssa::{Function, InstKind, Terminator};

use crate::enc::ret;
use crate::frame::FrameLayout;
use crate::reg::{Fpr, Gpr};
use crate::regalloc::{Assignment, allocate_trivial};
use crate::reloc::Reloc;

pub use binop::emit_binop;
pub use call::emit_call;
pub use cast::{
    emit_bitcast_f64_to_i64, emit_bitcast_i64_to_f64, emit_fp_to_si, emit_int_to_ptr,
    emit_ptr_to_int, emit_si_to_fp, emit_trunc_i64_to_bool, emit_zext_bool_to_i64,
    emit_zext_i32_to_i64,
};
pub use cmp::{emit_fcmp, emit_icmp};
pub use mem::{emit_alloca, emit_load, emit_load_dyn, emit_store, emit_store_dyn};
pub use operand::{materialize_const_i64, materialize_operand_fpr, materialize_operand_gpr};
pub use refs::{emit_fn_addr, emit_global_ref, emit_static_str_ref, emit_string_ref};

/// Operand scratch GPRs — sub-modules use these to materialize int
/// constants (`materialize_const_i64`) and arrange operands.
pub(crate) const OP_SCRATCH_LHS: Gpr = Gpr::X9;
pub(crate) const OP_SCRATCH_RHS: Gpr = Gpr::X10;
/// Tertiary GPR scratch for compound ops (e.g. SREM = SDIV+MSUB).
pub(crate) const OP_SCRATCH_TMP: Gpr = Gpr::X11;
/// FPR scratches for FP-side operand materialization.
pub(crate) const FP_SCRATCH_LHS: Fpr = Fpr::V16;
pub(crate) const FP_SCRATCH_RHS: Fpr = Fpr::V17;

/// Output of `compile_function` — raw aarch64 bytes + per-function
/// reloc table, ready to hand off to torajs-obj (#7).
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    pub name: String,
    pub bytes: Vec<u8>,
    pub relocs: Vec<Reloc>,
    pub frame: FrameLayout,
}

/// Compile one SSA Function to aarch64 bytes.
pub fn compile_function(func: &Function) -> CompiledFunction {
    let alloc = allocate_trivial(func);
    let frame = FrameLayout::from_alloca_bytes(alloc.raw_alloca_bytes, alloc.has_calls);
    let mut bytes: Vec<u8> = Vec::new();
    let mut relocs: Vec<Reloc> = Vec::new();

    frame.emit_prologue(&mut bytes);

    for block in &func.blocks {
        for inst in &block.insts {
            emit_inst(&mut bytes, &mut relocs, inst, &alloc);
        }
        emit_terminator(&mut bytes, &block.term, &frame);
    }

    CompiledFunction {
        name: func.name.clone(),
        bytes,
        relocs,
        frame,
    }
}

fn emit_inst(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &torajs_core::ssa::Inst,
    alloc: &Assignment,
) {
    match &inst.kind {
        InstKind::BinOp(op, lhs, rhs) => emit_binop(bytes, inst, op, lhs, rhs, alloc),
        InstKind::ICmp(pred, lhs, rhs) => emit_icmp(bytes, inst, *pred, lhs, rhs, alloc),
        InstKind::FCmp(pred, lhs, rhs) => emit_fcmp(bytes, inst, *pred, lhs, rhs, alloc),
        InstKind::BitCastF64ToI64(src) => emit_bitcast_f64_to_i64(bytes, inst, src, alloc),
        InstKind::BitCastI64ToF64(src) => emit_bitcast_i64_to_f64(bytes, inst, src, alloc),
        InstKind::SiToFp(src) => emit_si_to_fp(bytes, inst, src, alloc),
        InstKind::FpToSi(src) => emit_fp_to_si(bytes, inst, src, alloc),
        InstKind::ZExtBoolToI64(src) => emit_zext_bool_to_i64(bytes, inst, src, alloc),
        InstKind::ZExtI32ToI64(src) => emit_zext_i32_to_i64(bytes, inst, src, alloc),
        InstKind::TruncI64ToBool(src) => emit_trunc_i64_to_bool(bytes, inst, src, alloc),
        InstKind::PtrToInt(src) => emit_ptr_to_int(bytes, inst, src, alloc),
        InstKind::IntToPtr(src) => emit_int_to_ptr(bytes, inst, src, alloc),
        InstKind::Alloca(_) | InstKind::AllocaBytes(_) => emit_alloca(bytes, inst, alloc),
        InstKind::Load(ty, ptr, offset) => emit_load(bytes, inst, ty, ptr, *offset, alloc),
        InstKind::Store(val, ptr, offset) => emit_store(bytes, val, ptr, *offset, alloc),
        InstKind::LoadDyn(ty, base, dyn_offset) => {
            emit_load_dyn(bytes, inst, ty, base, dyn_offset, alloc)
        }
        InstKind::StoreDyn(val, base, dyn_offset) => {
            emit_store_dyn(bytes, val, base, dyn_offset, alloc)
        }
        InstKind::Call(func_id, args) => emit_call(bytes, relocs, inst, *func_id, args, alloc),
        InstKind::GlobalRef(name) => emit_global_ref(bytes, relocs, inst, name, alloc),
        InstKind::StringRef(string_id) => emit_string_ref(bytes, relocs, inst, *string_id, alloc),
        InstKind::StaticStrRef(string_id) => {
            emit_static_str_ref(bytes, relocs, inst, *string_id, alloc)
        }
        InstKind::FnAddr(func_id) => emit_fn_addr(bytes, relocs, inst, *func_id, alloc),
        other => todo!("S4+: InstKind::{:?}", other),
    }
}

fn emit_terminator(bytes: &mut Vec<u8>, term: &Terminator, frame: &FrameLayout) {
    match term {
        Terminator::Ret(_) => {
            frame.emit_epilogue(bytes);
            write_u32(bytes, ret(Gpr::X30));
        }
        other => todo!("S5: Terminator::{:?}", other),
    }
}

pub(crate) fn write_u32(bytes: &mut Vec<u8>, word: u32) {
    bytes.extend_from_slice(&word.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_fixtures::{build_one_plus_two, words_to_le_bytes};

    /// S1 acceptance — `fn one_plus_two() -> i64 { 1 + 2 }` shape
    /// compiles end-to-end to the hand-encoded 16-byte reference:
    ///
    /// ```text
    ///   MOVZ x9, #1       0xD2800029
    ///   MOVZ x10, #2      0xD280004A
    ///   ADD x0, x9, x10   0x8B0A0120
    ///   RET               0xD65F03C0
    /// ```
    #[test]
    fn one_plus_two_byte_equal_reference() {
        let func = build_one_plus_two();
        let compiled = compile_function(&func);

        let expected = words_to_le_bytes(&[0xD280_0029, 0xD280_004A, 0x8B0A_0120, 0xD65F_03C0]);
        assert_eq!(
            compiled.bytes, expected,
            "byte stream mismatch — expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
        assert_eq!(compiled.name, "one_plus_two");
        assert!(compiled.relocs.is_empty(), "S1 has no relocs");
        assert!(
            compiled.frame.is_trivial(),
            "leaf fn must have trivial frame"
        );
    }

    #[test]
    fn one_plus_two_emits_exactly_16_bytes() {
        let func = build_one_plus_two();
        let compiled = compile_function(&func);
        assert_eq!(compiled.bytes.len(), 16, "4 instructions × 4 bytes");
    }
}
