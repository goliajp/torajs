//! The per-type-family member tables a member read on a known type
//! is answered from, tried in order (chunks 191-206).

use crate::check::Type;

/// Per-type-family `try_match` dispatch chain (chunks 191-206).
/// Every sibling reads only `(obj_ty, name)` — no checker state.
/// The full sibling dispatch table:
///   191 Date / 192 Weak{Ref,Map,Set} / 193 Set ∪ Map ∪
///   MapIter ∪ ArrIter / 194 Number ∪ Boolean ∪ BigInt ∪
///   Symbol prim methods / 195 RegExp / 196 Array (incl.
///   chunk-206 Array-as-Object catch-all) / 197 String /
///   198 Promise / 199 Struct ∪ Function ∪ Any misc /
///   200 Object("console" | "Math" | "Number" | "BigInt"
///   | "JSON" | "Array" | "Promise" | "Symbol") +
///   single-tag Object static / 201 Object("Date" | "Bun"
///   | "BunFile" | "Response" | "process" | "env" |
///   "process_stdout" | "process_stderr" | "fs" |
///   "fs_promises" | "String") I/O namespaces / 202
///   Object("Object") accessor / mutating / property-
///   descriptor / 203 Object("Object") ∪ Object("Reflect")
///   mixed (keys / hasOwn / Reflect.get) / 204 generic
///   Object(_) catch-alls (hasOwnProperty / prototype /
///   name / length / etc.) + Symbol singletons / 205
///   prim ∪ Any unions (String|Array length, prim
///   constructor, prim|Any hasOwnProperty).
/// Whether any per-family arm claims `name` on `obj_ty` — i.e. whether
/// a read resolves to a modeled member rather than falling through to
/// [`crate::check_type_of_member_terminal_miss::answer`].
/// `Some(Err(_))` counts as claimed.
pub(crate) fn member_is_modeled(obj_ty: &Type, name: &str) -> bool {
    try_family_dispatch(obj_ty, name).is_some()
}

/// Whether a per-family arm answers `name` on `obj_ty` with a type.
pub(crate) fn member_resolves(obj_ty: &Type, name: &str) -> bool {
    matches!(try_family_dispatch(obj_ty, name), Some(Ok(_)))
}

pub(crate) fn try_family_dispatch(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    // Date instance methods — see
    // [`crate::check_type_of_member_date`] (chunk 191 —
    // first sub-batch of check_type_of_member per-type-
    // family decomposition).
    if matches!(obj_ty, Type::Date)
        && let Some(r) = crate::check_type_of_member_date::try_match(name)
    {
        return Some(r);
    }
    // Weak family (WeakRef / WeakMap / WeakSet) instance
    // methods — see [`crate::check_type_of_member_weak`]
    // (chunk 192 — second sub-batch).
    if matches!(obj_ty, Type::WeakRef | Type::WeakMap | Type::WeakSet)
        && let Some(r) = crate::check_type_of_member_weak::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // Set / Map / MapIter / ArrIter instance methods — see
    // [`crate::check_type_of_member_setmap`] (chunk 193 —
    // third sub-batch).
    if matches!(
        &obj_ty,
        Type::Map(..) | Type::Set | Type::MapIter(..) | Type::ArrIter
    ) && let Some(r) = crate::check_type_of_member_setmap::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // Primitive wrappers (Number / Boolean / BigInt / Symbol)
    // single-type arms — see
    // [`crate::check_type_of_member_prim`] (chunk 194 —
    // fourth sub-batch). Mixed arms (`(Number | String |
    // Boolean | BigInt | Symbol, "constructor")` etc.) stay
    // in the main match so the String / Any branches are
    // still served when this dispatch returns None.
    if matches!(
        &obj_ty,
        Type::Number | Type::Boolean | Type::BigInt | Type::Symbol
    ) && let Some(r) = crate::check_type_of_member_prim::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // RegExp instance methods + properties — see
    // [`crate::check_type_of_member_regex`] (chunk 195 —
    // fifth sub-batch).
    if matches!(obj_ty, Type::RegExp)
        && let Some(r) = crate::check_type_of_member_regex::try_match(name)
    {
        return Some(r);
    }
    // `Type::Array` instance methods + the Array-as-Object
    // catch-all (`arr.x` for unknown `x` → Type::Any) — see
    // [`crate::check_type_of_member_array`] (chunk 196 —
    // sixth sub-batch; chunk 206 folded the catch-all in as
    // the last sibling arm). The shared `(Type::String,
    // "length") | (Type::Array(_), "length")` arm stays in
    // the main match (handled by chunk 205 prim-union).
    if matches!(obj_ty, Type::Array(_))
        && let Some(r) = crate::check_type_of_member_array::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // `Type::String` instance methods — see
    // [`crate::check_type_of_member_string`] (chunk 197 —
    // seventh sub-batch). Mixed-type arms (the shared
    // `length` arm, prim-union `constructor`, and
    // prim+Any union `hasOwnProperty` /
    // `propertyIsEnumerable`) stay in the main match.
    if matches!(obj_ty, Type::String)
        && let Some(r) = crate::check_type_of_member_string::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // `Type::Promise` instance methods — see
    // [`crate::check_type_of_member_promise`] (chunk 198 —
    // eighth sub-batch). The pre-match structural getter
    // forwarding (`if let Type::Promise(inner) = obj_ty`)
    // stays in the main module; only the 3 method arms
    // (`then` / `catch` / `finally`) move.
    if matches!(obj_ty, Type::Promise(_))
        && let Some(r) = crate::check_type_of_member_promise::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // `Type::Struct` / `Type::Function` / `Type::Any`
    // single-type members + per-type catch-alls — see
    // [`crate::check_type_of_member_misc`] (chunk 199 —
    // ninth sub-batch). The mixed-type prim+Any
    // `hasOwnProperty` / `propertyIsEnumerable` arm and
    // the `Array(_), name` catch-all stay in the main
    // match (their patterns aren't single-type).
    if matches!(obj_ty, Type::Struct(_) | Type::Function(..) | Type::Any)
        && let Some(r) = crate::check_type_of_member_misc::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // `Type::Object("NAMESPACE")` static-namespace members
    // (console / Math / Number / BigInt / JSON / Array /
    // Object single-type / Promise / Symbol) — see
    // [`crate::check_type_of_member_namespace`] (chunk 200 —
    // tenth sub-batch). Mixed-namespace arms (Object ∪
    // Reflect for keys / hasOwn / ownKeys, generic
    // Object(_) catch-alls, Object mutating /
    // accessor / property-descriptor methods) stay in the
    // main match because their patterns span more than one
    // namespace tag or carry richer dispatch.
    if let Type::Object(_) = obj_ty
        && let Some(r) = crate::check_type_of_member_namespace::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // I/O-flavored namespaces — see
    // [`crate::check_type_of_member_namespace_io`] (chunk
    // 201 — eleventh sub-batch). Sibling to chunk 200;
    // covers Date / Bun / BunFile / Response / process /
    // env / process_stdout-stderr / fs / fs_promises /
    // String static namespace tags.
    if let Type::Object(_) = obj_ty
        && let Some(r) = crate::check_type_of_member_namespace_io::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // `Type::Object("Object")` accessor / mutating /
    // property-descriptor arms (getPrototypeOf /
    // defineProperty / getOwnPropertyDescriptor /
    // setPrototypeOf / defineProperties / create / assign /
    // preventExtensions / seal / isExtensible / isSealed)
    // — see [`crate::check_type_of_member_object_meta`]
    // (chunk 202 — twelfth sub-batch). Mixed Object/Reflect
    // arms + generic `(Type::Object(_), …)` catch-alls stay
    // in the main match.
    if matches!(obj_ty, Type::Object("Object"))
        && let Some(r) = crate::check_type_of_member_object_meta::try_match(name)
    {
        return Some(r);
    }
    // Mixed `Type::Object("Object")` ∪ `Type::Object("Reflect")`
    // arms (`keys` ∪ `getOwnPropertyNames` ∪ `ownKeys`,
    // `hasOwn` ∪ `has`, `Reflect.get`) — see
    // [`crate::check_type_of_member_reflect`] (chunk 203 —
    // thirteenth sub-batch). Generic `(Type::Object(_), …)`
    // catch-alls (hasOwnProperty / propertyIsEnumerable /
    // isPrototypeOf / toString / prototype / name / length)
    // stay in the main match — they fire for every namespace
    // tag, not just Object/Reflect.
    if matches!(obj_ty, Type::Object("Object") | Type::Object("Reflect"))
        && let Some(r) = crate::check_type_of_member_reflect::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // Generic `(Type::Object(_), …)` Object.prototype catch-alls
    // (V3-18 m2.b — hasOwnProperty / propertyIsEnumerable /
    // isPrototypeOf / toString — subset stubs that fire for
    // every namespace tag) + constructor introspection (V3-18
    // m2.c — prototype / name / length) + Symbol singleton
    // accessors (T-13.c — `Symbol.iterator` / `asyncIterator` /
    // `toPrimitive`, which gate on `Type::Object("Symbol")`).
    // See [`crate::check_type_of_member_object_generic`]
    // (chunk 204 — fourteenth sub-batch). All other
    // `Type::Object(...)` arms have already been handled by
    // the more-specific pre-match dispatches above (chunks
    // 200 namespace / 201 namespace_io / 202 object_meta /
    // 203 reflect); the generic catch-alls fire last for any
    // namespace tag not picked up earlier.
    if let Type::Object(_) = obj_ty
        && let Some(r) = crate::check_type_of_member_object_generic::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // Mixed-primitive ∪ Any union arms (the cross-type-family
    // arms whose `|`-union spans more than one primitive
    // — `(String, "length") | (Array, "length")`, prim-union
    // `constructor`, prim+Any union `hasOwnProperty` /
    // `propertyIsEnumerable`). See
    // [`crate::check_type_of_member_prim_union`] (chunk 205 —
    // fifteenth sub-batch). Cross-family `|`-union patterns
    // can't live in any single primitive's dedicated sibling.
    crate::check_type_of_member_prim_union::try_match(obj_ty, name)
}
