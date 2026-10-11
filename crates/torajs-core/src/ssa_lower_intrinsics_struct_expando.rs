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
//! A declared field is not an own property either once it is deleted,
//! or when the literal that built the cell left it out: the record and
//! the probe of it are here too.
//!
//! `any_prop_has` is already declared in the `any_substrate` group;
//! only its enumerable twin is new, and it lands here rather than
//! beside it because that declare fn is registered known debt.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

#[derive(Debug, Clone, Copy)]
pub(crate) struct StructExpandoIds {
    /// §7.3.x propertyIsEnumerable over an any-boxed receiver — the
    /// enumerable-flag twin of `__torajs_any_prop_has`.
    pub any_prop_enumerable: FuncId,
    /// The [[GetOwnProperty]] face of `__torajs_any_prop_has`
    /// (`hasOwnProperty` / `Object.hasOwn`): a module namespace
    /// reads the binding, so a dead-zone field raises.
    pub any_own_prop_has: FuncId,
    /// §20.1.2.9 getOwnPropertyDescriptors — the plural of the
    /// descriptor read, which had no lowering at all.
    pub get_property_descriptors: FuncId,
    /// Is the dict entry under a key a deletion record? 1 / 0 (I32).
    pub entry_is_hole: FuncId,
    /// Record a declared field deleted (a fresh cell's absent field).
    pub obj_field_delete: FuncId,
    /// HasProperty past a struct cell that lacks a declared member.
    pub struct_lacks_in: FuncId,
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
        any_own_prop_has: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_own_prop_has",
            &[Type::Any, Type::Ptr],
            Type::I64,
        ),
        get_property_descriptors: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_get_property_descriptors",
            &[Type::Any],
            Type::Any,
        ),
        entry_is_hole: declare_intrinsic(
            module,
            fn_table,
            "__torajs_dynobj_entry_is_hole",
            &[Type::Ptr, Type::Ptr],
            Type::I32,
        ),
        obj_field_delete: declare_intrinsic(
            module,
            fn_table,
            "__torajs_obj_declared_field_delete",
            &[Type::Ptr, Type::Ptr],
            Type::I64,
        ),
        struct_lacks_in: declare_intrinsic(
            module,
            fn_table,
            "__torajs_struct_lacks_in",
            &[Type::I64, Type::I64, Type::Ptr, Type::Ptr],
            Type::I64,
        ),
    }
}
