//! WeakRef / WeakMap / WeakSet instance-method arms extracted
//! from the [`crate::check_type_of_member::check`] top-level
//! `match (&obj_ty, name) { ... }` (chunk 192 — second sub-batch
//! of check_type_of_member.rs per-type-family decomposition;
//! mirrors chunks 185-191 try_match shape).
//!
//! Pure type-table lookup — every Weak-family method returns a
//! fixed `Type::Function(args, ret)` literal with no `Checker` /
//! `Ast` state. All three Weak types collapse here (vs a sibling
//! per type) because each has few enough arms (WeakRef = 1,
//! WeakMap = 4, WeakSet = 3) that splitting per type would just
//! add module overhead.
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match. Caller falls through to the next type-family
//! arm on `None`.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        // T-26 — WeakRef.deref(). Returns the target if
        // still alive (rc-bumped on success), or null.
        // Type-erased to Type::Any; users `as` cast to
        // the original concrete type.
        //
        // S325 — sig is 0-arg per ES §26.1.3.2; widen via
        // a dedicated arm below (the static-table path
        // only fires when args.len() == 0, so trailing
        // args[1..] are typecheck-and-dropped there).
        (Type::WeakRef, "deref") => {
            Type::Function(Vec::new(), Box::new(Type::Nullable(Box::new(Type::Any))))
        }
        // T-26.B — WeakMap methods. set takes (key,
        // value); both type-erased to Any. get returns
        // Nullable<Any>. has / delete return Boolean.
        (Type::WeakMap, "set") => Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Void)),
        (Type::WeakMap, "get") => Type::Function(
            vec![Type::Any],
            Box::new(Type::Nullable(Box::new(Type::Any))),
        ),
        // 383-04 — the stage-3 upsert pair (bun ships both).
        (Type::WeakMap, "getOrInsert") => {
            Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Any))
        }
        (Type::WeakMap, "getOrInsertComputed") => Type::Function(
            vec![
                Type::Any,
                Type::Function(vec![Type::Any], Box::new(Type::Any)),
            ],
            Box::new(Type::Any),
        ),
        (Type::WeakMap, "has") => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        (Type::WeakMap, "delete") => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        // T-26.B — WeakSet methods.
        (Type::WeakSet, "add") => Type::Function(vec![Type::Any], Box::new(Type::Void)),
        (Type::WeakSet, "has") => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        (Type::WeakSet, "delete") => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        _ => return None,
    };
    Some(Ok(ty))
}
