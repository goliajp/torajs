//! Alloca + Load + Store lowering — frame-relative slot addressing
//! and imm12-scaled accesses.
//!
//! S3-A3 adds the register-indexed variants `emit_load_dyn` and
//! `emit_store_dyn` for `InstKind::LoadDyn` / `StoreDyn`. The
//! dynamic offset Operand materializes into a scratch GPR and the
//! emitted LDR/STR uses [Xn, Xm, LSL #0] addressing (Xm holds the
//! raw byte offset).

use torajs_core::ssa::{Inst, Operand, Type};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr, operand_is_f64};
use super::{
    FP_SCRATCH_LHS, FP_SCRATCH_RESULT, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS,
    OP_SCRATCH_TMP, write_def_spill_fpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{
    add_imm, ldr_d_imm12, ldr_d_reg, ldr_d_reg_lsl3, ldr_x_imm12, ldr_x_reg, ldr_x_reg_lsl3,
    ldrb_w_reg, ldur_x_imm9, str_d_imm12, str_d_reg, str_d_reg_lsl3, str_x_imm12, str_x_reg,
    str_x_reg_lsl3, stur_x_imm9,
};
use crate::reg::Gpr;
use crate::regalloc::Assignment;

/// Emit `ADD Xrd, SP, #slot_offset` — materialize the alloca's stack
/// slot address into the GPR the allocator assigned to the result.
pub fn emit_alloca(bytes: &mut Vec<u8>, inst: &Inst, alloc: &Assignment) {
    let result_vid = inst.result.expect("Alloca must have result");
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    let slot_offset = alloc.alloca_offset_of(result_vid);
    assert!(
        slot_offset < 4096,
        "S3-A2 slot offset {slot_offset} must fit ADD imm12 (larger frames land in S3-B with shifted-imm or scratch path)",
    );
    write_u32(bytes, add_imm(dst, Gpr::SP, slot_offset as u16));
    write_def_spill_gpr(bytes, spill_off, dst);
}

/// Emit `LDR Xd/Dd, [Xn, #offset]` — load a 64-bit value from the
/// pointer operand at the given byte offset.
///
/// `ty == Type::F64` dispatches to the FP D-form (LDR Dd, …); all
/// other 64-bit-shaped types use the GPR X-form. The pointer operand
/// (`ptr`) is always materialized into a GPR — there's no FP base-
/// addressing form on aarch64.
pub fn emit_load(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    ty: &Type,
    ptr: &Operand,
    offset: u64,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("Load must have result");
    assert!(
        offset < 32760,
        "S3-A2 Load offset {offset} must fit imm12*8 (= 32760 max); larger offsets land in S3-B with scratch path"
    );
    let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_LHS, alloc);
    if *ty == Type::F64 {
        let (dst, spill_off) = alloc.def_fpr(result_vid, FP_SCRATCH_RESULT);
        write_u32(bytes, ldr_d_imm12(dst, rn, offset as u32));
        write_def_spill_fpr(bytes, spill_off, dst);
    } else {
        let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
        // ssa_lower emits `Load _, ptr +4` and similar non-8-aligned
        // offsets when accessing struct/heap-header fields packed
        // at byte granularity (refcount/type_tag/flags). The scaled
        // LDR imm12 form requires the offset be a multiple of 8 —
        // pre-fix the encoder silently floor-divided unaligned
        // offsets by 8, redirecting the load to the wrong address
        // (root cause of fn-return dynobj typeof returning "string"
        // because the type_tag read at +4 collided with offset 0's
        // refcount slot). Use unscaled LDUR (signed imm9, ±256) for
        // unaligned offsets and the canonical LDR scaled imm12 form
        // for aligned ones; offsets outside both ranges (≥4096 or
        // ≥256 unaligned) still hit the existing 32760 assert.
        if offset % 8 == 0 {
            write_u32(bytes, ldr_x_imm12(dst, rn, offset as u32));
        } else {
            assert!(
                offset < 256,
                "non-aligned Load offset {offset} must fit LDUR imm9 (= 255 max); larger needs scratch-reg path"
            );
            write_u32(bytes, ldur_x_imm9(dst, rn, offset as i32));
        }
        write_def_spill_gpr(bytes, spill_off, dst);
    }
}

/// A stored constant zero — f64 `+0.0` and i64 `0` share the all-
/// zero bit pattern — needs no materialization at all: `STR XZR`
/// writes it directly (S7 knife r0; a 16-slot `[0, 0, …]` array
/// literal paid MOVZ + FMOV + STR per slot). `-0.0` has a sign bit
/// and keeps the FPR path.
fn store_val_is_zero(val: &Operand) -> bool {
    match val {
        Operand::ConstI64(0) => true,
        Operand::ConstF64(c) => c.to_bits() == 0,
        _ => false,
    }
}

/// Emit `STR Xs/Ds, [Xn, #offset]` — store a 64-bit value to the
/// pointer operand at the given byte offset. The value operand
/// (`val`) decides FP vs GPR via `operand_is_f64`.
pub fn emit_store(
    bytes: &mut Vec<u8>,
    val: &Operand,
    ptr: &Operand,
    offset: u64,
    alloc: &Assignment,
) {
    assert!(
        offset < 32760,
        "S3-A2 Store offset {offset} must fit imm12*8 (= 32760 max)"
    );
    if store_val_is_zero(val) {
        let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_RHS, alloc);
        if offset % 8 == 0 {
            write_u32(bytes, str_x_imm12(Gpr::XZR, rn, offset as u32));
        } else {
            assert!(
                offset < 256,
                "non-aligned zero Store offset {offset} must fit STUR imm9"
            );
            write_u32(bytes, stur_x_imm9(Gpr::XZR, rn, offset as i32));
        }
        return;
    }
    if operand_is_f64(val, alloc) {
        let rs = materialize_operand_fpr(bytes, val, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
        let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_RHS, alloc);
        // D-form scaled store: not yet ssa_lower-emitted at non-
        // aligned offsets; keep the aligned-only form until a real
        // need surfaces (mirror the LDR path's safety strategy).
        write_u32(bytes, str_d_imm12(rs, rn, offset as u32));
    } else {
        let rs = materialize_operand_gpr(bytes, val, OP_SCRATCH_LHS, alloc);
        let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_RHS, alloc);
        // See `emit_load`'s comment for why non-8-aligned offsets
        // route through STUR — pre-fix this branch went through
        // STR-scaled and silently floor-divided to the wrong slot,
        // breaking every heap-header field write (type_tag@+4 etc.).
        if offset % 8 == 0 {
            write_u32(bytes, str_x_imm12(rs, rn, offset as u32));
        } else {
            assert!(
                offset < 256,
                "non-aligned Store offset {offset} must fit STUR imm9 (= 255 max)"
            );
            write_u32(bytes, stur_x_imm9(rs, rn, offset as i32));
        }
    }
}

/// Emit `LDR Xd/Dd, [Xn, Xm, LSL #0]` — load 64-bit, register-
/// indexed. `ty == Type::F64` dispatches to LDR Dd, [Xn, Xm].
pub fn emit_load_dyn(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    ty: &Type,
    base: &Operand,
    dyn_offset: &Operand,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("LoadDyn must have result");
    let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_LHS, alloc);
    let rm = materialize_operand_gpr(bytes, dyn_offset, OP_SCRATCH_RHS, alloc);
    if *ty == Type::F64 {
        let (dst, spill_off) = alloc.def_fpr(result_vid, FP_SCRATCH_RESULT);
        write_u32(bytes, ldr_d_reg(dst, rn, rm));
        write_def_spill_fpr(bytes, spill_off, dst);
    } else {
        let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
        write_u32(bytes, ldr_x_reg(dst, rn, rm));
        write_def_spill_gpr(bytes, spill_off, dst);
    }
}

/// Emit `LDR Xd/Dd, [Xn, Xm, LSL #3]` — scaled register-indexed
/// load: the operand is a slot INDEX, the AGU does the ×8 (S7 knife
/// c2 — the standalone shift the e-graph folded away sat on the
/// address dependency chain every iteration).
pub fn emit_load_dyn_scaled8(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    ty: &Type,
    base: &Operand,
    idx: &Operand,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("LoadDynScaled8 must have result");
    let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_LHS, alloc);
    let rm = materialize_operand_gpr(bytes, idx, OP_SCRATCH_RHS, alloc);
    if *ty == Type::F64 {
        let (dst, spill_off) = alloc.def_fpr(result_vid, FP_SCRATCH_RESULT);
        write_u32(bytes, ldr_d_reg_lsl3(dst, rn, rm));
        write_def_spill_fpr(bytes, spill_off, dst);
    } else {
        let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
        write_u32(bytes, ldr_x_reg_lsl3(dst, rn, rm));
        write_def_spill_gpr(bytes, spill_off, dst);
    }
}

/// Emit `LDRB Wd, [Xn, Xm]` — byte load, zero-extended to the full
/// Xd (Wd write zeroes bits 32..64; LDRB itself zeroes bits 8..32).
/// One instruction per byte probe — the S7-r2 SplitIter inline scan.
pub fn emit_load_u8_dyn(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    base: &Operand,
    idx: &Operand,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("LoadU8Dyn must have result");
    let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_LHS, alloc);
    let rm = materialize_operand_gpr(bytes, idx, OP_SCRATCH_RHS, alloc);
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    write_u32(bytes, ldrb_w_reg(dst, rn, rm));
    write_def_spill_gpr(bytes, spill_off, dst);
}

/// Emit `STR Xs/Ds, [Xn, Xm, LSL #3]` — scaled register-indexed
/// store, symmetric to [`emit_load_dyn_scaled8`].
pub fn emit_store_dyn_scaled8(
    bytes: &mut Vec<u8>,
    val: &Operand,
    base: &Operand,
    idx: &Operand,
    alloc: &Assignment,
) {
    if store_val_is_zero(val) {
        let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_RHS, alloc);
        let rm = materialize_operand_gpr(bytes, idx, OP_SCRATCH_TMP, alloc);
        write_u32(bytes, str_x_reg_lsl3(Gpr::XZR, rn, rm));
        return;
    }
    if operand_is_f64(val, alloc) {
        let rs = materialize_operand_fpr(bytes, val, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
        let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_RHS, alloc);
        let rm = materialize_operand_gpr(bytes, idx, OP_SCRATCH_TMP, alloc);
        write_u32(bytes, str_d_reg_lsl3(rs, rn, rm));
    } else {
        let rs = materialize_operand_gpr(bytes, val, OP_SCRATCH_LHS, alloc);
        let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_RHS, alloc);
        let rm = materialize_operand_gpr(bytes, idx, OP_SCRATCH_TMP, alloc);
        write_u32(bytes, str_x_reg_lsl3(rs, rn, rm));
    }
}

/// Emit `STR Xs/Ds, [Xn, Xm, LSL #0]` — store 64-bit, register-
/// indexed. Value operand decides FP vs GPR.
pub fn emit_store_dyn(
    bytes: &mut Vec<u8>,
    val: &Operand,
    base: &Operand,
    dyn_offset: &Operand,
    alloc: &Assignment,
) {
    if store_val_is_zero(val) {
        let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_RHS, alloc);
        let rm = materialize_operand_gpr(bytes, dyn_offset, OP_SCRATCH_TMP, alloc);
        write_u32(bytes, str_x_reg(Gpr::XZR, rn, rm));
        return;
    }
    if operand_is_f64(val, alloc) {
        let rs = materialize_operand_fpr(bytes, val, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
        let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_RHS, alloc);
        let rm = materialize_operand_gpr(bytes, dyn_offset, OP_SCRATCH_TMP, alloc);
        write_u32(bytes, str_d_reg(rs, rn, rm));
    } else {
        let rs = materialize_operand_gpr(bytes, val, OP_SCRATCH_LHS, alloc);
        let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_RHS, alloc);
        let rm = materialize_operand_gpr(bytes, dyn_offset, OP_SCRATCH_TMP, alloc);
        write_u32(bytes, str_x_reg(rs, rn, rm));
    }
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
    /// A stored constant zero takes STR XZR (S7 knife r0) — no MOVZ,
    /// no FMOV. Same fixture shape as `alloca_store_load_42` but the
    /// stored value is `ConstI64(0)` / `ConstF64(0.0)`; both must
    /// produce the identical XZR store word.
    #[test]
    fn store_const_zero_uses_xzr() {
        use torajs_core::ssa::{
            Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
        };
        for zero in [Operand::ConstI64(0), Operand::ConstF64(0.0)] {
            let v0 = ValueId(0);
            let v1 = ValueId(1);
            let func = Function {
                name: "store_zero".into(),
                params: Vec::new(),
                ret: Type::I64,
                values: vec![
                    ValueInfo {
                        ty: Type::Ptr,
                        name: None,
                    },
                    ValueInfo {
                        ty: Type::I64,
                        name: None,
                    },
                ],
                blocks: vec![Block {
                    id: BlockId(0),
                    insts: vec![
                        Inst {
                            result: Some(v0),
                            kind: InstKind::AllocaBytes(8),
                            origin: None,
                        },
                        Inst {
                            result: None,
                            kind: InstKind::Store(zero.clone(), Operand::Value(v0), 0),
                            origin: None,
                        },
                        Inst {
                            result: Some(v1),
                            kind: InstKind::Load(Type::I64, Operand::Value(v0), 0),
                            origin: None,
                        },
                    ],
                    term: Terminator::Ret(Some(Operand::Value(v1))),
                }],
                current_origin: None,
            };
            let compiled = compile_function(&func);
            let zero_store = enc::str_x_imm12(Gpr::XZR, Gpr::X13, 0).to_le_bytes();
            assert!(
                compiled.bytes.windows(4).any(|w| w == zero_store),
                "expected STR XZR store word for {zero:?}"
            );
            // and no FMOV materialization word anywhere
            let fmov = enc::fmov_d_from_x(crate::reg::Fpr::V16, Gpr::X9).to_le_bytes();
            assert!(
                !compiled.bytes.windows(4).any(|w| w == fmov),
                "no FPR materialization expected for {zero:?}"
            );
        }
    }

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
