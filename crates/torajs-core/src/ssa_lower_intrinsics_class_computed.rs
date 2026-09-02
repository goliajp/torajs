//! Pass 0 `declare_intrinsic` group: member faces defined under a
//! RUNTIME property key (RFC 20260802-class-computed-member 刀 2,
//! extended by 565-03 to the object-literal twin).
//!
//! Two define kernels, both keyed by a RUNTIME property key — the
//! `lower_key(DefineKey::Expr)` product (a Str cell, or a Symbol
//! passed through per §7.1.19 step 2) — landing the reified member
//! face on `__proto_<C>` (instance) or `__class_<C>` (static):
//!
//! - `computed_method_define(tag, key, adapter, is_static, this_free)` — mints
//!   the reified-method cell and defines it with the §10.2.10 method
//!   attribute set.
//! - `computed_accessor_define(tag, key, get, set, is_static)` — a
//!   single-face AccessorPair define whose flags carry only the
//!   present face, so `get [k]` / `set [k]` with the same runtime
//!   key merge into one pair (§7.3.9 redefine semantics in the
//!   dynobj define kernel).
//! - `fn_computed_name_define(cell, key)` — 565-03, the object-literal
//!   twin: §10.2.9 names an anonymous function definition sitting in a
//!   computed field after its key, and an ordinary closure carries that
//!   name as an own property rather than on its layout.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ClassComputedIds {
    pub computed_method_define: FuncId,
    pub computed_accessor_define: FuncId,
    /// 565-03 — `fn_computed_name_define(cell, key)`: §10.2.9
    /// SetFunctionName for an object-literal member under a computed
    /// key. The class twin carries its name on the reified face
    /// (564-01); an ordinary closure gets an own `name` property.
    pub fn_computed_name_define: FuncId,
    /// 420-06 — `class_source_register(tag, src_str)`: hand the
    /// type-erased class declaration text to the runtime's per-tag
    /// source table (§20.2.3.5 class-ctor toString).
    pub class_source_register: FuncId,
}

pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> ClassComputedIds {
    ClassComputedIds {
        computed_method_define: declare_intrinsic(
            module,
            fn_table,
            "__torajs_class_computed_method_define",
            &[Type::I64, Type::Ptr, Type::I64, Type::I64, Type::I64],
            Type::Void,
        ),
        computed_accessor_define: declare_intrinsic(
            module,
            fn_table,
            "__torajs_class_computed_accessor_define",
            &[Type::I64, Type::Ptr, Type::I64, Type::I64, Type::I64],
            Type::Void,
        ),
        fn_computed_name_define: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fn_computed_name_define",
            &[Type::Ptr, Type::Ptr],
            Type::Void,
        ),
        class_source_register: declare_intrinsic(
            module,
            fn_table,
            "__torajs_class_source_register",
            &[Type::I64, Type::Ptr],
            Type::Void,
        ),
    }
}
