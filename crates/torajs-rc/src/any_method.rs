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
/// `String.prototype.indexOf` / `Array.prototype.indexOf`.
pub const ANY_METHOD_INDEX_OF: i64 = 6;
/// `String.prototype.includes` / `Array.prototype.includes`.
pub const ANY_METHOD_INCLUDES: i64 = 7;
/// `String.prototype.slice` (`Array.prototype.slice` is a C4+ arm).
pub const ANY_METHOD_SLICE: i64 = 8;
/// `String.prototype.split`.
pub const ANY_METHOD_SPLIT: i64 = 9;
/// `String.prototype.trim`.
pub const ANY_METHOD_TRIM: i64 = 10;
/// `String.prototype.trimStart`.
pub const ANY_METHOD_TRIM_START: i64 = 11;
/// `String.prototype.trimEnd`.
pub const ANY_METHOD_TRIM_END: i64 = 12;
/// `Array.prototype.shift`.
pub const ANY_METHOD_SHIFT: i64 = 13;
/// `Array.prototype.unshift`.
pub const ANY_METHOD_UNSHIFT: i64 = 14;
/// `Array.prototype.join`.
pub const ANY_METHOD_JOIN: i64 = 15;

/// Compile-time name → id intern. `None`-shaped misses map to
/// [`ANY_METHOD_UNKNOWN`] (the runtime throws with the name bytes).
pub fn any_method_id(name: &str) -> i64 {
    match name {
        "push" => ANY_METHOD_PUSH,
        "pop" => ANY_METHOD_POP,
        "charAt" => ANY_METHOD_CHAR_AT,
        "toUpperCase" => ANY_METHOD_TO_UPPER_CASE,
        "toLowerCase" => ANY_METHOD_TO_LOWER_CASE,
        "indexOf" => ANY_METHOD_INDEX_OF,
        "includes" => ANY_METHOD_INCLUDES,
        "slice" => ANY_METHOD_SLICE,
        "split" => ANY_METHOD_SPLIT,
        "trim" => ANY_METHOD_TRIM,
        "trimStart" => ANY_METHOD_TRIM_START,
        "trimEnd" => ANY_METHOD_TRIM_END,
        "shift" => ANY_METHOD_SHIFT,
        "unshift" => ANY_METHOD_UNSHIFT,
        "join" => ANY_METHOD_JOIN,
        _ => ANY_METHOD_UNKNOWN,
    }
}
