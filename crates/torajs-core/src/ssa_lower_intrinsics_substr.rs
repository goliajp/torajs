//! Pass 0 `declare_intrinsic` group: Object.is (Number arm) +
//! SplitIter + Substr view core (Phase Substr.A).
//!
//! chunk 129 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-128). 18 declarations covering the contiguous block
//! between the promise sibling and the inline
//! `ssa_lower_substr_trim_into::declare_all` call.
//!
//! Subgroups (source order):
//! - **Object.is f64 arm** (v0.2 #3): `(F64, F64) -> Bool`. Diverges
//!   from `===` on NaN-NaN (true) + ±0 (false); ±0 needs bit-level
//!   compare that FCmp alone can't express.
//! - **SplitIter ABI** (P-iter): `init(slot, parent_str, sep_str)`,
//!   `next(slot, out_substr_ptr) -> Bool`, `drop(slot)`. State + the
//!   yielded substr both live in caller-stack alloca slots; `init`
//!   manages one parent rc, `drop` releases it. `iter_slot` is an
//!   opaque 48-byte buffer (Type::Ptr to the alloca'd address);
//!   `out_substr` is a 32-byte caller-allocated Substr slot.
//! - **Substr view core**:
//!   - `substr_create(parent_str, offset, len) -> Substr`.
//!   - `substr_drop(s)`.
//!   - `substr_char_code_at(s, i) -> i64` (UTF-16 code unit, zext'd
//!     to i64).
//!   - `substr_code_point_at(s, i) -> i64` (surrogate-pair aware).
//!   - `substr_eq_str(s, str) -> Bool`.
//!   - View-aware probes (read bytes from parent + offset, no
//!     materialize) — needle is Str (literal-side common case):
//!     `starts_with` / `ends_with` / `includes` / `index_of`.
//!   - View-of-view (fresh standalone Substr referencing the same
//!     root parent — 32-byte malloc, no byte copy):
//!     `substr_slice(s, start, end)`, `substr_substring(s, a, b)`.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

/// Mach-O symbol of the immortal Substr-shaped `undefined` sentinel
/// cell (`#[unsafe(no_mangle)] pub static __TORAJS_SUBSTR_UNDEF_CELL`
/// in torajs-str `undef_sentinel.rs`) — the Substr mirror of
/// `STR_UNDEF_CELL_SYM`, see that const's doc for the GlobalRef
/// resolution path. Identity-compare emit sites (strict-eq / typeof
/// on a Substr slot) materialize the address with zero calls.
pub(crate) const SUBSTR_UNDEF_CELL_SYM: &str = "___TORAJS_SUBSTR_UNDEF_CELL";

pub(crate) struct SubstrIds {
    pub object_is_f64: FuncId,
    pub split_iter_init: FuncId,
    pub split_iter_next: FuncId,
    pub split_iter_drop: FuncId,
    pub substr_create: FuncId,
    pub substr_drop: FuncId,
    /// RFC 20260821 A2 — whole-array release for Str/Substr elements.
    /// One call replaces the SSA-emitted element walk plus its
    /// trailing `arr_drop`; the walk's bookkeeping was the larger
    /// half of that loop's cost.
    pub arr_drop_str_elems: FuncId,
    /// Rotation 468 — a substring view may only live in the split
    /// block that owns it: slots memcpy'd out of an `Arr<Substr>`
    /// adopt owned copies (replaces the rc-inc walk), and a split
    /// product about to receive owned writes materializes in place.
    pub arr_substr_adopt_copied: FuncId,
    pub arr_substr_materialize_owned: FuncId,
    pub substr_char_code_at: FuncId,
    pub substr_code_point_at: FuncId,
    pub substr_eq_str: FuncId,
    pub substr_starts_with: FuncId,
    pub substr_ends_with: FuncId,
    pub substr_includes: FuncId,
    pub substr_index_of: FuncId,
    pub substr_slice: FuncId,
    pub substr_substring: FuncId,
    pub str_index_view: FuncId,
    pub substr_index_view: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> SubstrIds {
    let ptr_str = &[Type::Ptr, Type::Str][..];
    SubstrIds {
        object_is_f64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_object_is_f64",
            &[Type::F64, Type::F64],
            Type::Bool,
        ),
        split_iter_init: declare_intrinsic(
            module,
            fn_table,
            "__torajs_split_iter_init",
            &[Type::Ptr, Type::Str, Type::Str],
            Type::Void,
        ),
        split_iter_next: declare_intrinsic(
            module,
            fn_table,
            "__torajs_split_iter_next",
            &[Type::Ptr, Type::Ptr],
            Type::Bool,
        ),
        split_iter_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_split_iter_drop",
            &[Type::Ptr],
            Type::Void,
        ),
        substr_create: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_create",
            &[Type::Str, Type::I64, Type::I64],
            Type::Substr,
        ),
        substr_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_drop",
            &[Type::Substr],
            Type::Void,
        ),
        arr_drop_str_elems: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_drop_str_elems",
            &[Type::Ptr],
            Type::Void,
        ),
        arr_substr_adopt_copied: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_substr_adopt_copied",
            &[Type::Ptr, Type::I64, Type::I64],
            Type::Void,
        ),
        arr_substr_materialize_owned: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_substr_materialize_owned",
            &[Type::Ptr],
            Type::Void,
        ),
        substr_char_code_at: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_char_code_at",
            &[Type::Substr, Type::I64],
            Type::F64,
        ),
        substr_code_point_at: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_code_point_at",
            &[Type::Substr, Type::I64],
            Type::I64,
        ),
        substr_eq_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_eq_str",
            &[Type::Substr, Type::Str],
            Type::Bool,
        ),
        substr_starts_with: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_starts_with",
            ptr_str,
            Type::Bool,
        ),
        substr_ends_with: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_ends_with",
            ptr_str,
            Type::Bool,
        ),
        substr_includes: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_includes",
            ptr_str,
            Type::Bool,
        ),
        substr_index_of: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_index_of",
            ptr_str,
            Type::I64,
        ),
        substr_slice: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_slice",
            &[Type::Ptr, Type::I64, Type::I64],
            Type::Substr,
        ),
        substr_substring: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_substring",
            &[Type::Ptr, Type::I64, Type::I64],
            Type::Substr,
        ),
        // RFC 20260707 residual — string INDEX reads (`s[i]`) answer
        // the immortal Substr-shaped undefined sentinel on OOB
        // (unlike charAt / slice which answer ""/empty view per
        // spec); identity-compare emit sites materialize the
        // sentinel via `GlobalRef(SUBSTR_UNDEF_CELL_SYM)`.
        str_index_view: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_index_view",
            &[Type::Str, Type::I64],
            Type::Substr,
        ),
        substr_index_view: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_index_view",
            &[Type::Substr, Type::I64],
            Type::Substr,
        ),
    }
}
