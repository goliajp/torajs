//! Pass 0 `declare_intrinsic` group: the value-shaped-parent super
//! channel (RFC 20260815 knife 2b).
//!
//! Two ids: the module-init registration handing the runtime a
//! class's `__ctorany_<C>` boxed adapter (keyed on the class cell,
//! beside the factory-adapter registration), and the `super(…)`
//! dispatch kernel the capturing lane calls when the parent is a
//! runtime VALUE — a class cell routes to its registered twin, a
//! closure takes the ordinary receiver-honoring call channel, and
//! anything else raises §15.7.14's TypeError.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct CtoranyIds {
    pub ctorany_register: FuncId,
    pub super_call_value: FuncId,
    pub heritage_check: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> CtoranyIds {
    CtoranyIds {
        ctorany_register: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_ctorany_register",
            &[Type::Any, Type::Ptr],
            Type::Void,
        ),
        super_call_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_super_call",
            &[Type::Any, Type::Any, Type::Any],
            Type::Any,
        ),
        heritage_check: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_heritage_check",
            &[Type::Any],
            Type::Void,
        ),
    }
}
