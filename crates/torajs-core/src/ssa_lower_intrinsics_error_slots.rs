//! Pass 0 `declare_intrinsic` group: the §20.5 error-instance slots
//! whose own-ness is runtime state.
//!
//! `message` (§20.5.6.1.1) exists only where the constructor got one;
//! `name` (§20.5.3.2) lives on `<C>.prototype` and becomes the
//! instance's only where user code assigns it. Neither can be read as
//! a plain field load or answered from a compile-time field list, so
//! each carries a resolver (own slot, else the class prototype chain)
//! and a presence probe.
//!
//! Their own group per the rotation-255 rule — the `any_substrate`
//! declare fn is a registered known-debt function, so new intrinsics
//! land beside it rather than inside it.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ErrorSlotIds {
    pub error_message_present: FuncId,
    pub error_message_get: FuncId,
    pub error_name_present: FuncId,
    pub error_name_get: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ErrorSlotIds {
    let obj = &[Type::Ptr][..];
    let mut decl = |name: &str, ret: Type| declare_intrinsic(module, fn_table, name, obj, ret);
    ErrorSlotIds {
        error_message_present: decl("__torajs_error_message_present", Type::Bool),
        error_message_get: decl("__torajs_error_message_get", Type::Str),
        error_name_present: decl("__torajs_error_name_present", Type::Bool),
        error_name_get: decl("__torajs_error_name_get", Type::Str),
    }
}
