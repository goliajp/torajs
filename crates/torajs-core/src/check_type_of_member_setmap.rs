//! Map / Set / MapIter / ArrIter instance-method arms extracted
//! from the [`crate::check_type_of_member::check`] top-level
//! `match (&obj_ty, name) { ... }` (chunk 193 — third sub-batch
//! of check_type_of_member.rs per-type-family decomposition;
//! mirrors chunks 191-192 try_match shape).
//!
//! Pure type-table lookup — every Map/Set/iterator method
//! returns a fixed `Type::Function(args, ret)` literal (or
//! `Type::Number` for the `.size` getter) with no `Checker` /
//! `Ast` state. All four collection types collapse here because
//! they share the same runtime substrate (Set ≡ Map<T,undef>;
//! MapIter / ArrIter share an `IteratorResult` shape).
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match. Caller falls through to the next type-family
//! arm on `None`.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        /* P6.1 / P6.2 — Map.prototype.size / Set.prototype.size.
         * Both expose a non-method `size` (Number) read via the
         * member arm; ssa_lower calls `__torajs_map_size`
         * (Set storage is the same Map runtime). */
        (Type::Map, "size") | (Type::Set, "size") => Type::Number,
        /* P6.1 — Map<K,V> methods. set takes (key, value)
         * both type-erased to Any (the runtime stores
         * tagged-Any slots regardless); set returns the
         * map itchecker per spec §23.1.3.9 enabling chained
         * `m.set(k,v).set(k2,v2)` idiom (S127-5). ssa-lower
         * mirrors with an emit_rc_inc on the receiver
         * before returning so the chained value owns its
         * own ref independent of the source binding.
         * get returns Nullable<Any>. has / delete return
         * Boolean. clear returns Void. */
        (Type::Map, "set") => Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Map)),
        (Type::Map, "get") => Type::Function(
            vec![Type::Any],
            Box::new(Type::Nullable(Box::new(Type::Any))),
        ),
        // Stage-3 upsert (RFC 20260721 刀 6) — answers the present
        // value or the freshly inserted default.
        (Type::Map, "getOrInsert") => {
            Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Any))
        }
        (Type::Map, "has") => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        (Type::Map, "delete") => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        (Type::Map, "clear") => Type::Function(Vec::new(), Box::new(Type::Void)),
        /* P6.4a — Map.forEach. Spec callback shape is
         * `(value, key, map) => void`. Both `value` and
         * `key` are type-erased to Any since storage is
         * the (tag, payload) Any-domain. */
        (Type::Map, "forEach") => Type::Function(
            vec![Type::Function(
                vec![Type::Any, Type::Any, Type::Map],
                Box::new(Type::Void),
            )],
            Box::new(Type::Void),
        ),
        /* P6.4b — Map.keys / .values return a stateful
         * MapIter (spec §23.1.3.8 / §23.1.3.13). The
         * iter's `next()` produces `IteratorResult<any>`
         * = `{ value: any, done: boolean }`. .entries is
         * deferred to P6.4c (needs Array<Any> alloc per
         * step + boxed (k, v) write). */
        (Type::Map, "keys") | (Type::Map, "values") => {
            Type::Function(Vec::new(), Box::new(Type::MapIter))
        }
        /* P6.4c — Map.entries yields `[k, v]` pairs;
         * Set.entries yields `[v, v]` pairs (spec
         * §23.1.3.4 / §24.2.3.6). Both return the same
         * MapIter handle (the runtime kind decides the
         * yield shape). */
        (Type::Map, "entries") => Type::Function(Vec::new(), Box::new(Type::MapIter)),
        /* P6.4b — Set.keys = .values per spec §24.2.3.5
         * (returns iterator over the elements). */
        (Type::Set, "keys") | (Type::Set, "values") => {
            Type::Function(Vec::new(), Box::new(Type::MapIter))
        }
        (Type::Set, "entries") => Type::Function(Vec::new(), Box::new(Type::MapIter)),
        /* P6.4b — MapIter.next() returns the spec-
         * shaped IteratorResult struct. */
        (Type::MapIter, "next") => Type::Function(
            Vec::new(),
            Box::new(Type::Struct(vec![
                ("value".into(), Type::Any),
                ("done".into(), Type::Boolean),
            ])),
        ),
        /* P6.4c-C3 — ArrIter.next() shape matches
         * MapIter (both produce `IteratorResult<any>`). */
        (Type::ArrIter, "next") => Type::Function(
            Vec::new(),
            Box::new(Type::Struct(vec![
                ("value".into(), Type::Any),
                ("done".into(), Type::Boolean),
            ])),
        ),
        /* P6.2 — Set<T> methods. add takes a single Any-
         * typed value; storage piggy-backs on Map<T,
         * undef> at runtime. S127-5 — add returns the set
         * itchecker per spec §24.2.3.1 to enable chained
         * `s.add(v1).add(v2)` idiom. ssa-lower mirrors
         * with emit_rc_inc + receiver return. */
        (Type::Set, "add") => Type::Function(vec![Type::Any], Box::new(Type::Set)),
        (Type::Set, "has") => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        // ES2025 read-only Set setops (§24.2.3.{12,13,14}).
        // §24.2.1.2 GetSetRecord admits any set-like argument —
        // the runtime protocol decides (a non-set-like refuses
        // with the spec TypeError/RangeError), so the checker's
        // parameter is Any; a real-Set argument keeps the Set×Set
        // fast kernels in the lowering.
        (Type::Set, "isSubsetOf") | (Type::Set, "isSupersetOf") | (Type::Set, "isDisjointFrom") => {
            Type::Function(vec![Type::Any], Box::new(Type::Boolean))
        }
        // ES2025 combining Set setops (§24.2.3.{15,16,17,18}) —
        // same set-like admission. Returns a fresh Set with rc=1.
        (Type::Set, "union")
        | (Type::Set, "intersection")
        | (Type::Set, "difference")
        | (Type::Set, "symmetricDifference") => {
            Type::Function(vec![Type::Any], Box::new(Type::Set))
        }
        (Type::Set, "delete") => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        (Type::Set, "clear") => Type::Function(Vec::new(), Box::new(Type::Void)),
        /* P6.4a — Set.forEach. Spec callback shape is
         * `(value, value2, set) => void`, where the
         * first two args are the same element. */
        (Type::Set, "forEach") => Type::Function(
            vec![Type::Function(
                vec![Type::Any, Type::Any, Type::Set],
                Box::new(Type::Void),
            )],
            Box::new(Type::Void),
        ),
        _ => return None,
    };
    Some(Ok(ty))
}
