//! Pass 0 `declare_intrinsic` group: constructor return-override
//! substrate (RFC 20260820-ctor-return-override).
//!
//! Its own group rather than a line in `any_substrate::declare`,
//! which the file-size ledger holds at only-ever-shrinking.
//!
//! Both answering kernels hand back an OWNED box; the carry borrows
//! all three operands. See `torajs-anyvalue::ctor_return` for the
//! desugared shape these serve.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct CtorRetIds {
    /// §10.2.2 step 13 for a `return <expr>` in a ctor body.
    pub ctor_ret_value: FuncId,
    /// The `super(…)` answer taking over as `this`.
    pub ctor_ret_adopt: FuncId,
    /// One own element moved onto an adopted object.
    pub ctor_ret_carry: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> CtorRetIds {
    let any_pair = &[Type::Any, Type::Any][..];
    CtorRetIds {
        ctor_ret_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_ctor_ret_value",
            any_pair,
            Type::Any,
        ),
        ctor_ret_adopt: declare_intrinsic(
            module,
            fn_table,
            "__torajs_ctor_ret_adopt",
            any_pair,
            Type::Any,
        ),
        ctor_ret_carry: declare_intrinsic(
            module,
            fn_table,
            "__torajs_ctor_ret_carry",
            &[Type::Any, Type::Any, Type::Ptr],
            Type::Void,
        ),
    }
}
