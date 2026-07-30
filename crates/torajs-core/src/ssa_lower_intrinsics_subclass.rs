//! Pass 0 `declare_intrinsic` group: exotic-subclass substrate (RFC
//! 20260730-exotic-backed-class-instance blades 1-2).
//!
//! Per-builtin mint kernels (`class_tag` → boxed instance carrying
//! `FLAG_SUBCLASSED` + a blade-0 side-table entry; Array's also takes
//! an initial length) and the ctor-side one-argument `super(v)`
//! semantics kernels (`new Array(len)` length validation per
//! §23.1.2.1 / the wrapper ctors' `[[*Data]] = To*(v)` coercions).
//! Every operand and answer is any-world — subclass instances live in
//! the any world (dict-mode). Grows one mint + one super pair per tag
//! as blade 2 walks the remaining builtins.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ExoticSubclassIds {
    pub arr_subclass_alloc: FuncId,
    pub arr_subclass_super_len: FuncId,
    pub number_wrapper_subclass_alloc: FuncId,
    pub string_wrapper_subclass_alloc: FuncId,
    pub boolean_wrapper_subclass_alloc: FuncId,
    pub number_wrapper_subclass_super: FuncId,
    pub string_wrapper_subclass_super: FuncId,
    pub boolean_wrapper_subclass_super: FuncId,
}

pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> ExoticSubclassIds {
    let tag_only = &[Type::I64][..];
    let super_pair = &[Type::Any, Type::Any][..];
    let d =
        |module: &mut Module,
         fn_table: &mut HashMap<String, FuncId>,
         name: &str,
         params: &[Type]| declare_intrinsic(module, fn_table, name, params, Type::Any);
    ExoticSubclassIds {
        arr_subclass_alloc: d(
            module,
            fn_table,
            "__torajs_arr_subclass_alloc",
            &[Type::I64, Type::I64],
        ),
        arr_subclass_super_len: d(
            module,
            fn_table,
            "__torajs_arr_subclass_super_len",
            super_pair,
        ),
        number_wrapper_subclass_alloc: d(
            module,
            fn_table,
            "__torajs_number_wrapper_subclass_alloc",
            tag_only,
        ),
        string_wrapper_subclass_alloc: d(
            module,
            fn_table,
            "__torajs_string_wrapper_subclass_alloc",
            tag_only,
        ),
        boolean_wrapper_subclass_alloc: d(
            module,
            fn_table,
            "__torajs_boolean_wrapper_subclass_alloc",
            tag_only,
        ),
        number_wrapper_subclass_super: d(
            module,
            fn_table,
            "__torajs_number_wrapper_subclass_super",
            super_pair,
        ),
        string_wrapper_subclass_super: d(
            module,
            fn_table,
            "__torajs_string_wrapper_subclass_super",
            super_pair,
        ),
        boolean_wrapper_subclass_super: d(
            module,
            fn_table,
            "__torajs_boolean_wrapper_subclass_super",
            super_pair,
        ),
    }
}
