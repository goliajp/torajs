//! Pass 0 `declare_intrinsic` group: constructor return-override
//! substrate (RFC 20260820-ctor-return-override).
//!
//! Its own group rather than a line in `any_substrate::declare`,
//! which the file-size ledger holds at only-ever-shrinking.
//!
//! The pick hands back an OWNED box; the carry borrows all three
//! operands. See `torajs-anyvalue::ctor_return` for the
//! desugared shape these serve.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct CtorRetIds {
    /// §10.2.2 step 13 — serves both the super site and the factory.
    /// Third operand is the constructor kind (nonzero = derived), which
    /// names the parent at a super site and the class at a factory.
    pub ctor_ret_value: FuncId,
    /// One own element moved onto an adopted object.
    pub ctor_ret_carry: FuncId,
    /// RFC 20260917-plain-fn-new-target knife 3 — a function's construct
    /// face registered against its value face.
    pub new_target_register: FuncId,
    /// Knife 2 — the same, from a minted cell's boxed entry and the
    /// face's adapter address.
    pub new_target_register_entries: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> CtorRetIds {
    CtorRetIds {
        ctor_ret_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_ctor_ret_value",
            &[Type::Any, Type::Any, Type::I64],
            Type::Any,
        ),
        ctor_ret_carry: declare_intrinsic(
            module,
            fn_table,
            "__torajs_ctor_ret_carry",
            &[Type::Any, Type::Any, Type::Ptr],
            Type::Void,
        ),
        new_target_register: declare_intrinsic(
            module,
            fn_table,
            "__torajs_new_target_register",
            &[Type::Any, Type::Any],
            Type::Void,
        ),
        new_target_register_entries: declare_intrinsic(
            module,
            fn_table,
            "__torajs_new_target_register_entries",
            &[Type::Ptr, Type::Ptr],
            Type::Void,
        ),
    }
}
