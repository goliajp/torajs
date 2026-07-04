//! Method-id constants for `any`-receiver method calls
//! (Any-method-call RFC 20260704).
//!
//! ssa-lower interns the compile-time-known method NAME into one of
//! these ids at the call site, so the runtime dispatcher
//! (`__torajs_any_method_call` → per-tag `__torajs_{arr,str}_any_*`)
//! switches on an integer instead of comparing strings — the name
//! bytes still travel alongside for TypeError messages only.
//!
//! Shared at the torajs-rc layer (the one crate every runtime tier
//! and the compiler both depend on), the same placement as `Tag` /
//! `ARR_KIND_*`. Ids are ABI between the baked compiler and the
//! staticlibs — append-only, never renumber.

/// Name not in the intern table — the runtime dispatcher answers a
/// catchable TypeError for every receiver.
pub const ANY_METHOD_UNKNOWN: i64 = 0;
/// `Array.prototype.push`.
pub const ANY_METHOD_PUSH: i64 = 1;
/// `Array.prototype.pop`.
pub const ANY_METHOD_POP: i64 = 2;
/// `String.prototype.charAt`.
pub const ANY_METHOD_CHAR_AT: i64 = 3;
/// `String.prototype.toUpperCase`.
pub const ANY_METHOD_TO_UPPER_CASE: i64 = 4;
/// `String.prototype.toLowerCase`.
pub const ANY_METHOD_TO_LOWER_CASE: i64 = 5;

/// Compile-time name → id intern. `None`-shaped misses map to
/// [`ANY_METHOD_UNKNOWN`] (the runtime throws with the name bytes).
pub fn any_method_id(name: &str) -> i64 {
    match name {
        "push" => ANY_METHOD_PUSH,
        "pop" => ANY_METHOD_POP,
        "charAt" => ANY_METHOD_CHAR_AT,
        "toUpperCase" => ANY_METHOD_TO_UPPER_CASE,
        "toLowerCase" => ANY_METHOD_TO_LOWER_CASE,
        _ => ANY_METHOD_UNKNOWN,
    }
}
