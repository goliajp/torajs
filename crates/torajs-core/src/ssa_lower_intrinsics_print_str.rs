//! Pass 0 `declare_intrinsic` group: print + StrRepr basics.
//!
//! chunk 110 narrow trial of the lower_inner Pass 0 multi-sub-sibling
//! split design. The 8 declarations grouped here are the universal
//! console.log / StrRepr alloc-print-drop-concat / rc_inc primitives
//! every program lowers; isolating them lets the giant Intrinsics
//! struct init in ssa_lower.rs use field-shorthand for these 8 fields
//! and shrinks the Pass 0 body by ~50 LOC. Scale up by porting each
//! domain (arr/obj/num/regex/...) into its own sibling once the design
//! is validated by ship + gate.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

/// FuncIds for the print + StrRepr basic runtime intrinsics declared
/// up-front in `lower_inner` Pass 0. The field names mirror the
/// `Intrinsics` struct fields so the caller can destructure with
/// shorthand into the final `Intrinsics { ... }` literal.
pub(crate) struct PrintStrIds {
    pub print_i64: FuncId,
    pub print_f64: FuncId,
    pub print_bool: FuncId,
    pub str_alloc: FuncId,
    pub str_print: FuncId,
    pub str_drop: FuncId,
    pub str_concat: FuncId,
    pub str_append: FuncId,
    pub str_concat_i64: FuncId,
    pub str_concat_f64: FuncId,
    pub rc_inc: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> PrintStrIds {
    PrintStrIds {
        print_i64: declare_intrinsic(module, fn_table, "print_i64", &[Type::I64], Type::Void),
        print_f64: declare_intrinsic(module, fn_table, "print_f64", &[Type::F64], Type::Void),
        print_bool: declare_intrinsic(module, fn_table, "print_bool", &[Type::Bool], Type::Void),
        str_alloc: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_alloc",
            &[Type::Ptr, Type::I64],
            Type::Str,
        ),
        str_print: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_print",
            &[Type::Str],
            Type::Void,
        ),
        str_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_drop",
            &[Type::Str],
            Type::Void,
        ),
        str_concat: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_concat",
            &[Type::Str, Type::Str],
            Type::Str,
        ),
        // Never emitted by the lowering: torajs-egraph's `str_append`
        // pass rewrites `concat` + `drop-left` pairs onto it, and can
        // only do so if the declaration is already in the module.
        str_append: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_append",
            &[Type::Str, Type::Str],
            Type::Str,
        ),
        // Never emitted by the lowering: torajs-egraph's
        // `concat_num_fuse` pass rewrites `to_str` + `concat` +
        // `drop` triples onto these (S1-A2 attack B1); the
        // declarations must already be in the module for it to.
        str_concat_i64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_concat_i64",
            &[Type::Str, Type::I64],
            Type::Str,
        ),
        str_concat_f64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_concat_f64",
            &[Type::Str, Type::F64],
            Type::Str,
        ),
        rc_inc: declare_intrinsic(
            module,
            fn_table,
            "__torajs_rc_inc",
            &[Type::Ptr],
            Type::Void,
        ),
    }
}
