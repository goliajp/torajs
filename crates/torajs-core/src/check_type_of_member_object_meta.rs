//! `Type::Object("Object")` accessor / mutating / property-
//! descriptor arms extracted from
//! [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 202 — twelfth sub-
//! batch of check_type_of_member.rs per-type-family
//! decomposition; mirrors chunks 191-201 try_match shape).
//!
//! Covers the **single-`Type::Object("Object")`** arms whose
//! lowering involves accessor / mutation / property-descriptor
//! semantics (subset-deferred or stubbed in ssa_lower; the
//! checktime sigs here keep test262 5k consumers flowing):
//! - `getPrototypeOf` (P4.2 Phase B+C — returns Any)
//! - `defineProperty` (P3.3 — ssa_lower extracts
//!   descriptor.value, routes to dynobj_set)
//! - `getOwnPropertyDescriptor` (constructs Any-boxed
//!   descriptor `{value, writable, enumerable, configurable}`)
//! - `setPrototypeOf` (permissive Any stub — 2026-05-18)
//! - `defineProperties` (permissive Any stub — 2026-05-18)
//! - `create` (fresh dynobj-backed Any-box at lower time)
//! - `assign` (no-op returning target if not intercepted)
//! - `preventExtensions` / `seal` (no-op substrate)
//! - `isExtensible` / `isSealed` (no-op returns true/false)
//!
//! Mixed-namespace Object/Reflect arms (`Object.keys` ∪
//! `Reflect.ownKeys`, `Object.hasOwn` ∪ `Reflect.has`,
//! `Reflect.get`) + generic `(Type::Object(_), …)` catch-alls
//! (`hasOwnProperty` / `propertyIsEnumerable` /
//! `isPrototypeOf` / `toString` / `prototype` / `name` /
//! `length`) stay in the main match — their patterns aren't
//! single-`Type::Object("Object")`.
//!
//! Returns `Some(Ok(_))` on hit, `None` when `name` doesn't
//! match any of the above.

use crate::check::Type;

pub(crate) fn try_match(name: &str) -> Option<Result<Type, String>> {
    let ty = match name {
        // P4.2 Phase B+C — Object.getPrototypeOf returns
        // the class's prototype object as an Any-box (the
        // same `__proto_<C>` registered via
        // __torajs_proto_register at module init). Pre-P4.2
        // the stub returned Null; with prototype singletons
        // now exposed, return Any so the caller can `===`
        // against `C.prototype` and chain-walk via further
        // getPrototypeOf calls. Returns ANY_NULL (still
        // Type::Any tag-wise) when the arg has no prototype
        // (Type::Obj with class_tag 0, or a Type::Any whose
        // dynobj lacks `__proto__`).
        "getPrototypeOf" => Type::Function(vec![Type::Any], Box::new(Type::Any)),
        // P3.3 — Object.defineProperty(obj, key, descriptor)
        // accepted at typecheck. ssa_lower intercepts the
        // Call, extracts descriptor.value (other descriptor
        // fields like writable/configurable/enumerable/get/
        // set are subset-deferred), and routes to dynobj_set.
        // obj is Type::Any (must be a dynobj-backed Any-box);
        // descriptor is Type::Any (typically a plain object
        // literal at the call site — ssa_lower probes for the
        // .value field at AST time).
        //
        // RFC 20260716 刀 18 — key sig relaxed from `Type::String`
        // to `Type::Any` (mirror of gOPD 刀 17). A StringWrapper /
        // Number / Boolean / etc. key routes through `lower_key`'s
        // `emit_to_string` coerce (§20.1.2.6 step 1 → §7.1.19 →
        // §7.1.17); the runtime helper still borrows a raw Str
        // pointer and SSA lower drops the coerced Str after.
        "defineProperty" => {
            Type::Function(vec![Type::Any, Type::Any, Type::Any], Box::new(Type::Void))
        }
        // P3.getOwnPropertyDescriptor — accept at typecheck.
        // ssa_lower intercepts and constructs an Any-boxed
        // descriptor object `{value, writable, enumerable,
        // configurable}` from the dynobj bucket's stored
        // tag/value/flags (per dcf069f attribute-flag
        // tracking). Missing key returns Any-boxed undefined.
        //
        // RFC 20260716 刀 17 — key sig relaxed from `Type::String`
        // to `Type::Any` so a StringWrapper / Number / Boolean etc.
        // arg (`new String("k")`, `42`) flows into SSA lower where
        // `emit_to_string` performs the ES §7.1.19 ToPropertyKey →
        // §7.1.17 ToString coercion. The runtime helper still takes
        // a raw Str pointer; SSA lower drops the owned Str after
        // the helper reads it (helper borrows).
        "getOwnPropertyDescriptor" => {
            Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Any))
        }
        // 2026-05-18 — accept these as permissive Any
        // typecheck-only stubs (no real substrate yet).
        // ssa_lower has no special intercept either: the
        // calls reach the generic call path and would
        // panic. With test262 5k unlock being the goal,
        // accept here so harness-shim consumers (which
        // never read the return) flow through; cases
        // that need real spec behavior bucket as bugs
        // rather than incompatible.
        "setPrototypeOf" => Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Any)),
        "defineProperties" => Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Void)),
        // `Object.create(proto, descriptors?)` — common
        // test262 init pattern (`Object.create(null)`).
        // Returns Any (a fresh dynobj-backed Any-box at
        // lower time).
        "create" => Type::Function(vec![Type::Any], Box::new(Type::Any)),
        // `Object.assign(target, ...sources)` — copy own
        // enumerable props. Subset accepts any-typed
        // target + variadic any sources; ssa_lower's
        // generic-call path picks it up as a no-op
        // (returns target) if not intercepted.
        "assign" => Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Any)),
        // `Object.preventExtensions(obj)` /
        // `Object.isExtensible(obj)` / `Object.seal(obj)`
        // / `Object.isSealed(obj)` — no-op substrate
        // returns the obj / true|false. Real semantics
        // (frozen-bit dispatch) requires runtime header
        // flag extension — deferred.
        "preventExtensions" | "seal" => Type::Function(vec![Type::Any], Box::new(Type::Any)),
        "isExtensible" | "isSealed" => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        _ => return None,
    };
    Some(Ok(ty))
}
