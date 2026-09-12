//! Pass 0 `declare_intrinsic` group: the global environment record's
//! OBJECT half (RFC 20260911-global-object-record).
//!
//! §9.1.1 gives global code a record with two halves, and only one of
//! them is decidable when tr compiles. The declarative half is the
//! bindings the program writes down; the object half is the global
//! object, and whether it carries a given name is a fact about the
//! run — `this.p = 1` in global code puts it there, and a bare `p`
//! anywhere after that reads it back.
//!
//! So the three operators that can reach an unresolvable name each get
//! a kernel, and they differ only in what happens on a miss: a read
//! throws (§9.1.1.4 GetBindingValue, `S` = false), `typeof` answers
//! `"undefined"` (§13.5.1.1), and a write creates the property
//! (§9.1.1.4.6 SetMutableBinding).
//!
//! The lookups do not MINT the singleton; the write does. Minting
//! drags in the whole ns-static world, which the linker strips from a
//! program with no `globalThis` in it — and a program that never wrote
//! through the object has nothing there for a read to find anyway.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct GlobalObjectIds {
    pub global_ident_get: FuncId,
    pub global_ident_typeof: FuncId,
    pub global_ident_set: FuncId,
    pub global_var_bind: FuncId,
    pub global_var_sync: FuncId,
    pub script_preflight: FuncId,
    pub global_var_declare: FuncId,
    pub global_fn_declare: FuncId,
}

pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> GlobalObjectIds {
    GlobalObjectIds {
        // (name Str, borrowed) → owned Any, or the recorded
        // ReferenceError and undefined.
        global_ident_get: declare_intrinsic(
            module,
            fn_table,
            "__torajs_global_ident_get",
            &[Type::Str],
            Type::Any,
        ),
        // (name Str, borrowed) → owned Str. Never throws.
        global_ident_typeof: declare_intrinsic(
            module,
            fn_table,
            "__torajs_global_ident_typeof",
            &[Type::Str],
            Type::Str,
        ),
        // (name Str borrowed, value Any borrowed) → the value, so the
        // assignment is still an expression (§13.15.2 step 5).
        global_ident_set: declare_intrinsic(
            module,
            fn_table,
            "__torajs_global_ident_set",
            &[Type::Str, Type::Any],
            Type::Any,
        ),
        // (name Str borrowed, slot Ptr, kind I64) → nothing; may
        // record a TypeError (RFC 20260912-global-var-cell).
        global_var_bind: declare_intrinsic(
            module,
            fn_table,
            "__torajs_global_var_bind",
            &[Type::Str, Type::Ptr, Type::I64],
            Type::Void,
        ),
        // (name Str borrowed, slot Ptr) → nothing; may record a getter's
        // throw. A read of a name a LATER eval / Script declares
        // (rotation 640, `ssa_lower_ident_site_bound`).
        global_var_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_global_var_sync",
            &[Type::Str, Type::Ptr],
            Type::Void,
        ),
        // A Script evaluated inside a function (RFC 20260912 knife 5,
        // `globalthis_object_decls`): (kind I64, name Str borrowed) →
        // nothing; may record the §16.1.7 SyntaxError / TypeError.
        script_preflight: declare_intrinsic(
            module,
            fn_table,
            "__torajs_script_preflight",
            &[Type::I64, Type::Str],
            Type::Void,
        ),
        // (name Str borrowed) → nothing. §9.1.1.4.17 with D = false.
        global_var_declare: declare_intrinsic(
            module,
            fn_table,
            "__torajs_global_var_declare",
            &[Type::Str],
            Type::Void,
        ),
        // (name Str borrowed, value Any borrowed) → nothing.
        // §9.1.1.4.18 with D = false.
        global_fn_declare: declare_intrinsic(
            module,
            fn_table,
            "__torajs_global_fn_declare",
            &[Type::Str, Type::Any],
            Type::Void,
        ),
    }
}
