//! Pass 0 `declare_intrinsic` group: the SuperProperty CALL channel.
//!
//! Its own group rather than a line in the `ctorany` one beside it —
//! that group is the value-shaped-parent `super(…)` CONSTRUCTOR
//! channel, a different question (which ctor runs on this object)
//! from this one (which method the base names, called against the
//! current `this`).
//!
//! Three ids, all `any`-operand: the read (base, key, receiver), the
//! write (base, key, value, receiver) and the call (the read's three
//! plus one dense args pack). See
//! `torajs-anyvalue::super_prop_call` for what the kernels owe the
//! spec.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct SuperPropIds {
    /// §13.3.7 read off a Super Reference.
    pub super_prop_get: FuncId,
    /// §9.1.9 write through a Super Reference.
    pub super_prop_set: FuncId,
    /// §13.3.6 call off a Super Reference.
    pub super_prop_call: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> SuperPropIds {
    SuperPropIds {
        super_prop_get: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_super_prop_get",
            &[Type::Any, Type::Any, Type::Any],
            Type::Any,
        ),
        super_prop_set: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_super_prop_set",
            &[Type::Any, Type::Any, Type::Any, Type::Any],
            Type::Any,
        ),
        super_prop_call: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_super_prop_call",
            &[Type::Any, Type::Any, Type::Any, Type::Any],
            Type::Any,
        ),
    }
}
