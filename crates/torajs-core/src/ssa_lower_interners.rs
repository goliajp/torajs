//! Type-layout interners extracted from `ssa_lower.rs` chunk 395 —
//! Path A.3-batch16.
//!
//! Free helpers (module-level, not on `LowerCtx`) that deduplicate the
//! `Vec<Type>`-keyed layout tables used by the SSA lowering pass:
//!
//! - `intern_arr_layout(arr_layouts, elem)` — return the existing
//!   `ssa::ArrId` for `Type::Arr(elem)` if a matching row already
//!   exists, otherwise append and return the new id. Linear scan on
//!   an interner rebuilt per-module; row counts stay small (< a few
//!   dozen distinct element types per program) so the O(N) probe is
//!   below noise vs the cost of the surrounding SSA emit.
//! - `intern_fn_sig(fn_sigs, params, ret)` — same shape for function
//!   signatures, returning an `ssa::SigId` keyed on `(params, ret)`.
//!   Used by both direct-call SSA emit and closure-value plumbing.
//!
//! Both fns are `pub(crate)` and take `&mut Vec<...>` refs so they can
//! be called from anywhere in the crate (including `LowerCtx`
//! borrow-invariant contexts that hold the interner tables as fields).
//! Original call sites in `ssa_lower.rs` continue to work unchanged
//! via `pub(crate) use crate::ssa_lower_interners::{...};` re-export.

use crate::ssa;
use crate::ssa::Type;

pub(crate) fn intern_arr_layout(arr_layouts: &mut Vec<Type>, elem: Type) -> ssa::ArrId {
    for (i, ex) in arr_layouts.iter().enumerate() {
        if *ex == elem {
            return ssa::ArrId(i as u32);
        }
    }
    let id = ssa::ArrId(arr_layouts.len() as u32);
    arr_layouts.push(elem);
    id
}

pub(crate) fn intern_fn_sig(
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    params: Vec<Type>,
    ret: Type,
) -> ssa::SigId {
    for (i, ex) in fn_sigs.iter().enumerate() {
        if ex.0 == params && ex.1 == ret {
            return ssa::SigId(i as u32);
        }
    }
    let id = ssa::SigId(fn_sigs.len() as u32);
    fn_sigs.push((params, ret));
    id
}
