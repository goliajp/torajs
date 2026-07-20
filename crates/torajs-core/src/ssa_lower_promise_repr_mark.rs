//! Promise value-repr stamping (RFC 20260720-anylane-promise-methods
//! knife 1) — the Promise mirror of [`crate::ssa_lower_arr_kind_mark`].
//!
//! A Promise cell's `value` slot is a repr-blind i64; the typed tier
//! reads it through statically-typed lowering, but the any-lane
//! `.then`/`.catch` bridge must box the settled value into an
//! AnyValue, which needs the storage form ON the cell. The SSA
//! `Type::Promise` is type-erased (no inner T — check.rs's
//! `Promise(Box<Type>)` erases here), so stamping at the box_to_any
//! boundary is impossible; instead MINT sites stamp, where the
//! resolved/rejected VALUE's static type fixes the form
//! (`Promise.resolve(v)` / `.reject(v)` lowering). Runtime-side value
//! sharing (`resolve_thenable`) copies the byte along.
//!
//! An unstamped cell (`REPR_UNSTAMPED` = the alloc default) keeps the
//! bridge's loud TypeError — a mint family not yet wired (combinator
//! results, chain results) degrades to today's behavior, never to a
//! silent mis-box.
//!
//! Codes mirror `torajs-promise/src/layout.rs::REPR_*` — must move in
//! lockstep.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// RFC 20260720-promise-any-cb knife 1 — bit 8 of the then/catch
/// kernels' `ret_repr` parameter: set when the handler's first
/// parameter is `any`, so the kernel boxes the settled value per the
/// source cell's repr stamp before invoking. Low byte stays the
/// return-repr code. Mirrors
/// `torajs-promise/src/then_box.rs::PARAM_ANY_FLAG` — must move in
/// lockstep.
pub(crate) const PARAM_ANY_FLAG: i64 = 256;

pub(crate) const REPR_I64: i64 = 1;
pub(crate) const REPR_F64: i64 = 2;
pub(crate) const REPR_BOOL: i64 = 3;
pub(crate) const REPR_STR: i64 = 4;
pub(crate) const REPR_HEAP: i64 = 5;
pub(crate) const REPR_ANY: i64 = 6;
pub(crate) const REPR_VOID: i64 = 7;
pub(crate) const REPR_NULL: i64 = 8;

/// The repr code for a mint-site value, given the STORED form the
/// mint lowering actually emits (not the surface arg type alone):
/// `stored_as_f64` is true when the site bitcasts to f64 bits (an F64
/// arg, or an I64 arg whose value slot is f64-widened), `is_null`
/// when the operand is the lowered `null` literal. `None` = a form
/// the bridge doesn't decode yet — leave the cell UNSTAMPED (loud);
/// the Map/Set/BigInt family also lands here because the mint's
/// is_heap dispatch stores them raw (pre-existing storage-face gap,
/// RFC "不入本 RFC" note).
pub(crate) fn promise_value_repr(arg_ty: &Type, stored_as_f64: bool, is_null: bool) -> Option<i64> {
    if is_null {
        return Some(REPR_NULL);
    }
    if stored_as_f64 {
        return Some(REPR_F64);
    }
    match arg_ty {
        Type::I64 | Type::I32 => Some(REPR_I64),
        Type::F64 => Some(REPR_F64),
        Type::Bool => Some(REPR_BOOL),
        Type::Str => Some(REPR_STR),
        Type::Any => Some(REPR_ANY),
        Type::Void => Some(REPR_VOID),
        Type::Substr
        | Type::Obj(_)
        | Type::Arr(_)
        | Type::Closure(_)
        | Type::RegExp
        | Type::Date
        | Type::Symbol
        | Type::Promise => Some(REPR_HEAP),
        _ => None,
    }
}

impl LowerCtx<'_> {
    /// Append the `__torajs_promise_stamp_repr(p, repr)` call on the
    /// freshly-minted cell.
    pub(crate) fn emit_promise_stamp_repr(&mut self, promise: &Operand, repr: i64) {
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.promise_stamp_repr,
                vec![promise.clone(), Operand::ConstI64(repr)],
            ),
        );
    }
}
