//! Pass 0 `declare_intrinsic` group: M4 exception state runtime.
//!
//! chunk 134 — the tail of Pass 0 `declare_intrinsic` calls.
//! 4 declarations around two module-level i64 globals
//! (`throw_active`, `throw_value`) the backend implements. Lowering
//! threads set/check/take through the call path; user code never
//! touches these symbols directly.
//!
//! - `throw_set(tag, value) -> Void` — P4.7-widened signature.
//!   Lowered throw sites compute the tag from the throw expr's
//!   static type via box_to_tag_value-style classification (HEAP for
//!   Str/Arr/Obj/Closure/Dynobj-Any, I64 for numbers, F64 for floats,
//!   BOOL for booleans, etc.). The runtime stores `(active=1,
//!   tag, value)` for the next `throw_check` / `throw_take_tag` +
//!   `throw_take` pair.
//! - `throw_check() -> i64` — non-zero when a throw is pending.
//!   ssa_lower emits this after every may-throw call site.
//! - `throw_take() -> i64` — returns the value + clears active.
//!   Typed catches call this alone (tag silently ignored).
//! - `throw_take_tag() -> i64` — returns the tag (call before
//!   `throw_take` for `: any` catch slots that need to rebox via
//!   `any_box(tag, value)`).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ThrowIds {
    pub throw_set: FuncId,
    pub throw_check: FuncId,
    pub throw_take: FuncId,
    pub throw_take_tag: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ThrowIds {
    ThrowIds {
        throw_set: declare_intrinsic(
            module,
            fn_table,
            "__torajs_throw_set",
            &[Type::I64, Type::I64],
            Type::Void,
        ),
        throw_check: declare_intrinsic(module, fn_table, "__torajs_throw_check", &[], Type::I64),
        throw_take: declare_intrinsic(module, fn_table, "__torajs_throw_take", &[], Type::I64),
        throw_take_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_throw_take_tag",
            &[],
            Type::I64,
        ),
    }
}
