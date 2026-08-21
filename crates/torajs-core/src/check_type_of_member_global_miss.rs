//! Terminal-miss posture for the ECMAScript global objects — one of
//! the receiver shapes [`crate::check_type_of_member`]'s
//! `answer_terminal_miss` consults once every per-family `try_match`
//! arm has declined.
//!
//! `Math` / `JSON` / `Array` / `Reflect` / ... are **ordinary
//! extensible objects**. A name absent from the surface tr models is
//! therefore not evidence of a typo, the way a miss on a
//! user-authored object literal is: §10.1.8.1 [[Get]] answers
//! `undefined`, and any code that ran earlier may have put the name
//! there. test262 leans on both halves — `Array.myproperty = 1`
//! plants an expando, `Reflect.enumerate` and `Promise.then` read
//! names the spec deliberately does *not* define and expect
//! `undefined` back.
//!
//! The split against `check_type_of_ident`'s `NS_OBJECT_IDENTS` is
//! deliberate: that table also carries tr's own handles for the host
//! surface (`Bun` / `process` / `fs` / `fs_promises`). Those stay a
//! loud compile reject, because a miss there is a hole in *our*
//! modeling rather than a genuine absence, and the sweep's
//! `incompatible` bucket is where we find those holes. Going lenient
//! on them would convert a legible gap into a runtime
//! `undefined is not a function`.

/// The ECMAScript-defined subset of `check_type_of_ident`'s
/// `NS_OBJECT_IDENTS` — the 21 entries that name a global the spec
/// itself defines, minus the four host handles. Keep the two lists in
/// step: a new spec global added there belongs here too.
const ECMA_GLOBAL_OBJECTS: [&str; 21] = [
    "console", "Math", "Object", "Number", "String", "Boolean", "JSON", "Array", "Reflect", "Date",
    "WeakRef", "WeakMap", "WeakSet", "Map", "Set", "Symbol", "BigInt", "Promise", "RegExp",
    "Function", "Iterator",
];

/// Whether a `Type::Object(tag)` receiver answers `undefined` for an
/// unmodeled member rather than rejecting the program.
pub(crate) fn answers_undefined(tag: &str) -> bool {
    ecma_global_tag(tag).is_some()
}

/// The `&'static str` table entry for `name`, so a caller holding a
/// namespace ident as a `String` can build the `Type::Object(tag)` the
/// member predicates key on.
pub(crate) fn ecma_global_tag(name: &str) -> Option<&'static str> {
    ECMA_GLOBAL_OBJECTS.iter().copied().find(|&g| g == name)
}

/// Whether `<global>.<name>` reads a member tr does not model — the
/// shared question behind the miss posture, the expando-write gate,
/// and `typeof`'s decision not to guess "function".
pub(crate) fn member_unmodeled(tag: &'static str, name: &str) -> bool {
    !crate::check_type_of_member::member_is_modeled(&crate::check::Type::Object(tag), name)
}

/// Whether `<global>.<name> = value` is an EXPANDO write tr can
/// admit. True only for a name outside the modeled surface.
///
/// The gate is the point. Admitting every name would accept
/// `Math.max = fn` while every static `Math.max(...)` call site kept
/// running the builtin — the silent-wrong faucet rotation 448 named
/// when it admitted exactly the two Promise statics whose call sites
/// read the patch back. A name tr does not model has no such second
/// path: the write lands in the singleton's own-entry dict and the
/// read comes back out of it through the any-member lane, so the two
/// halves agree. Modeled names keep the loud reject until
/// monkey-patching is real.
pub(crate) fn expando_write_admitted(obj_ty: &crate::check::Type, name: &str) -> bool {
    let crate::check::Type::Object(tag) = obj_ty else {
        return false;
    };
    ecma_global_tag(tag).is_some_and(|t| member_unmodeled(t, name))
}
