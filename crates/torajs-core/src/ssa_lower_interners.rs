//! Interner helpers extracted from `ssa_lower.rs` chunk 395 + 403 —
//! Path A.3-batch16 + batch24.
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
//!
//! `LowerCtx`-bound `intern_string_literal(s) -> ValueId` (chunk 403)
//! extends the same theme with a `Str`-shaped `StaticStrRef` emit:
//! encodes `s` via `StringLiteral::encode_from_str` (P11.1-S2-a),
//! pushes it into `self.new_strings`, and returns a `Type::Str` SSA
//! operand referencing the new `ssa::StringId`.

use crate::ssa;
use crate::ssa::{InstKind, Type, ValueId};
use crate::ssa_lower::LowerCtx;

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

impl<'a> LowerCtx<'a> {
    /// Intern a string literal and return a Type::Str SSA value pointing at
    /// a fresh heap-allocated `{u64 len; u8 data[]}` copy. The static bytes
    /// live as a `[N x i8]` global (no NUL, len is explicit); `__torajs_str_alloc`
    /// copies them into a heap StrRepr at runtime. Every literal use does
    /// one alloc — caller is responsible for emitting Drop at scope end
    /// (P2.2.b.2 wires that up; this sub-step intentionally leaks one
    /// alloc per literal use, which is fine for one-shot bench programs).
    pub(crate) fn intern_string_literal<S: AsRef<torajs_wtf8::Wtf8> + ?Sized>(
        &mut self,
        s: &S,
    ) -> ValueId {
        // Phase P-rpn — every string-literal expression resolves to a
        // Str-shaped `StaticStrRef` global (rc_inc / rc_dec / free
        // all no-op via the STATIC_LITERAL flag). Encoding decision
        // happens in `StringLiteral::encode_from_str` (P11.1-S2-a).
        let lit = ssa::StringLiteral::encode_from_wtf8(s.as_ref());
        let sid = ssa::StringId((self.string_id_base + self.new_strings.len()) as u32);
        self.new_strings.push(lit);
        self.f
            .append_inst(self.cur_block, InstKind::StaticStrRef(sid), Type::Str, None)
    }
}
