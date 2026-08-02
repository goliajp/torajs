//! Pass 0 `declare_intrinsic` group: runtime computed class member
//! names (RFC 20260802-class-computed-member 刀 2).
//!
//! Two define kernels, both keyed by a RUNTIME property key — the
//! `lower_key(DefineKey::Expr)` product (a Str cell, or a Symbol
//! passed through per §7.1.19 step 2) — landing the reified member
//! face on `__proto_<C>` (instance) or `__class_<C>` (static):
//!
//! - `computed_method_define(tag, key, adapter, is_static)` — mints
//!   the reified-method cell and defines it with the §10.2.10 method
//!   attribute set.
//! - `computed_accessor_define(tag, key, get, set, is_static)` — a
//!   single-face AccessorPair define whose flags carry only the
//!   present face, so `get [k]` / `set [k]` with the same runtime
//!   key merge into one pair (§7.3.9 redefine semantics in the
//!   dynobj define kernel).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ClassComputedIds {
    pub computed_method_define: FuncId,
    pub computed_accessor_define: FuncId,
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
            &[Type::I64, Type::Ptr, Type::I64, Type::I64],
            Type::Void,
        ),
        computed_accessor_define: declare_intrinsic(
            module,
            fn_table,
            "__torajs_class_computed_accessor_define",
            &[Type::I64, Type::Ptr, Type::I64, Type::I64, Type::I64],
            Type::Void,
        ),
    }
}
