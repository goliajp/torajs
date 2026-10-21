//! Alloca + Load + Store lowering — frame-relative slot addressing
//! and imm12-scaled accesses.
//!
//! S3-A3 adds the register-indexed variants `emit_load_dyn` and
//! `emit_store_dyn` for `InstKind::LoadDyn` / `StoreDyn`. The
//! dynamic offset Operand materializes into a scratch GPR and the
//! emitted LDR/STR uses [Xn, Xm, LSL #0] addressing (Xm holds the
//! raw byte offset).

use torajs_core::ssa::{Inst, Operand, Type};

use super::operand::{
    materialize_const_i64, materialize_operand_fpr, materialize_operand_gpr, operand_is_f64,
};
use super::store_pair::{word_in_gpr, zero_word};
use super::{
    FP_SCRATCH_LHS, FP_SCRATCH_RESULT, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS,
    OP_SCRATCH_TMP, write_def_spill_fpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{
    ldr_d_imm12, ldr_d_reg, ldr_d_reg_lsl3, ldr_w_imm12, ldr_w_reg, ldr_x_imm12, ldr_x_reg,
    ldr_x_reg_lsl3, ldrb_w_reg, ldrh_w_reg_lsl1, ldur_w_imm9, ldur_x_imm9, str_d_imm12, str_d_reg,
    str_d_reg_lsl3, str_w_imm12, str_w_reg, str_x_imm12, str_x_reg, str_x_reg_lsl3, stur_w_imm9,
    stur_x_imm9,
};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;

/// Emit `ADD Xrd, SP, #slot_offset` — materialize the alloca's stack
/// slot address into the GPR the allocator assigned to the result
/// (two ADDs past the immediate's 4 KiB, see `slot::sp_add`). A
/// spilled result emits nothing: every read recomputes the address
/// (`operand::materialize_operand_gpr`), one ADD against a store
/// here and a load there.
pub fn emit_alloca(bytes: &mut Vec<u8>, inst: &Inst, alloc: &Assignment) {
    let result_vid = inst.result.expect("Alloca must have result");
    if let Reg::Gpr(dst) = alloc.of(result_vid) {
        super::slot::sp_add(bytes, dst, alloc.alloca_offset_of(result_vid));
    }
}

/// An offset past every immediate form — scaled imm12 (aligned,
/// < 32760) and unscaled imm9 (unaligned, < 256) — rides the
/// register-indexed form instead: the byte offset materializes into
/// `OP_SCRATCH_TMP` (free here — the value takes LHS, the pointer
/// RHS / LHS, the result its own home) and the access becomes
/// `[Xn, Xm]`. 555-01: a 65k-element array literal stores its slots
/// at offsets up to 524 KiB. Answers the index register, or `None`
/// when an immediate form fits.
fn wide_offset(bytes: &mut Vec<u8>, offset: u64, scale: u64) -> Option<Gpr> {
    let fits = if offset.is_multiple_of(scale) {
        offset < 4095 * scale
    } else {
        offset < 256
    };
    if fits {
        return None;
    }
    materialize_const_i64(bytes, OP_SCRATCH_TMP, offset as i64);
    Some(OP_SCRATCH_TMP)
}

/// The encoders of one GPR access (width and direction): register-
/// indexed, scaled imm12 when the offset is a multiple of `scale`,
/// unscaled imm9 otherwise.
struct GprForms {
    reg: fn(Gpr, Gpr, Gpr) -> u32,
    imm12: fn(Gpr, Gpr, u32) -> u32,
    imm9: fn(Gpr, Gpr, i32) -> u32,
    scale: u64,
}

const X_LOAD: GprForms = GprForms {
    reg: ldr_x_reg,
    imm12: ldr_x_imm12,
    imm9: ldur_x_imm9,
    scale: 8,
};
const X_STORE: GprForms = GprForms {
    reg: str_x_reg,
    imm12: str_x_imm12,
    imm9: stur_x_imm9,
    scale: 8,
};
const W_LOAD: GprForms = GprForms {
    reg: ldr_w_reg,
    imm12: ldr_w_imm12,
    imm9: ldur_w_imm9,
    scale: 4,
};
const W_STORE: GprForms = GprForms {
    reg: str_w_reg,
    imm12: str_w_imm12,
    imm9: stur_w_imm9,
    scale: 4,
};

// ssa_lower reads and writes heap-header fields at offsets like +4;
// the scaled imm12 form needs a multiple of the access size, so the
// rest take the unscaled imm9 form and out-of-range offsets the
// register-indexed form (`wide_offset`)
fn emit_gpr_access(bytes: &mut Vec<u8>, f: &GprForms, rt: Gpr, rn: Gpr, offset: u64) {
    let word = match wide_offset(bytes, offset, f.scale) {
        Some(rm) => (f.reg)(rt, rn, rm),
        None if offset.is_multiple_of(f.scale) => (f.imm12)(rt, rn, offset as u32),
        None => (f.imm9)(rt, rn, offset as i32),
    };
    write_u32(bytes, word);
}

/// Emit `LDR Xd/Wd/Dd, [Xn, #offset]` — load the value at the given
/// byte offset from the pointer operand.
///
/// `ty == Type::F64` dispatches to the FP D-form (LDR Dd, …), `I32`
/// to the W-form, which reads exactly its four bytes zero-extended;
/// every other type is 64-bit shaped and takes the X-form. The
/// pointer operand (`ptr`) is always materialized into a GPR —
/// there's no FP base-addressing form on aarch64.
pub fn emit_load(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    ty: &Type,
    ptr: &Operand,
    offset: u64,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("Load must have result");
    let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_LHS, alloc);
    if *ty == Type::F64 {
        let wide = wide_offset(bytes, offset, 8);
        let (dst, spill_off) = alloc.def_fpr(result_vid, FP_SCRATCH_RESULT);
        match wide {
            Some(rm) => write_u32(bytes, ldr_d_reg(dst, rn, rm)),
            None => write_u32(bytes, ldr_d_imm12(dst, rn, offset as u32)),
        }
        write_def_spill_fpr(bytes, spill_off, dst);
    } else {
        let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
        let forms = if *ty == Type::I32 { &W_LOAD } else { &X_LOAD };
        emit_gpr_access(bytes, forms, dst, rn, offset);
        write_def_spill_gpr(bytes, spill_off, dst);
    }
}

/// Emit `STR Xs/Ws/Ds, [Xn, #offset]` — store the value operand to
/// the pointer operand at the given byte offset. The value operand
/// (`val`) decides FP vs GPR via `operand_is_f64`; `w32` (an I32
/// value) stores only its four bytes, leaving the neighbouring
/// header field untouched.
pub fn emit_store(
    bytes: &mut Vec<u8>,
    val: &Operand,
    ptr: &Operand,
    offset: u64,
    w32: bool,
    alloc: &Assignment,
) {
    let forms = if w32 { &W_STORE } else { &X_STORE };
    if zero_word(val) {
        let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_RHS, alloc);
        emit_gpr_access(bytes, forms, Gpr::XZR, rn, offset);
        return;
    }
    if let Operand::ConstF64(_) = val {
        // the bit pattern goes to memory from a GPR: no FMOV into a D
        // register first
        let rs = word_in_gpr(bytes, val, OP_SCRATCH_LHS, alloc);
        let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_RHS, alloc);
        emit_gpr_access(bytes, &X_STORE, rs, rn, offset);
        return;
    }
    if operand_is_f64(val, alloc) {
        let rs = materialize_operand_fpr(bytes, val, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
        let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_RHS, alloc);
        // D-form scaled store: not yet ssa_lower-emitted at non-
        // aligned offsets; keep the aligned-only form until a real
        // need surfaces (mirror the LDR path's safety strategy).
        match wide_offset(bytes, offset, 8) {
            Some(rm) => write_u32(bytes, str_d_reg(rs, rn, rm)),
            None => write_u32(bytes, str_d_imm12(rs, rn, offset as u32)),
        }
    } else {
        let rs = materialize_operand_gpr(bytes, val, OP_SCRATCH_LHS, alloc);
        let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_RHS, alloc);
        emit_gpr_access(bytes, forms, rs, rn, offset);
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

/// Emit `LDRH Wd, [Xn, Xm, LSL #1]` — one UTF-16 code unit at
/// `base + idx*2`, zero-extended to the full Xd.
pub fn emit_load_u16_dyn(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    base: &Operand,
    idx: &Operand,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("LoadU16Dyn must have result");
    let rn = materialize_operand_gpr(bytes, base, OP_SCRATCH_LHS, alloc);
    let rm = materialize_operand_gpr(bytes, idx, OP_SCRATCH_RHS, alloc);
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    write_u32(bytes, ldrh_w_reg_lsl1(dst, rn, rm));
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
    if zero_word(val) {
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
    if zero_word(val) {
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
    /// 555-01 — an offset past the scaled-imm12 range rides the
    /// register-indexed form with the byte offset in X11.
    #[test]
    fn wide_offset_store_load_use_register_index() {
        use torajs_core::ssa::{
            Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
        };
        let v0 = ValueId(0);
        let v1 = ValueId(1);
        let off: u64 = 40000;
        let func = Function {
            name: "wide".into(),
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
                        kind: InstKind::Store(Operand::ConstI64(7), Operand::Value(v0), off),
                        origin: None,
                    },
                    Inst {
                        result: Some(v1),
                        kind: InstKind::Load(Type::I64, Operand::Value(v0), off),
                        origin: None,
                    },
                ],
                term: Terminator::Ret(Some(Operand::Value(v1))),
            }],
            current_origin: None,
        };
        let compiled = compile_function(&func);
        let words: Vec<u32> = compiled
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        let movz = enc::movz_imm(Gpr::X11, (off & 0xFFFF) as u16, 0);
        assert!(words.contains(&movz), "offset materializes into X11");
        assert!(
            words.contains(&enc::str_x_reg(Gpr::X9, Gpr::X13, Gpr::X11)),
            "STR rides [Xn, X11]"
        );
        // LDR Xt, [X13, X11] for whatever Rt the allocator picked.
        let ldr_shape = enc::ldr_x_reg(Gpr::X0, Gpr::X13, Gpr::X11) & !0x1F;
        assert!(
            words.iter().any(|w| (w & !0x1F) == ldr_shape),
            "LDR rides [Xn, X11]"
        );
        // the offset is past the scaled imm12 reach, so no STR / LDR
        // (unsigned offset) may address the slot's base register
        assert!(
            !words
                .iter()
                .any(|w| (w & 0xFF80_0000) == 0xF900_0000 && (w >> 5) & 0x1F == Gpr::X13 as u32),
            "no scaled-immediate access off X13"
        );
    }

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
                            kind: InstKind::Store(zero, Operand::Value(v0), 0),
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
            enc::ldp_x_off(Gpr::X29, Gpr::X30, Gpr::SP, 16),
            enc::add_imm(Gpr::SP, Gpr::SP, 32),
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
            enc::ldp_x_off(Gpr::X29, Gpr::X30, Gpr::SP, 16),
            enc::add_imm(Gpr::SP, Gpr::SP, 32),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "alloca+storedyn+loaddyn: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
        assert_eq!(compiled.bytes.len(), 48, "12 inst × 4 bytes");
    }

    /// LDR / STR Xt, [SP, #imm12] at any register: the spill traffic
    /// of a value whose slot is at `off`.
    fn is_sp_slot_access(w: u32, off: u32) -> bool {
        let at = ((off / 8) << 10) | (31 << 5);
        w & !0x1F == 0xF940_0000 | at || w & !0x1F == 0xF900_0000 | at
    }

    #[test]
    fn a_spilled_alloca_address_is_recomputed_not_stored() {
        use super::super::{ModuleFacts, OP_SCRATCH_LHS, compile_function_with};
        use crate::linear_scan::allocate_linear_scan;
        use crate::reg::Reg;
        use torajs_core::ssa::{Function, InstKind, Operand, Terminator, Type};
        // `ret ptrtoint(alloca 8)`: a read no access can fold, with the
        // alloca forced into a spill slot
        let mut func = Function::new("t", Type::I64);
        let b = func.add_block();
        let a = func.append_inst(b, InstKind::AllocaBytes(8), Type::Ptr, None);
        let p = func.append_inst(b, InstKind::PtrToInt(Operand::Value(a)), Type::I64, None);
        func.set_term(b, Terminator::Ret(Some(Operand::Value(p))));
        let alloc = allocate_linear_scan(&func).with_homes(&[(a.0, Reg::SpillGpr(4096))]);
        let w: Vec<u32> = compile_function_with(&func, alloc, &ModuleFacts::default())
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        assert!(!w.iter().any(|&x| is_sp_slot_access(x, 4096)), "{w:08X?}");
        assert!(
            w.contains(&enc::add_imm(OP_SCRATCH_LHS, Gpr::SP, 0)),
            "{w:08X?}"
        );
    }
}
