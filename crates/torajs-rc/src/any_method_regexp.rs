//! §22.2.6 — the `RegExp.prototype` accessor getters other than
//! `flags` ([`crate::ANY_METHOD_GET_FLAGS`], 211): the eight per-flag
//! booleans and `source`. Reified getter ids in the `get size`
//! posture: never interned by name, carried by the getter cell,
//! routed by the dispatcher. They continue after
//! [`crate::any_method_buffer`]'s 212-215; same append-only contract,
//! checked by `mid_ids_are_unique`.

/// §22.2.6.6 `get RegExp.prototype.hasIndices`
pub const ANY_METHOD_GET_HAS_INDICES: i64 = 216;
/// §22.2.6.5 `get RegExp.prototype.global`
pub const ANY_METHOD_GET_GLOBAL: i64 = 217;
/// §22.2.6.7 `get RegExp.prototype.ignoreCase`
pub const ANY_METHOD_GET_IGNORE_CASE: i64 = 218;
/// §22.2.6.9 `get RegExp.prototype.multiline`
pub const ANY_METHOD_GET_MULTILINE: i64 = 219;
/// §22.2.6.3 `get RegExp.prototype.dotAll`
pub const ANY_METHOD_GET_DOT_ALL: i64 = 220;
/// §22.2.6.17 `get RegExp.prototype.unicode`
pub const ANY_METHOD_GET_UNICODE: i64 = 221;
/// §22.2.6.18 `get RegExp.prototype.unicodeSets`
pub const ANY_METHOD_GET_UNICODE_SETS: i64 = 222;
/// §22.2.6.14 `get RegExp.prototype.sticky`
pub const ANY_METHOD_GET_STICKY: i64 = 223;
/// §22.2.6.13 `get RegExp.prototype.source`
pub const ANY_METHOD_GET_SOURCE: i64 = 224;

/// `(name, arity)` rows for [`crate::any_method_meta`].
pub(crate) fn regexp_accessor_meta(mid: i64) -> Option<(&'static str, u32)> {
    Some(match mid {
        ANY_METHOD_GET_HAS_INDICES => ("get hasIndices", 0),
        ANY_METHOD_GET_GLOBAL => ("get global", 0),
        ANY_METHOD_GET_IGNORE_CASE => ("get ignoreCase", 0),
        ANY_METHOD_GET_MULTILINE => ("get multiline", 0),
        ANY_METHOD_GET_DOT_ALL => ("get dotAll", 0),
        ANY_METHOD_GET_UNICODE => ("get unicode", 0),
        ANY_METHOD_GET_UNICODE_SETS => ("get unicodeSets", 0),
        ANY_METHOD_GET_STICKY => ("get sticky", 0),
        ANY_METHOD_GET_SOURCE => ("get source", 0),
        _ => return None,
    })
}
