//! Pass 0 `declare_intrinsic` group: the receiver-gate call kernel
//! (398-06 / 398-11 修法 ① substrate).
//!
//! One id today: `closure_call_with_this(env, this, argv, argc)` —
//! the explicit-`this` twin of `closure_call_variadic`, called from
//! the typed indirect lanes' runtime recv arm
//! (`ssa_lower_call_recv_gate`). Future receiver-channel kernels
//! (the 398-11 any-boundary family) grow here, not in the
//! `any_substrate` declare (registered no-growth).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct RecvGateIds {
    pub closure_call_with_this: FuncId,
    /// 403-03 — `anyv_to_callable_cell(box) -> ptr`: the fn-typed
    /// return boundary's Any→Closure coercion (Closure cell passes
    /// through, stake riding the pointer; anything else settles its
    /// stake and answers the undefined sentinel the call site's
    /// undefable guard turns into a catchable TypeError).
    pub anyv_to_callable_cell: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> RecvGateIds {
    RecvGateIds {
        closure_call_with_this: declare_intrinsic(
            module,
            fn_table,
            "__torajs_closure_call_with_this",
            &[Type::Ptr, Type::Any, Type::Ptr, Type::I64],
            Type::Any,
        ),
        anyv_to_callable_cell: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_to_callable_cell",
            &[Type::Any],
            Type::Ptr,
        ),
    }
}
