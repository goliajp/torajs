//! Pass 0 `declare_intrinsic` group: WeakRef / WeakMap / WeakSet
//! substrate (T-26 / T-26.B, v0.7).
//!
//! chunk 125 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-124). 15 declarations covering pointer-identity-keyed
//! Weak collections with auto-eviction on key death via the shared
//! weakref registry.
//!
//! - **WeakRef** (T-26): `create(target_ptr) -> WeakRef` (target is
//!   any heap type, type-erased to Ptr at the SSA layer); `deref ->
//!   Ptr` (returns target +1 rc'd on success, NULL when reclaimed);
//!   `drop` is rc-aware + unregisters from the runtime's global
//!   registry on last owner; `target_dying(p)` is the cb the runtime
//!   fires when a tracked target's last rc hits 0.
//! - **WeakMap** (T-26.B): create / set(m, k, v) / get(m, k) /
//!   has(m, k) -> i64 / delete(m, k) -> i64 / drop.
//! - **WeakSet** (T-26.B): create / add / has -> i64 / delete -> i64
//!   / drop.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct WeakIds {
    pub weakref_create: FuncId,
    pub weakref_deref_any: FuncId,
    pub weakref_drop: FuncId,
    pub weakref_target_dying: FuncId,
    /// §24.1.1.1 step 7 and its three siblings — one runtime walk
    /// fills any of the four collections from a general iterable
    /// (`(target, iterable, kind)`; kind per
    /// `torajs_rc::collection_kind`). Declared with the weak group
    /// because that is where the family's other shared kernels live.
    pub collection_init_from_iterable: FuncId,
    pub weakmap_create: FuncId,
    pub weakmap_set: FuncId,
    pub weakmap_get: FuncId,
    pub weakmap_has: FuncId,
    pub weakmap_delete: FuncId,
    pub weakmap_drop: FuncId,
    /// RC-4 F2 — key classification + extraction (ES CanBeHeldWeakly):
    /// `from_any` reads illegal keys as absent (NULL), `or_throw`
    /// records a pending TypeError for set/add.
    pub weak_key_from_any: FuncId,
    pub weak_key_from_any_or_throw: FuncId,
    pub weakset_create: FuncId,
    pub weakset_add: FuncId,
    pub weakset_has: FuncId,
    pub weakset_delete: FuncId,
    pub weakset_drop: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> WeakIds {
    WeakIds {
        weakref_create: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakref_create",
            &[Type::Ptr],
            Type::WeakRef,
        ),
        // Chunk 629 — boxed deref: checker types `wr.deref()` as
        // Nullable<Any>, so the SSA value is an AnyValue box (alive
        // box = ptr, cleared = ANY_UNDEF). The raw-Ptr
        // `__torajs_weakref_deref` stays in the runtime as this
        // helper's core but is no longer declared as an intrinsic.
        weakref_deref_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakref_deref_any",
            &[Type::WeakRef],
            Type::Any,
        ),
        weakref_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakref_drop",
            &[Type::WeakRef],
            Type::Void,
        ),
        weakref_target_dying: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakref_target_dying",
            &[Type::Ptr],
            Type::Void,
        ),
        collection_init_from_iterable: declare_intrinsic(
            module,
            fn_table,
            "__torajs_collection_init_from_iterable",
            &[Type::Any, Type::Any, Type::I64],
            Type::Void,
        ),
        weakmap_create: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakmap_create",
            &[],
            Type::WeakMap,
        ),
        weakmap_set: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakmap_set",
            &[Type::WeakMap, Type::Ptr, Type::Any],
            Type::Void,
        ),
        weakmap_get: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakmap_get",
            &[Type::WeakMap, Type::Ptr],
            Type::Any,
        ),
        weak_key_from_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weak_key_from_any",
            &[Type::Any],
            Type::Ptr,
        ),
        weak_key_from_any_or_throw: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weak_key_from_any_or_throw",
            &[Type::Any],
            Type::Ptr,
        ),
        weakmap_has: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakmap_has",
            &[Type::WeakMap, Type::Ptr],
            Type::I64,
        ),
        weakmap_delete: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakmap_delete",
            &[Type::WeakMap, Type::Ptr],
            Type::I64,
        ),
        weakmap_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakmap_drop",
            &[Type::WeakMap],
            Type::Void,
        ),
        weakset_create: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakset_create",
            &[],
            Type::WeakSet,
        ),
        weakset_add: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakset_add",
            &[Type::WeakSet, Type::Ptr],
            Type::Void,
        ),
        weakset_has: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakset_has",
            &[Type::WeakSet, Type::Ptr],
            Type::I64,
        ),
        weakset_delete: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakset_delete",
            &[Type::WeakSet, Type::Ptr],
            Type::I64,
        ),
        weakset_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_weakset_drop",
            &[Type::WeakSet],
            Type::Void,
        ),
    }
}
