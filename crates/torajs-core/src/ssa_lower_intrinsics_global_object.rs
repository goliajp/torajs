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
    }
}
