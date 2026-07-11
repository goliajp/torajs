//! Per-receiver-shape builtin-method id tables + the static
//! `<Ctor>.prototype.<m>` method-value read (RFC
//! 20260711-closure-reflection chunk A).
//!
//! Two consumers share the family tables:
//!
//! - [`crate::method_value`]'s `builtin_method_supported` — member
//!   reads off a LIVE `any` receiver (`s.toUpperCase`), where the
//!   shape comes from the runtime tag;
//! - [`__torajs_builtin_proto_method_value`] — the static
//!   `String.prototype.anchor` form, where the shape is known from
//!   the namespace ident at compile time and ssa-lower passes the
//!   builtin-proto tag (`torajs-rc/builtin_proto.rs` order).
//!
//! Each family fn mirrors exactly one `method_call_*` dispatch
//! arm's id-switch — extend together when an arm grows a method
//! (a listed id MUST be callable through `f.call(recv, …)` on that
//! receiver shape, or the reified cell would TypeError on call).

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_ADD, ANY_METHOD_ANCHOR, ANY_METHOD_APPLY, ANY_METHOD_BIND, ANY_METHOD_CALL,
    ANY_METHOD_CHAR_AT, ANY_METHOD_CLEAR, ANY_METHOD_CONCAT, ANY_METHOD_COPY_WITHIN,
    ANY_METHOD_DELETE, ANY_METHOD_ENDS_WITH, ANY_METHOD_ENTRIES, ANY_METHOD_EVERY, ANY_METHOD_EXEC,
    ANY_METHOD_FILL, ANY_METHOD_FILTER, ANY_METHOD_FIND, ANY_METHOD_FIND_INDEX,
    ANY_METHOD_FOR_EACH, ANY_METHOD_GET, ANY_METHOD_HAS, ANY_METHOD_HAS_OWN_PROPERTY,
    ANY_METHOD_INCLUDES, ANY_METHOD_INDEX_OF, ANY_METHOD_JOIN, ANY_METHOD_KEYS,
    ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_MAP, ANY_METHOD_MATCH, ANY_METHOD_POP,
    ANY_METHOD_PROPERTY_IS_ENUMERABLE, ANY_METHOD_PUSH, ANY_METHOD_REDUCE, ANY_METHOD_REDUCE_RIGHT,
    ANY_METHOD_REPLACE, ANY_METHOD_REPLACE_ALL, ANY_METHOD_REVERSE, ANY_METHOD_SET,
    ANY_METHOD_SHIFT, ANY_METHOD_SLICE, ANY_METHOD_SOME, ANY_METHOD_SORT, ANY_METHOD_SPLICE,
    ANY_METHOD_SPLIT, ANY_METHOD_STARTS_WITH, ANY_METHOD_SUP, ANY_METHOD_TEST,
    ANY_METHOD_TO_EXPONENTIAL, ANY_METHOD_TO_FIXED, ANY_METHOD_TO_LOCALE_STRING,
    ANY_METHOD_TO_LOWER_CASE, ANY_METHOD_TO_PRECISION, ANY_METHOD_TO_STRING,
    ANY_METHOD_TO_UPPER_CASE, ANY_METHOD_TRIM, ANY_METHOD_TRIM_END, ANY_METHOD_TRIM_START,
    ANY_METHOD_UNKNOWN, ANY_METHOD_UNSHIFT, ANY_METHOD_VALUE_OF, ANY_METHOD_VALUES,
};

use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
}

/// `method_call_str` arm ids (+ the dispatcher's toString identity).
pub(crate) fn str_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_TO_STRING
            | ANY_METHOD_CHAR_AT
            | ANY_METHOD_TO_UPPER_CASE
            | ANY_METHOD_TO_LOWER_CASE
            | ANY_METHOD_INDEX_OF
            | ANY_METHOD_INCLUDES
            | ANY_METHOD_SLICE
            | ANY_METHOD_SPLIT
            | ANY_METHOD_TRIM
            | ANY_METHOD_TRIM_START
            | ANY_METHOD_TRIM_END
            | ANY_METHOD_MATCH
            | ANY_METHOD_REPLACE
            | ANY_METHOD_REPLACE_ALL
            | ANY_METHOD_STARTS_WITH
            | ANY_METHOD_ENDS_WITH
            // annexB B.2.2 html methods — ids 95-107 are contiguous
            // by design (attr forms first), one range covers the
            // family.
            | ANY_METHOD_ANCHOR..=ANY_METHOD_SUP
    )
}

/// `method_call_num` arm ids.
pub(crate) fn num_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_TO_STRING
            | ANY_METHOD_TO_FIXED
            | ANY_METHOD_TO_EXPONENTIAL
            | ANY_METHOD_TO_PRECISION
            | ANY_METHOD_TO_LOCALE_STRING
            | ANY_METHOD_VALUE_OF
    )
}

/// `method_call_arr` arm ids.
pub(crate) fn arr_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_PUSH
            | ANY_METHOD_POP
            | ANY_METHOD_SHIFT
            | ANY_METHOD_UNSHIFT
            | ANY_METHOD_INDEX_OF
            | ANY_METHOD_LAST_INDEX_OF
            | ANY_METHOD_INCLUDES
            | ANY_METHOD_JOIN
            | ANY_METHOD_SLICE
            | ANY_METHOD_MAP
            | ANY_METHOD_FILTER
            | ANY_METHOD_FOR_EACH
            | ANY_METHOD_REVERSE
            | ANY_METHOD_CONCAT
            | ANY_METHOD_FILL
            | ANY_METHOD_COPY_WITHIN
            | ANY_METHOD_SPLICE
            | ANY_METHOD_EVERY
            | ANY_METHOD_SOME
            | ANY_METHOD_FIND
            | ANY_METHOD_FIND_INDEX
            | ANY_METHOD_REDUCE
            | ANY_METHOD_REDUCE_RIGHT
            | ANY_METHOD_SORT
            | ANY_METHOD_KEYS
            | ANY_METHOD_VALUES
            | ANY_METHOD_ENTRIES
    )
}

/// `method_call_mapset` Map arm ids.
pub(crate) fn map_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_GET
            | ANY_METHOD_SET
            | ANY_METHOD_HAS
            | ANY_METHOD_DELETE
            | ANY_METHOD_CLEAR
            | ANY_METHOD_FOR_EACH
            | ANY_METHOD_KEYS
            | ANY_METHOD_VALUES
            | ANY_METHOD_ENTRIES
    )
}

/// `method_call_mapset` Set arm ids.
pub(crate) fn set_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_ADD
            | ANY_METHOD_HAS
            | ANY_METHOD_DELETE
            | ANY_METHOD_CLEAR
            | ANY_METHOD_FOR_EACH
            | ANY_METHOD_KEYS
            | ANY_METHOD_VALUES
            | ANY_METHOD_ENTRIES
    )
}

/// `method_call_regexp` arm ids.
pub(crate) fn regexp_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_TEST | ANY_METHOD_EXEC | ANY_METHOD_TO_STRING
    )
}

/// `method_call_weak` WeakMap arm ids.
pub(crate) fn weakmap_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_GET | ANY_METHOD_SET | ANY_METHOD_HAS | ANY_METHOD_DELETE
    )
}

/// `method_call_weak` WeakSet arm ids.
pub(crate) fn weakset_supports(mid: i64) -> bool {
    matches!(mid, ANY_METHOD_ADD | ANY_METHOD_HAS | ANY_METHOD_DELETE)
}

/// `method_call_closure` arm ids — `Function.prototype`'s surface.
pub(crate) fn closure_supports(mid: i64) -> bool {
    matches!(mid, ANY_METHOD_CALL | ANY_METHOD_APPLY | ANY_METHOD_BIND)
}

/// `method_call_date` arm ids — the getter / setter / to*String
/// table (ids 25-62 with the annexB aliases).
pub(crate) fn date_supports(mid: i64) -> bool {
    use torajs_rc::{
        ANY_METHOD_GET_DATE, ANY_METHOD_GET_DAY, ANY_METHOD_GET_FULL_YEAR, ANY_METHOD_GET_HOURS,
        ANY_METHOD_GET_MILLISECONDS, ANY_METHOD_GET_MINUTES, ANY_METHOD_GET_MONTH,
        ANY_METHOD_GET_SECONDS, ANY_METHOD_GET_TIME, ANY_METHOD_GET_TIMEZONE_OFFSET,
        ANY_METHOD_GET_UTC_DATE, ANY_METHOD_GET_UTC_DAY, ANY_METHOD_GET_UTC_FULL_YEAR,
        ANY_METHOD_GET_UTC_HOURS, ANY_METHOD_GET_UTC_MILLISECONDS, ANY_METHOD_GET_UTC_MINUTES,
        ANY_METHOD_GET_UTC_MONTH, ANY_METHOD_GET_UTC_SECONDS, ANY_METHOD_GET_YEAR,
        ANY_METHOD_SET_DATE, ANY_METHOD_SET_FULL_YEAR, ANY_METHOD_SET_HOURS,
        ANY_METHOD_SET_MILLISECONDS, ANY_METHOD_SET_MINUTES, ANY_METHOD_SET_MONTH,
        ANY_METHOD_SET_SECONDS, ANY_METHOD_SET_TIME, ANY_METHOD_SET_YEAR,
        ANY_METHOD_TO_DATE_STRING, ANY_METHOD_TO_GMT_STRING, ANY_METHOD_TO_ISO_STRING,
        ANY_METHOD_TO_JSON, ANY_METHOD_TO_LOCALE_DATE_STRING, ANY_METHOD_TO_LOCALE_TIME_STRING,
        ANY_METHOD_TO_UTC_STRING,
    };
    matches!(
        mid,
        ANY_METHOD_GET_TIME
            | ANY_METHOD_VALUE_OF
            | ANY_METHOD_TO_STRING
            | ANY_METHOD_TO_ISO_STRING
            | ANY_METHOD_TO_JSON
            | ANY_METHOD_TO_LOCALE_STRING
            | ANY_METHOD_GET_FULL_YEAR
            | ANY_METHOD_GET_UTC_FULL_YEAR
            | ANY_METHOD_GET_MONTH
            | ANY_METHOD_GET_UTC_MONTH
            | ANY_METHOD_GET_DATE
            | ANY_METHOD_GET_UTC_DATE
            | ANY_METHOD_GET_HOURS
            | ANY_METHOD_GET_UTC_HOURS
            | ANY_METHOD_GET_MINUTES
            | ANY_METHOD_GET_UTC_MINUTES
            | ANY_METHOD_GET_SECONDS
            | ANY_METHOD_GET_UTC_SECONDS
            | ANY_METHOD_GET_MILLISECONDS
            | ANY_METHOD_GET_UTC_MILLISECONDS
            | ANY_METHOD_GET_DAY
            | ANY_METHOD_GET_UTC_DAY
            | ANY_METHOD_GET_TIMEZONE_OFFSET
            | ANY_METHOD_GET_YEAR
            | ANY_METHOD_SET_TIME
            | ANY_METHOD_SET_YEAR
            | ANY_METHOD_SET_FULL_YEAR
            | ANY_METHOD_SET_MONTH
            | ANY_METHOD_SET_DATE
            | ANY_METHOD_SET_HOURS
            | ANY_METHOD_SET_MINUTES
            | ANY_METHOD_SET_SECONDS
            | ANY_METHOD_SET_MILLISECONDS
            | ANY_METHOD_TO_UTC_STRING
            | ANY_METHOD_TO_GMT_STRING
            | ANY_METHOD_TO_DATE_STRING
            | ANY_METHOD_TO_LOCALE_DATE_STRING
            | ANY_METHOD_TO_LOCALE_TIME_STRING
    )
}

/// The builtin-proto tag's method surface — tag order is locked to
/// `torajs-rc/builtin_proto.rs` (Number=0 … Function=13). Tags with
/// no any-dispatch method arm today (Object beyond the universal
/// probes / Symbol / BigInt / Error / Promise) answer false — the
/// read stays undefined rather than minting a cell that would
/// TypeError on `f.call(recv, …)`.
fn proto_tag_supports(tag: i64, mid: i64) -> bool {
    // Object.prototype's universal own-property probes resolve on
    // every receiver shape, so every builtin prototype hands them
    // out (mirrors `builtin_method_supported`'s leading arm); the
    // per-family surface is shared with the own-property variant.
    if mid == ANY_METHOD_HAS_OWN_PROPERTY || mid == ANY_METHOD_PROPERTY_IS_ENUMERABLE {
        return true;
    }
    proto_tag_owns(tag, mid)
}

/// Own-property variant of [`proto_tag_supports`] (RFC 20260712
/// chunk 1) — the readable surface counts inherited methods (the
/// universal `Object.prototype` probes resolve on every prototype),
/// but an own-property probe must NOT: `hasOwnProperty` /
/// `propertyIsEnumerable` are own only on `Object.prototype`
/// (tag 1). Every family method IS its prototype's own property
/// per spec.
pub(crate) fn proto_tag_owns(tag: i64, mid: i64) -> bool {
    if mid == ANY_METHOD_HAS_OWN_PROPERTY || mid == ANY_METHOD_PROPERTY_IS_ENUMERABLE {
        return tag == 1;
    }
    match tag {
        0 => num_supports(mid),
        2 => arr_supports(mid),
        3 => str_supports(mid),
        4 => mid == ANY_METHOD_TO_STRING,
        7 => regexp_supports(mid),
        8 => date_supports(mid),
        11 => map_supports(mid),
        12 => set_supports(mid),
        13 => closure_supports(mid),
        _ => false,
    }
}

/// `<Ctor>.prototype.<m>` static member read (chunk A). Own-entry
/// probe on the builtin-proto singleton dynobj first — a user
/// monkey-patch (`String.prototype.foo = …`) wins over the interned
/// builtin cell, matching ES own-property-before-intrinsic order.
/// Data-tagged hits (null/bool/i64/f64/heap) box out under the
/// owned protocol; absent (5, 0) and accessor-sentinel entries fall
/// through to the method-cell table. Unknown / unsupported names
/// answer undefined (bun's wrong-arm shape).
///
/// # Safety
/// `key` is NULL or a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_method_value(
    tag: i64,
    key: *const c_void,
) -> AnyValue {
    if key.is_null() {
        return VALUE_UNDEFINED;
    }
    let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(tag) };
    if !proto.is_null() {
        let dtag = unsafe { __torajs_dynobj_get_tag(proto, key) } as i64;
        if (0..=4).contains(&dtag) {
            let dval = unsafe { __torajs_dynobj_get_value(proto, key) } as i64;
            // The probe pair is a borrow — the returned box owns
            // its own reference.
            crate::payload_rc_inc(dtag, dval);
            return unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(dtag, dval) };
        }
    }
    let mid = unsafe { crate::method_value::key_method_id(key) };
    if mid == ANY_METHOD_UNKNOWN
        || (mid as usize) >= crate::method_value::TABLE_SIZE
        || !proto_tag_supports(tag, mid)
    {
        return VALUE_UNDEFINED;
    }
    // Immortal interned cell — rc traffic no-ops, hand out as-is.
    crate::method_value::builtin_method_cell(mid) as AnyValue
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_tags_route_to_their_family() {
        // String proto answers str methods, not date's.
        assert!(proto_tag_supports(3, ANY_METHOD_SLICE));
        assert!(!proto_tag_supports(3, torajs_rc::ANY_METHOD_GET_TIME));
        // Date proto answers annexB getYear.
        assert!(proto_tag_supports(8, torajs_rc::ANY_METHOD_GET_YEAR));
        // Function proto answers call/apply/bind only.
        assert!(proto_tag_supports(13, ANY_METHOD_CALL));
        assert!(!proto_tag_supports(13, ANY_METHOD_SLICE));
        // No-arm tags answer false for ordinary methods…
        assert!(!proto_tag_supports(9, ANY_METHOD_TO_STRING));
        // …but the universal probes resolve everywhere.
        assert!(proto_tag_supports(9, ANY_METHOD_HAS_OWN_PROPERTY));
        assert!(proto_tag_supports(1, ANY_METHOD_PROPERTY_IS_ENUMERABLE));
    }
}
