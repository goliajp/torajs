//! Pass 0 `declare_intrinsic` group: ArrayBuffer / TypedArray /
//! DataView substrate (RFC 20260823-typedarray-substrate).
//!
//! 刀 1 declares only what the *lowering* reaches directly. The four
//! `ArrayBuffer.prototype` accessors and its two methods are read
//! and called through the any-lane member probe and the any-lane
//! method dispatch, which are already declared — a new builtin
//! class does not need one intrinsic per property.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct BufferIds {
    pub arraybuffer_create: FuncId,
    pub arraybuffer_is_view: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> BufferIds {
    BufferIds {
        // (length, options — both borrowed Any) → owned ArrayBuffer
        // Any. Both coercions live in the kernel so that their spec
        // order survives; a rejected length records the RangeError
        // and answers undefined for the caller's throw check.
        arraybuffer_create: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arraybuffer_create",
            &[Type::Any, Type::Any],
            Type::Any,
        ),
        // §25.1.5.1 — a question about the argument's tag, so it
        // never throws and needs no check.
        arraybuffer_is_view: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arraybuffer_is_view",
            &[Type::Any],
            Type::I64,
        ),
    }
}
