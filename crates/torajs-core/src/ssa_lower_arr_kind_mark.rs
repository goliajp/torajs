//! Typed-array elem-kind marking helpers for `LowerCtx` extracted
//! from [`crate::ssa_lower_any_box`] (RFC 20260708-typed-arr-oob-read
//! chunk 2 file-size split; bodies verbatim from chunk 626 / 621).
//!
//! When a typed `Type::Arr(..)` value crosses into the `any` world
//! (Arr<Any> param, print_any walker, container store), one
//! `__torajs_arr_mark_kind(arr, chain)` call makes the heap block
//! self-describing (the V8 ElementsKind shape). See
//! `emit_arr_mark_kind` for the chain encoding.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    pub(crate) fn emit_arr_mark_kind(&mut self, val: &Operand) {
        let val_ty = self.operand_ty(val);
        let Type::Arr(id) = val_ty else { return };
        let elem = self.arr_layouts[id.0 as usize].clone();
        let chain = self.arr_kind_chain(&elem, 0);
        if chain != 0 {
            self.f.append_void(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.arr_mark_kind,
                    vec![val.clone(), Operand::ConstI64(chain as i64)],
                ),
            );
        }
    }
    pub(crate) fn mark_arr_arg_for_any_param(&mut self, expected: Type, op: &Operand) {
        if let Type::Arr(pid) = expected
            && matches!(self.arr_layouts[pid.0 as usize], Type::Any)
        {
            self.emit_arr_mark_kind(op);
        }
    }
    /// Element-kind chain for an array layout — 3 bits per nesting
    /// level, little-endian. Also read by
    /// [`crate::ssa_lower_array::alloc_stack_arr`], which bakes the
    /// level-0 kind straight into the alloca's header (a stack
    /// literal carries `FLAG_STATIC_LITERAL`, which the runtime
    /// marker refuses to write).
    pub(crate) fn arr_kind_chain(&self, elem: &Type, depth: u32) -> u64 {
        arr_kind_chain_of(self.arr_layouts, elem, depth)
    }
}

/// [`LowerCtx::arr_kind_chain`] without a `LowerCtx` — the boxed-entry
/// synthesis runs before one exists and needs the same answer for a
/// body's ARRAY RETURN (RFC 20260823-proxy-substrate 刀 4b).
pub(crate) fn arr_kind_chain_of(arr_layouts: &[Type], elem: &Type, depth: u32) -> u64 {
    if depth >= 21 {
        return 0;
    }
    match elem {
        Type::I64 | Type::I32 => 1,
        Type::F64 => 2,
        Type::Bool => 3,
        // Arr<Any> carries FLAG_ARR_ANY — self-describing, no mark.
        Type::Any => 0,
        Type::Arr(id) => {
            let inner = arr_layouts[id.0 as usize].clone();
            4 | (arr_kind_chain_of(arr_layouts, &inner, depth + 1) << 3)
        }
        t if t.is_refcounted() => 4,
        _ => 0,
    }
}
