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
    }
}
