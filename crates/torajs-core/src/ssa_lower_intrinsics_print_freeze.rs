//! Pass 0 `declare_intrinsic` group: console.log path
//! (`__torajs_print_anyv` + multi-arg joiner + per-T arr_print_inline
//! family + Map/Set/Fn `_outer` wrappers + `any_to_str_pair`) +
//! Object.freeze.
//!
//! chunk 123 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-122). 18 declarations covering the contiguous source
//! block from `print_any` through `obj_check_not_frozen`, just
//! before the bigint family.
//!
//! Subgroups (source order):
//! - **print_anyv core**: `__torajs_print_anyv` (single-arg with
//!   trailing \n); `__torajs_print_anyv_inline_top` (multi-arg
//!   joiner — no trailing \n, lowerer emits `' '` between args +
//!   `'\n'` after last); `__torajs_io_putc_stdout` (`int putc(int)`
//!   — the separator + final \n emitter).
//! - **typed Arr<T> inline walkers** (torajs-arr::print_inline) —
//!   bun-form `[ a, b, c ]` no-\n print of typed Arr args in the
//!   multi-arg joiner: `arr_print_{i64,f64,bool,str,substr}_inline`.
//!   Standalone `__torajs_arr_print_<T>` (with \n) stays the
//!   single-arg console.log target.
//! - **Map/Set/Fn outer wrappers** — Type::Map / Type::Set / Type::FnSig
//!   console.log dispatch can't go through the tag-aware
//!   `__torajs_print_anyv` (Map/Set share Tag::Map=15 so the generic
//!   would mislabel sets as `Map(...)`). Fn-name registry Phase 1
//!   ships `[Function]\n` (Phase 2 will rewrite to bun-byte-equal
//!   `[Function: name]` via rodata `__torajs_fn_name_table[]`).
//! - **any_to_str_pair** (P-CONSOLE follow-up) — routes an Any-boxed
//!   value (split into tag+value via `anyv_unbox_tag` +
//!   `anyv_unbox_value`) into the runtime's tag-dispatched ToString.
//!   Used by `coerce_to_str` for mixed-arg console.log. Returns a
//!   fresh owned Str (rc=1; caller drops). For ANY_HEAP / Type::Str
//!   inputs the helper rc-inc's the existing pointer + returns it.
//! - **Object.freeze** (T-09.d, v0.4.0): `obj_freeze` sets FROZEN
//!   bit; `obj_is_frozen` reads it. `obj_is_frozen_any` is the
//!   NaN-box-aware variant (HEAP → deref + obj_is_frozen, non-HEAP →
//!   true per ES2015 §19.1.2.16; avoids SIGSEGV from
//!   sentinel-as-heap deref, see cases#obj-is-frozen-any-segv).
//!   `obj_check_not_frozen` is the silent-ignore mutation guard
//!   inline-checked by field-write codegen.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct PrintFreezeIds {
    pub print_any: FuncId,
    pub print_any_inline_top: FuncId,
    pub io_putc_stdout: FuncId,
    pub arr_print_i64_inline: FuncId,
    pub arr_print_f64_inline: FuncId,
    pub arr_print_bool_inline: FuncId,
    pub arr_print_str_inline: FuncId,
    pub arr_print_substr_inline: FuncId,
    pub map_print_outer: FuncId,
    pub set_print_outer: FuncId,
    pub fn_print_outer: FuncId,
    pub fnsig_to_str: FuncId,
    pub fn_name_str: FuncId,
    /// RFC 20260719-fn-tostring-source B5 — erased-source ToString
    /// kernel keyed on a raw fn_addr (`String(f)` / template lane).
    pub fn_source_str: FuncId,
    pub closure_name_str: FuncId,
    /// B5 — Closure-cell twin of `fn_source_str` (String(c) lane).
    pub closure_source_str: FuncId,
    /// RFC 20260721 刀 9 — plain-fn `.prototype` materialization
    /// (cell → owned AnyValue; undefined without `FLAG_FN_PROTO`).
    pub closure_prototype_any: FuncId,
    /// RFC 20260721 刀 4 — flavor-keyed `.constructor` reflection
    /// (async cell → %AsyncFunction%, else %Function%).
    pub closure_ctor_value: FuncId,
    pub any_to_str: FuncId,
    /// §23.1.3.30.2 steps 5-8 pre-probe for Any elements (刀 7 G8a).
    pub any_sort_undef_pre: FuncId,
    /// §13.15.3 concat-position twin — ToPrimitive(default) first.
    pub any_to_str_prim: FuncId,
    pub any_to_str_box: FuncId,
    pub any_to_display_str: FuncId,
    pub obj_freeze: FuncId,
    pub obj_freeze_any: FuncId,
    pub obj_is_frozen: FuncId,
    pub obj_is_frozen_any: FuncId,
    pub obj_check_not_frozen: FuncId,
    /// RFC 20260806-declared-field-redefine — successor guard at the
    /// typed `instance.field = v` site: asks about the field's
    /// writability as well as the cell's frozen bit, for one wider
    /// immediate in the test the frozen-only guard already ran.
    pub obj_check_field_writable: FuncId,
}

// CARVE-OUT: dispatch table — a flat declare_intrinsic list (one
// entry per runtime symbol, no logic branches); grows with the
// intrinsic surface, same posture as ssa_lower_intrinsics_table.
pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> PrintFreezeIds {
    PrintFreezeIds {
        print_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_print_anyv",
            &[Type::Any],
            Type::Void,
        ),
        print_any_inline_top: declare_intrinsic(
            module,
            fn_table,
            "__torajs_print_anyv_inline_top",
            &[Type::Any],
            Type::Void,
        ),
        io_putc_stdout: declare_intrinsic(
            module,
            fn_table,
            "__torajs_io_putc_stdout",
            &[Type::I64],
            Type::I64,
        ),
        arr_print_i64_inline: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_print_i64_inline",
            &[Type::Ptr],
            Type::Void,
        ),
        arr_print_f64_inline: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_print_f64_inline",
            &[Type::Ptr],
            Type::Void,
        ),
        arr_print_bool_inline: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_print_bool_inline",
            &[Type::Ptr],
            Type::Void,
        ),
        arr_print_str_inline: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_print_str_inline",
            &[Type::Ptr],
            Type::Void,
        ),
        arr_print_substr_inline: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_print_substr_inline",
            &[Type::Ptr],
            Type::Void,
        ),
        map_print_outer: declare_intrinsic(
            module,
            fn_table,
            "__torajs_map_print_outer",
            &[Type::Map],
            Type::Void,
        ),
        set_print_outer: declare_intrinsic(
            module,
            fn_table,
            "__torajs_set_print_outer",
            &[Type::Set],
            Type::Void,
        ),
        fn_print_outer: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fn_print_outer",
            &[Type::I64],
            Type::Void,
        ),
        fnsig_to_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fnsig_to_str",
            &[Type::I64],
            Type::Str,
        ),
        fn_name_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fn_name_str",
            &[Type::I64],
            Type::Str,
        ),
        fn_source_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fn_source_str",
            &[Type::I64],
            Type::Str,
        ),
        closure_name_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_closure_name_str",
            &[Type::Ptr],
            Type::Str,
        ),
        closure_source_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_closure_source_str",
            &[Type::Ptr],
            Type::Str,
        ),
        closure_prototype_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_closure_prototype_any",
            &[Type::Ptr],
            Type::Any,
        ),
        closure_ctor_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_closure_ctor_value",
            &[Type::Ptr],
            Type::Any,
        ),
        any_to_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_to_str_pair",
            &[Type::I64, Type::I64],
            Type::Str,
        ),
        any_sort_undef_pre: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_sort_undef_pre",
            &[Type::Any, Type::Any],
            Type::I64,
        ),
        any_to_str_prim: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_to_str_pair_prim",
            &[Type::I64, Type::I64],
            Type::Str,
        ),
        any_to_str_box: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_to_str",
            &[Type::Any],
            Type::Str,
        ),
        any_to_display_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_to_display_str",
            &[Type::Any],
            Type::Str,
        ),
        obj_freeze: declare_intrinsic(
            module,
            fn_table,
            "__torajs_obj_freeze",
            &[Type::Ptr],
            Type::Ptr,
        ),
        obj_freeze_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_obj_freeze_any",
            &[Type::Any],
            Type::Any,
        ),
        obj_is_frozen: declare_intrinsic(
            module,
            fn_table,
            "__torajs_obj_is_frozen",
            &[Type::Ptr],
            Type::Bool,
        ),
        obj_is_frozen_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_obj_is_frozen_any",
            &[Type::Any],
            Type::Bool,
        ),
        obj_check_not_frozen: declare_intrinsic(
            module,
            fn_table,
            "__torajs_obj_check_not_frozen",
            &[Type::Ptr],
            Type::Void,
        ),
        obj_check_field_writable: declare_intrinsic(
            module,
            fn_table,
            "__torajs_obj_check_field_writable",
            &[Type::Ptr, Type::Str],
            Type::Void,
        ),
    }
}
