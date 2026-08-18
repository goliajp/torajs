//! Pass 0 `declare_intrinsic` group: the §13.3.8.1 spread-call
//! kernels (rotation 372) — a call whose argument list carries a
//! dynamic `...spread` materializes the full list into one
//! `Array<Any>` and routes here; argc is a runtime fact the kernel
//! reads off the array, which the fixed-argc `any_call` /
//! `any_method_call` twins cannot express. Own group per the
//! any_substrate declare's no-growth ledger entry (rotation 245).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct SpreadCallIds {
    /// `(callee_any, args_arr)` — bare `f(a, ...xs)`; answers an
    /// owned Any, TypeError pending for a non-callable callee.
    pub any_call_spread: FuncId,
    /// `(recv_any, mid, name_str, name_len, recv_slot, args_arr)` —
    /// `recv.name(a, ...xs)`; same dispatch tail as
    /// `any_method_call` (inner + proto-patch consult + TypeError).
    pub any_method_call_spread: FuncId,
    /// `(callee_any, args_arr)` — `new callee(a, ...xs)`; enters the
    /// fixed-argc `anyv_construct` (IsConstructor gate + construct
    /// paths) with the runtime argc read off the array.
    pub anyv_construct_spread: FuncId,
}

pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> SpreadCallIds {
    SpreadCallIds {
        any_call_spread: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_call_spread",
            &[Type::Any, Type::Ptr][..],
            Type::Any,
        ),
        any_method_call_spread: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_method_call_spread",
            &[
                Type::Any,
                Type::I64,
                Type::Ptr,
                Type::I64,
                Type::Ptr,
                Type::Ptr,
            ][..],
            Type::Any,
        ),
        anyv_construct_spread: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_construct_spread",
            &[Type::Any, Type::Ptr][..],
            Type::Any,
        ),
    }
}
