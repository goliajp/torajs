//! Alloca + Load + Store lowering — frame-relative slot addressing
//! and imm12-scaled accesses.
//!
//! S3-A3 adds the register-indexed variants `emit_load_dyn` and
//! `emit_store_dyn` for `InstKind::LoadDyn` / `StoreDyn`. The
//! dynamic offset Operand materializes into a scratch GPR and the
//! emitted LDR/STR uses [Xn, Xm, LSL #0] addressing (Xm holds the
//! raw byte offset).

use torajs_core::ssa::{Inst, Operand, Type};

use super::operand::materialize_operand_gpr;
use super::write_u32;
use super::{OP_SCRATCH_LHS, OP_SCRATCH_RHS, OP_SCRATCH_TMP};
use crate::enc::{add_imm, ldr_x_imm12, ldr_x_reg, str_x_imm12, str_x_reg};
use crate::reg::Gpr;
use crate::regalloc::Assignment;

/// Emit `ADD Xrd, SP, #slot_offset` — materialize the alloca's stack
/// slot address into the GPR the allocator assigned to the result.
pub fn emit_alloca(bytes: &mut Vec<u8>, inst: &Inst, alloc: &Assignment) {
    let result_vid = inst.result.expect("Alloca must have result");
    let dst = alloc.of(result_vid).as_gpr();
    let slot_offset = alloc.alloca_offset_of(result_vid);
    assert!(
        slot_offset < 4096,
        "S3-A2 slot offset {slot_offset} must fit ADD imm12 (larger frames land in S3-B with shifted-imm or scratch path)",
    );
    write_u32(bytes, add_imm(dst, Gpr::SP, slot_offset as u16));
}

/// Emit `LDR Xd, [Xn, #offset]` — load a 64-bit value from the
/// pointer operand at the given byte offset.
///
/// S3-A2 covers 64-bit (Xd) loads only; F64-typed loads via
/// LDR Dd,[Xn,#imm] land in S3-B alongside FP store. The `_ty`
/// parameter is reserved for the future dispatch.
pub fn emit_load(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    _ty: &Type,
    ptr: &Operand,
    offset: u64,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("Load must have result");
    let dst = alloc.of(result_vid).as_gpr();
    let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_LHS, alloc);
    assert!(
        offset < 32760,
        "S3-A2 Load offset {offset} must fit imm12*8 (= 32760 max); larger offsets land in S3-B with scratch path"
    );
    write_u32(bytes, ldr_x_imm12(dst, rn, offset as u32));
}

/// Emit `STR Xs, [Xn, #offset]` — store a 64-bit value to the
/// pointer operand at the given byte offset.
pub fn emit_store(
    bytes: &mut Vec<u8>,
    val: &Operand,
    ptr: &Operand,
    offset: u64,
    alloc: &Assignment,
) {
    let rs = materialize_operand_gpr(bytes, val, OP_SCRATCH_LHS, alloc);
    let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_RHS, alloc);
    assert!(
        offset < 32760,
        "S3-A2 Store offset {offset} must fit imm12*8 (= 32760 max)"
    );
    write_u32(bytes, str_x_imm12(rs, rn, offset as u32));
}

/// Emit `LDR Xd, [Xn, Xm, LSL #0]` — load 64-bit, register-indexed.
///
/// `_ty` reserved for future int / fp dispatch (FP D-form lands in
/// S3-A4 alongside FP Load/Store-imm).
pub fn emit_load_dyn(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    _ty: &Type,
    base: &Operand,
    dyn_offset: &Operand,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("LoadDyn must have result");
    let dst = alloc.of(result_vid).as_gpr();
    let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_LHS, alloc);
    let rm = materialize_operand_gpr(bytes, dyn_offset, OP_SCRATCH_RHS, alloc);
    write_u32(bytes, ldr_x_reg(dst, rn, rm));
}

/// Emit `STR Xs, [Xn, Xm, LSL #0]` — store 64-bit, register-indexed.
pub fn emit_store_dyn(
    bytes: &mut Vec<u8>,
    val: &Operand,
    base: &Operand,
    dyn_offset: &Operand,
    alloc: &Assignment,
) {
    let rs = materialize_operand_gpr(bytes, val, OP_SCRATCH_LHS, alloc);
    let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_RHS, alloc);
    let rm = materialize_operand_gpr(bytes, dyn_offset, OP_SCRATCH_TMP, alloc);
    write_u32(bytes, str_x_reg(rs, rn, rm));
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::super::test_fixtures::{
        build_alloca_store_dyn_load_dyn_42, build_alloca_store_load_42, words_to_le_bytes,
    };
    use crate::enc;
    use crate::reg::Gpr;

    /// Canonical S3-A2 acceptance fixture:
    ///
    /// ```text
    /// fn alloca_store_load_42() -> i64 {
    ///     let p = alloca 8;      // v0 (Ptr), allocated x13 (first scratch)
    ///     *p = 42;
    ///     ret *p                 // v1 (I64), allocated x0 (ret slot)
    /// }
    /// ```
    ///
    /// Expected 10-instruction sequence:
    ///
    /// ```text
    ///   prologue:
    ///     STP x29, x30, [SP, #-16]!     0xA9BF7BFD
    ///     MOV x29, sp                   0x910003FD
    ///     SUB sp, sp, #16               0xD10043FF
    ///   body:
    ///     ADD x13, sp, #0               (alloca v0 → x13 = sp+0)
    ///     MOVZ x9, #42                  (materialize ConstI64(42))
    ///     STR x9, [x13, #0]             (Store)
    ///     LDR x0, [x13, #0]             (Load v1 → x0)
    ///   epilogue:
    ///     ADD sp, sp, #16               0x910043FF
    ///     LDP x29, x30, [SP], #16       0xA8C17BFD
    ///     RET                           0xD65F03C0
    /// ```
    #[test]
    fn alloca_store_load_42_byte_equal() {
        let func = build_alloca_store_load_42();
        let compiled = compile_function(&func);

        let expected = words_to_le_bytes(&[
            enc::stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16),
            enc::add_imm(Gpr::X29, Gpr::SP, 0),
            enc::sub_imm(Gpr::SP, Gpr::SP, 16),
            enc::add_imm(Gpr::X13, Gpr::SP, 0),
            enc::movz_imm(Gpr::X9, 42, 0),
            enc::str_x_imm12(Gpr::X9, Gpr::X13, 0),
            enc::ldr_x_imm12(Gpr::X0, Gpr::X13, 0),
            enc::add_imm(Gpr::SP, Gpr::SP, 16),
            enc::ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "alloca_store_load: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
        assert_eq!(compiled.bytes.len(), 40, "10 inst × 4 bytes");
        assert_eq!(compiled.frame.alloca_bytes, 16);
        assert!(!compiled.frame.is_trivial());
    }

    /// S3-A3 acceptance — alloca 16; storedyn at +8; loaddyn at +8.
    ///
    /// Verifies register-indexed addressing through StoreDyn +
    /// LoadDyn end-to-end. Sequence:
    ///
    /// ```text
    ///   prologue (3 inst)
    ///   ADD x13, sp, #0           (alloca v0 → x13)
    ///   MOVZ x9, #42              (materialize val)
    ///   MOVZ x11, #8              (materialize dyn_offset for store)
    ///   STR x9, [x13, x11]        (store via reg-index)
    ///   MOVZ x10, #8              (materialize dyn_offset for load)
    ///   LDR x0, [x13, x10]        (load via reg-index → x0 ret)
    ///   epilogue + RET (3 inst)
    /// ```
    ///
    /// 12 instructions × 4 = 48 bytes.
    #[test]
    fn alloca_storedyn_loaddyn_42_byte_equal() {
        let func = build_alloca_store_dyn_load_dyn_42();
        let compiled = compile_function(&func);

        let expected = words_to_le_bytes(&[
            // prologue
            enc::stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16),
            enc::add_imm(Gpr::X29, Gpr::SP, 0),
            enc::sub_imm(Gpr::SP, Gpr::SP, 16),
            // alloca v0
            enc::add_imm(Gpr::X13, Gpr::SP, 0),
            // StoreDyn: val → x9, ptr = x13 (alloc.of v0), dyn_offset → x11
            enc::movz_imm(Gpr::X9, 42, 0),
            enc::movz_imm(Gpr::X11, 8, 0),
            enc::str_x_reg(Gpr::X9, Gpr::X13, Gpr::X11),
            // LoadDyn: ptr = x13, dyn_offset → x10, dst = x0
            enc::movz_imm(Gpr::X10, 8, 0),
            enc::ldr_x_reg(Gpr::X0, Gpr::X13, Gpr::X10),
            // epilogue
            enc::add_imm(Gpr::SP, Gpr::SP, 16),
            enc::ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "alloca+storedyn+loaddyn: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
        assert_eq!(compiled.bytes.len(), 48, "12 inst × 4 bytes");
    }
}
