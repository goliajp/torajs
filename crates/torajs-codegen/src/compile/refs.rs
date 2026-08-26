//! Address-of-symbol lowering — GlobalRef / StringRef / StaticStrRef
//! / FnAddr / BoxedEntryAddr. Each emits the canonical aarch64 `ADRP Xd, page(sym);
//! ADD Xd, Xd, #pageoff(sym)` pair, paired with `Page21` +
//! `PageOff12` relocs that torajs-link patches at symbol resolution.
//!
//! Symbol name conventions used in the emitted relocs:
//!
//! ```text
//!   GlobalRef(name)        → name verbatim
//!   StringRef(StringId(n)) → "__torajs_str_dyn_{n}"
//!   StaticStrRef(...)      → "__torajs_str_lit_{n}"
//!   FnAddr(FuncId(n))      → "__torajs_fn_{n}"
//!   BoxedEntryAddr(FuncId(n)) → "__torajs_boxed_{n}"
//! ```
//!
//! These prefixes are a S4-B convention; torajs-obj (#7) and torajs-
//! link (#8) can canonicalize when they land. The codegen-side
//! invariant is just: `Reloc.target_sym` uniquely identifies the
//! resolution target; downstream resolves the name → address.

use torajs_core::ssa::{FuncId, Inst, StringId};

use super::{OP_SCRATCH_RESULT_GPR, write_def_spill_gpr, write_u32};
use crate::enc::{add_imm, adrp};
use crate::reg::Gpr;
use crate::regalloc::Assignment;
use crate::reloc::{Reloc, RelocKind};

/// Emit `ADRP Xd, page(sym); ADD Xd, Xd, #pageoff(sym)` into `bytes`
/// and record the matching `Page21` + `PageOff12` reloc entries.
fn emit_adrp_add_pair(bytes: &mut Vec<u8>, relocs: &mut Vec<Reloc>, dst: Gpr, target_sym: String) {
    let adrp_byte_offset = bytes.len() as u32;
    relocs.push(Reloc {
        byte_offset: adrp_byte_offset,
        kind: RelocKind::Page21 {
            target_sym: target_sym.clone(),
        },
    });
    write_u32(bytes, adrp(dst, 0));

    let add_byte_offset = bytes.len() as u32;
    relocs.push(Reloc {
        byte_offset: add_byte_offset,
        kind: RelocKind::PageOff12 { target_sym },
    });
    write_u32(bytes, add_imm(dst, dst, 0));
}

pub fn emit_global_ref(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    name: &str,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("GlobalRef must have result");
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    emit_adrp_add_pair(bytes, relocs, dst, name.to_string());
    write_def_spill_gpr(bytes, spill_off, dst);
}

pub fn emit_string_ref(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    string_id: StringId,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("StringRef must have result");
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    emit_adrp_add_pair(
        bytes,
        relocs,
        dst,
        format!("__torajs_str_dyn_{}", string_id.0),
    );
    write_def_spill_gpr(bytes, spill_off, dst);
}

pub fn emit_static_str_ref(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    string_id: StringId,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("StaticStrRef must have result");
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    emit_adrp_add_pair(
        bytes,
        relocs,
        dst,
        format!("__torajs_str_lit_{}", string_id.0),
    );
    write_def_spill_gpr(bytes, spill_off, dst);
}

pub fn emit_fn_addr(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    func_id: FuncId,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("FnAddr must have result");
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    emit_adrp_add_pair(bytes, relocs, dst, format!("__torajs_fn_{}", func_id.0));
    write_def_spill_gpr(bytes, spill_off, dst);
}

/// Same pair as [`emit_fn_addr`] under the mint-only alias
/// `__torajs_boxed_<n>` — torajs-link registers both aliases to the
/// same vaddr and judges only this one (`dead_strip_elide`).
pub fn emit_boxed_entry_addr(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    func_id: FuncId,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("BoxedEntryAddr must have result");
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    emit_adrp_add_pair(bytes, relocs, dst, format!("__torajs_boxed_{}", func_id.0));
    write_def_spill_gpr(bytes, spill_off, dst);
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::super::test_fixtures::words_to_le_bytes;
    use crate::enc;
    use crate::reg::Gpr;
    use crate::reloc::RelocKind;
    use torajs_core::ssa::{
        Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
    };

    /// S4-B acceptance — GlobalRef chain to the return value:
    ///
    /// ```text
    /// fn ref_global() -> i64 {
    ///     let p = global "global_x";    // v0 (Ptr) → X13 scratch
    ///     ret (p as i64)                // v1 (I64) → X0 ret
    /// }
    /// ```
    ///
    /// Expected 4-instruction sequence (leaf, trivial frame):
    ///
    /// ```text
    ///   ADRP x13, page(global_x)    (with Page21 reloc at 0)
    ///   ADD  x13, x13, #pageoff(.)  (with PageOff12 reloc at 4)
    ///   MOV  x0, x13                (PtrToInt)
    ///   RET
    /// ```
    #[test]
    fn global_ref_byte_equal_and_records_reloc_pair() {
        let v0 = ValueId(0); // GlobalRef result (Ptr) → X13
        let v1 = ValueId(1); // PtrToInt result (I64) → X0 (ret)
        let func = Function {
            name: "ref_global".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::Ptr,
                    name: Some("v0".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v1".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    Inst {
                        result: Some(v0),
                        kind: InstKind::GlobalRef("global_x".to_string()),
                        origin: None,
                    },
                    Inst {
                        result: Some(v1),
                        kind: InstKind::PtrToInt(Operand::Value(v0)),
                        origin: None,
                    },
                ],
                term: Terminator::Ret(Some(Operand::Value(v1))),
            }],
            current_origin: None,
        };
        let compiled = compile_function(&func);

        let expected = words_to_le_bytes(&[
            enc::adrp(Gpr::X13, 0),
            enc::add_imm(Gpr::X13, Gpr::X13, 0),
            enc::mov_x_reg(Gpr::X0, Gpr::X13),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "global_ref: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
        assert_eq!(compiled.bytes.len(), 16, "4 inst × 4 bytes");

        // Two relocs: Page21 at byte 0, PageOff12 at byte 4. Both
        // carry the same target_sym (`global_x`).
        assert_eq!(compiled.relocs.len(), 2);
        assert_eq!(compiled.relocs[0].byte_offset, 0);
        match &compiled.relocs[0].kind {
            RelocKind::Page21 { target_sym } => assert_eq!(target_sym, "global_x"),
            other => panic!("expected Page21 at byte 0, got {other:?}"),
        }
        assert_eq!(compiled.relocs[1].byte_offset, 4);
        match &compiled.relocs[1].kind {
            RelocKind::PageOff12 { target_sym } => assert_eq!(target_sym, "global_x"),
            other => panic!("expected PageOff12 at byte 4, got {other:?}"),
        }

        // Frame: trivial (leaf, no calls, no alloca).
        assert!(compiled.frame.is_trivial());
    }
}
