//! Pass 0 `declare_intrinsic` group: the §13.2.8.4 template-object
//! kernel trio (T-12 tagged templates).
//!
//! The site call streams as begin / per-pair / end so no variadic
//! FFI or pointer-array marshalling is needed: `begin(site, n)`
//! opens (a cache hit turns the following calls into no-ops),
//! `str(cooked, raw)` hands one static Str pair over, and `end()`
//! answers the frozen template object — a BORROW of the per-site
//! cached cell (see `ssa_lower_call_template_object`).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct TemplateIds {
    pub template_object_begin: FuncId,
    pub template_object_str: FuncId,
    pub template_object_end: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> TemplateIds {
    TemplateIds {
        template_object_begin: declare_intrinsic(
            module,
            fn_table,
            "__torajs_template_object_begin",
            &[Type::I64, Type::I64],
            Type::Void,
        ),
        template_object_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_template_object_str",
            &[Type::Str, Type::Str],
            Type::Void,
        ),
        template_object_end: declare_intrinsic(
            module,
            fn_table,
            "__torajs_template_object_end",
            &[],
            Type::Any,
        ),
    }
}
