//! Pass 0 `declare_intrinsic` group: the own-property probes a typed
//! struct receiver needs when the compile-time field list is not the
//! whole answer.
//!
//! A class instance can carry expando entries in its `+24` dict (RFC
//! 20260714-struct-dynamic-props) — `err.cause` is one the injected
//! ctor itself installs. Those are own properties, but they exist only
//! at runtime, so a `hasOwnProperty` / `propertyIsEnumerable` fold
//! over `struct_layouts` answers `false` for every one of them. The
//! fold stays for layout HITS and defers a miss to these.
//!
//! `any_prop_has` is already declared in the `any_substrate` group;
//! only its enumerable twin is new, and it lands here rather than
//! beside it because that declare fn is registered known debt.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct StructExpandoIds {
    /// §7.3.x propertyIsEnumerable over an any-boxed receiver — the
    /// enumerable-flag twin of `__torajs_any_prop_has`.
    pub any_prop_enumerable: FuncId,
}

pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> StructExpandoIds {
    StructExpandoIds {
        any_prop_enumerable: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_prop_enumerable",
            &[Type::Any, Type::Ptr],
            Type::I64,
        ),
    }
}
