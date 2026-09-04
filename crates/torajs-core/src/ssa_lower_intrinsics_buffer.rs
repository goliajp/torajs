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
    pub typedarray_create: FuncId,
    pub typedarray_is_kind: FuncId,
    pub uint8array_from_base64: FuncId,
    pub uint8array_from_hex: FuncId,
    pub dataview_create: FuncId,
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
        // §23.2.2.1-2 — the two `Uint8Array` statics. Both take the
        // string BORROWED and answer an owned `Uint8Array` Any, and
        // both can leave a pending SyntaxError, so the call site
        // checks. `fromHex` has no options bag at all.
        uint8array_from_base64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_uint8array_from_base64",
            &[Type::Any, Type::Any],
            Type::Any,
        ),
        uint8array_from_hex: declare_intrinsic(
            module,
            fn_table,
            "__torajs_uint8array_from_hex",
            &[Type::Any],
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
        // (kind discriminant, then three borrowed Any slots) → owned
        // TypedArray Any. The name resolved to the discriminant at
        // compile time, so the runtime never parses one.
        typedarray_create: declare_intrinsic(
            module,
            fn_table,
            "__torajs_typedarray_create",
            &[Type::I64, Type::Any, Type::Any, Type::Any],
            Type::Any,
        ),
        // The eleven share one heap tag, so `instanceof` asks about
        // the element kind rather than the tag.
        typedarray_is_kind: declare_intrinsic(
            module,
            fn_table,
            "__torajs_typedarray_is_kind",
            &[Type::Any, Type::I64],
            Type::Bool,
        ),
        // (buffer, byteOffset, byteLength — all borrowed Any) →
        // owned DataView Any. §25.3.2's coercions live in the
        // kernel so their spec order survives.
        dataview_create: declare_intrinsic(
            module,
            fn_table,
            "__torajs_dataview_create",
            &[Type::Any, Type::Any, Type::Any],
            Type::Any,
        ),
    }
}
