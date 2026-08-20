//! Pass 0 `declare_intrinsic` group: `Iterator` global substrate
//! (RFC 20260730-iterator-global).
//!
//! 刀 1 face: the prototype-chain writer class_globals emits for
//! stripped `class C extends Iterator {}` heirs, and the §7.3.22
//! OrdinaryHasInstance walk `v instanceof Iterator` lowers to (the
//! Iterator "class" has no per-instance heap tag — membership is
//! purely prototype-chain). Helper-cell kernels join in 刀 2+.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct IteratorIds {
    pub proto_chain_builtin: FuncId,
    pub instanceof_builtin_proto: FuncId,
    pub iterator_ctor_throw: FuncId,
    pub iterator_from: FuncId,
    pub iterator_concat: FuncId,
    pub iterator_zip: FuncId,
    pub iterator_zip_keyed: FuncId,
    pub dstr_close_pending: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> IteratorIds {
    IteratorIds {
        proto_chain_builtin: declare_intrinsic(
            module,
            fn_table,
            "__torajs_proto_chain_builtin",
            &[Type::Any, Type::I64],
            Type::I64,
        ),
        instanceof_builtin_proto: declare_intrinsic(
            module,
            fn_table,
            "__torajs_instanceof_builtin_proto",
            &[Type::Any, Type::I64],
            Type::Bool,
        ),
        iterator_ctor_throw: declare_intrinsic(
            module,
            fn_table,
            "__torajs_iterator_ctor_throw",
            &[],
            Type::Any,
        ),
        // §27.1.6.2 Iterator.from — GetIteratorFlattenable +
        // pass-through-or-wrap (刀 4). (value) → owned iterator Any.
        iterator_from: declare_intrinsic(
            module,
            fn_table,
            "__torajs_iterator_from",
            &[Type::Any],
            Type::Any,
        ),
        // Iterator.concat(...items) — eager iterability check + lazy
        // kind-CONCAT helper cell (刀 5a). (items Arr<Any>, borrowed)
        // → owned iterator Any.
        iterator_concat: declare_intrinsic(
            module,
            fn_table,
            "__torajs_iterator_concat",
            &[Type::Ptr],
            Type::Any,
        ),
        // Iterator.zip(iterables, options) — eager opens + lazy
        // kind-ZIP cell (刀 5b). (iterables, options — both borrowed
        // Any) → owned iterator Any.
        iterator_zip: declare_intrinsic(
            module,
            fn_table,
            "__torajs_iterator_zip",
            &[Type::Any, Type::Any],
            Type::Any,
        ),
        // Iterator.zipKeyed(obj, options) — keyed sibling (刀 5c).
        iterator_zip_keyed: declare_intrinsic(
            module,
            fn_table,
            "__torajs_iterator_zip_keyed",
            &[Type::Any, Type::Any],
            Type::Any,
        ),
        // RFC 20260820-dstr-deferred-close — close the iterator a
        // suspendable destructuring pattern parked (undefined =
        // drained/never-opened → no-op). (it, borrowed Any) → void.
        dstr_close_pending: declare_intrinsic(
            module,
            fn_table,
            "__torajs_dstr_close_pending",
            &[Type::Any],
            Type::Void,
        ),
    }
}
