//! Pass 0 `declare_intrinsic` group: private-name (`#x`) brand
//! substrate.
//!
//! A `#x` member READ lowers its tag channel to
//! `__torajs_any_member_get_priv_tag` (statically selected by the
//! `__priv_` prefix in `emit_member_fallback`) — same walk as the
//! base tag channel, but an own-face miss throws TypeError per
//! §7.3.31 PrivateGet / PrivateBrandCheck instead of answering
//! `undefined`. Its value twin also answers elements held in the
//! foreign-object store (slice 2c).
//! Grows with future brand-check kernels (`#x in o`, private write
//! miss).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct PrivateIds {
    pub any_member_get_priv_tag: FuncId,
    /// The value twin — slice 2c's foreign store answers here too.
    pub any_member_get_priv_value: FuncId,
    pub any_member_priv_has: FuncId,
    pub any_member_set_priv: FuncId,
    /// RFC 20260916 slice 2a — a class's private-method brand install.
    pub priv_install: FuncId,
    /// The install cursor of a class instance cell (-1 otherwise).
    pub priv_cursor: FuncId,
    /// The brand-check TypeError.
    pub priv_brand_fail: FuncId,
    /// RFC 20260917-class-eval-unit K1 — wires one evaluation of a
    /// unit class (the identity private brands will compare).
    pub class_unit_wire: FuncId,
    /// K2 — a class instance's hidden evaluation slot (0 otherwise).
    pub priv_unit_word: FuncId,
    /// K4-1 — one evaluation's static method face onto its class
    /// object (`unit, name Str, adapter vaddr, flags`).
    pub class_unit_static_method_define: FuncId,
    /// K4-3 — its accessor twin (`unit, name Str, get vaddr, set vaddr`).
    pub class_unit_static_accessor_define: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> PrivateIds {
    PrivateIds {
        any_member_get_priv_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_member_get_priv_tag",
            &[Type::Any, Type::Ptr],
            Type::I64,
        ),
        any_member_get_priv_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_member_get_priv_value",
            &[Type::Any, Type::Ptr],
            Type::I64,
        ),
        // §13.10.1 ergonomic brand check (`#x in o`).
        any_member_priv_has: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_member_priv_has",
            &[Type::Any, Type::Ptr],
            Type::Bool,
        ),
        // §7.3.32 PrivateSet — the write twin (statically selected
        // by the `__priv_` prefix in `emit_any_member_set`): an
        // undeclared brand throws instead of installing an expando.
        any_member_set_priv: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_member_set_priv",
            &[Type::Ptr, Type::Ptr, Type::I64, Type::I64, Type::I64],
            Type::Void,
        ),
        priv_install: declare_intrinsic(
            module,
            fn_table,
            "__torajs_priv_install",
            &[Type::Any, Type::I64, Type::I64],
            Type::Void,
        ),
        priv_cursor: declare_intrinsic(
            module,
            fn_table,
            "__torajs_priv_cursor",
            &[Type::Any],
            Type::I64,
        ),
        priv_brand_fail: declare_intrinsic(
            module,
            fn_table,
            "__torajs_priv_brand_fail",
            &[],
            Type::Void,
        ),
        class_unit_wire: declare_intrinsic(
            module,
            fn_table,
            crate::ast::class_unit::UNIT_WIRE,
            &[Type::Any, Type::Any, Type::I64, Type::I64],
            Type::Void,
        ),
        class_unit_static_method_define: declare_intrinsic(
            module,
            fn_table,
            "__torajs_class_unit_static_method_define",
            &[Type::Any, Type::Str, Type::I64, Type::I64],
            Type::Void,
        ),
        class_unit_static_accessor_define: declare_intrinsic(
            module,
            fn_table,
            "__torajs_class_unit_static_accessor_define",
            &[Type::Any, Type::Str, Type::I64, Type::I64],
            Type::Void,
        ),
        priv_unit_word: declare_intrinsic(
            module,
            fn_table,
            "__torajs_priv_unit_word",
            &[Type::Any, Type::I64],
            Type::I64,
        ),
    }
}
