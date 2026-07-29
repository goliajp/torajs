//! Mixed `Type::Object("Object")` ∪ `Type::Object("Reflect")`
//! arms extracted from
//! [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 203 — thirteenth
//! sub-batch of check_type_of_member.rs per-type-family
//! decomposition; mirrors chunks 191-202 try_match shape).
//!
//! Covers the **mixed-namespace** arms where one tag carries
//! the spec's `Reflect.*` shape and the other its `Object.*`
//! alias (or the standalone `Reflect.get` arm):
//! - `Object.keys` ∪ `Object.getOwnPropertyNames` ∪
//!   `Reflect.ownKeys` — all share Function([Any], Array<String>);
//!   ssa_lower routes through the same struct-keys emit.
//! - `Object.hasOwn` ∪ `Reflect.has` — Function([Any,String],
//!   Boolean); compile-time resolved when key is a Str literal.
//! - `Reflect.get(target, key)` — Function([Any,String], Any);
//!   subset folds typed struct + literal key to a field load.
//!
//! Single-tag arms stay in dedicated siblings:
//! - Other `Type::Object("Object")` accessor / mutating /
//!   property-descriptor methods → chunk 202
//!   (check_type_of_member_object_meta).
//! - Other `Type::Object("Object")` static methods (is /
//!   entries / fromEntries / values / freeze / isFrozen) →
//!   chunk 200 (check_type_of_member_namespace).
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match any of the above.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        // `Object.keys(obj)` — returns Array<String> with the
        // field names of obj's struct type. Static-resolved at
        // codegen (the struct layout is known at compile
        // time), so this is a compile-time constant array
        // emitted at the call site. Param is Type::Any
        // because the typechecker doesn't yet track
        // "any-struct" as a constraint; ssa_lower verifies
        // the arg actually carries Type::Obj at lower-time
        // and panics on non-struct args.
        (Type::Object("Object"), "keys")
        // chunk B2 (RFC 20260711 for-in) — parser-synthesized twin of
        // `keys` for the for-in desugar: a null / undefined Any
        // receiver enumerates nothing instead of throwing (§14.7.5
        // ForIn/OfHeadEvaluation step 3).
        | (Type::Object("Object"), "__forinKeys")
        // tr has no prototype chain, so own == all; alias
        // getOwnPropertyNames to keys at lower time.
        | (Type::Object("Object"), "getOwnPropertyNames")
        // ES6 §28.1.11 — `Reflect.ownKeys` shares this
        // signature; the ssa_lower dispatch routes both
        // through the same struct-keys emit.
        | (Type::Object("Reflect"), "ownKeys") => {
            Type::Function(vec![Type::Any], Box::new(Type::Array(Box::new(Type::String))))
        }
        // W-N-c — `Object.getOwnPropertySymbols(o)`: tr has no
        // symbol-keyed property surface (symbol index assignment
        // rejects loud at typecheck), so the result is statically
        // the empty array; undefined / null still throw at runtime
        // per §20.1.2.11 (ToObject).
        (Type::Object("Object"), "getOwnPropertySymbols") => {
            Type::Function(vec![Type::Any], Box::new(Type::Array(Box::new(Type::Symbol))))
        }
        /* v0.2 #3 — Object.hasOwn(obj, key) — compile-time
         * resolved when key is a Str literal (struct layout
         * known at lower time). Boolean result.
         *
         * Object.freeze / isFrozen are deferred — pairing
         * them as a no-op returning false would break
         * `Object.isFrozen(Object.freeze(o)) === true`
         * test262 cases. Real implementation needs a
         * frozen bit on the universal heap header (v0.3). */
        (Type::Object("Object"), "hasOwn")
        // ES6 §28.1.9 — `Reflect.has` shares this signature.
        | (Type::Object("Reflect"), "has") => {
            // §20.1.2.13 step 2 is ToPropertyKey, so the key domain is
            // the whole value domain, not `string`. Pinning it to
            // String refused the program for `Object.hasOwn(o, sym)`
            // — even for a statically-typed Symbol, which the lowering
            // has always handled.
            Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Boolean))
        }
        // ES6 §28.1.6 — `Reflect.get(target, key)`. Subset:
        // typed struct target + literal-string key folds at
        // ssa-lower time to a struct field load + box-to-Any
        // (key not in layout → ANY_UNDEF). Dynamic key or
        // non-struct target stays a deferred substrate.
        (Type::Object("Reflect"), "get") => {
            Type::Function(vec![Type::Any, Type::String], Box::new(Type::Any))
        }
        _ => return None,
    };
    Some(Ok(ty))
}
