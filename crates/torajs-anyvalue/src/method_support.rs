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

use torajs_rc::{
    ANY_METHOD_ADD, ANY_METHOD_ANCHOR, ANY_METHOD_APPLY, ANY_METHOD_AT, ANY_METHOD_BIND,
    ANY_METHOD_CALL, ANY_METHOD_CHAR_AT, ANY_METHOD_CHAR_CODE_AT, ANY_METHOD_CLEAR,
    ANY_METHOD_CODE_POINT_AT, ANY_METHOD_CONCAT, ANY_METHOD_COPY_WITHIN, ANY_METHOD_DELETE,
    ANY_METHOD_DIFFERENCE, ANY_METHOD_ENDS_WITH, ANY_METHOD_ENTRIES, ANY_METHOD_EVERY,
    ANY_METHOD_EXEC, ANY_METHOD_FILL, ANY_METHOD_FILTER, ANY_METHOD_FIND, ANY_METHOD_FIND_INDEX,
    ANY_METHOD_FIND_LAST, ANY_METHOD_FIND_LAST_INDEX, ANY_METHOD_FLAT, ANY_METHOD_FLAT_MAP,
    ANY_METHOD_FOR_EACH, ANY_METHOD_GET, ANY_METHOD_GET_OR_INSERT, ANY_METHOD_HAS,
    ANY_METHOD_INCLUDES, ANY_METHOD_INDEX_OF, ANY_METHOD_INTERSECTION, ANY_METHOD_IS_DISJOINT_FROM,
    ANY_METHOD_IS_SUBSET_OF, ANY_METHOD_IS_SUPERSET_OF, ANY_METHOD_IS_WELL_FORMED, ANY_METHOD_JOIN,
    ANY_METHOD_KEYS, ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_LOCALE_COMPARE, ANY_METHOD_MAP,
    ANY_METHOD_MATCH, ANY_METHOD_MATCH_ALL, ANY_METHOD_NORMALIZE, ANY_METHOD_PAD_END,
    ANY_METHOD_PAD_START, ANY_METHOD_POP, ANY_METHOD_PUSH, ANY_METHOD_REDUCE,
    ANY_METHOD_REDUCE_RIGHT, ANY_METHOD_REPEAT, ANY_METHOD_REPLACE, ANY_METHOD_REPLACE_ALL,
    ANY_METHOD_RESIZE, ANY_METHOD_REVERSE, ANY_METHOD_SEARCH, ANY_METHOD_SET, ANY_METHOD_SHIFT,
    ANY_METHOD_SLICE, ANY_METHOD_SOME, ANY_METHOD_SORT, ANY_METHOD_SPLICE, ANY_METHOD_SPLIT,
    ANY_METHOD_STARTS_WITH, ANY_METHOD_SUBARRAY, ANY_METHOD_SUBSTR, ANY_METHOD_SUBSTRING,
    ANY_METHOD_SUP, ANY_METHOD_SYMMETRIC_DIFFERENCE, ANY_METHOD_TEST, ANY_METHOD_TO_EXPONENTIAL,
    ANY_METHOD_TO_FIXED, ANY_METHOD_TO_LOCALE_LOWER_CASE, ANY_METHOD_TO_LOCALE_STRING,
    ANY_METHOD_TO_LOCALE_UPPER_CASE, ANY_METHOD_TO_LOWER_CASE, ANY_METHOD_TO_PRECISION,
    ANY_METHOD_TO_REVERSED, ANY_METHOD_TO_SORTED, ANY_METHOD_TO_SPLICED, ANY_METHOD_TO_STRING,
    ANY_METHOD_TO_UPPER_CASE, ANY_METHOD_TO_WELL_FORMED, ANY_METHOD_TRANSFER,
    ANY_METHOD_TRANSFER_TO_FIXED_LENGTH, ANY_METHOD_TRIM, ANY_METHOD_TRIM_END,
    ANY_METHOD_TRIM_START, ANY_METHOD_UNION, ANY_METHOD_UNSHIFT, ANY_METHOD_VALUE_OF,
    ANY_METHOD_VALUES, ANY_METHOD_WITH,
};

// Facade for the split-out builtin-proto faces — callers keep the
// crate::method_support::* paths (sibling split, file-size limit).
pub(crate) use crate::method_support_proto::{
    __torajs_builtin_proto_own_method_cell, proto_tag_accessor_mid, proto_tag_family_owns,
    proto_tag_owns,
};

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
            | ANY_METHOD_SUBSTRING
            | ANY_METHOD_SUBSTR
            | ANY_METHOD_AT
            | ANY_METHOD_CHAR_CODE_AT
            | ANY_METHOD_PAD_START
            | ANY_METHOD_PAD_END
            | ANY_METHOD_REPEAT
            | ANY_METHOD_CONCAT
            | ANY_METHOD_CODE_POINT_AT
            | ANY_METHOD_LOCALE_COMPARE
            | ANY_METHOD_NORMALIZE
            | ANY_METHOD_TO_LOCALE_LOWER_CASE
            | ANY_METHOD_TO_LOCALE_UPPER_CASE
            | ANY_METHOD_LAST_INDEX_OF
            | ANY_METHOD_SEARCH
            | ANY_METHOD_MATCH_ALL
            | ANY_METHOD_IS_WELL_FORMED
            | ANY_METHOD_TO_WELL_FORMED
            | ANY_METHOD_VALUE_OF
            | ANY_METHOD_TO_LOCALE_STRING
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
            | ANY_METHOD_TO_LOCALE_STRING
            | ANY_METHOD_TO_STRING
            | ANY_METHOD_AT
            | ANY_METHOD_FLAT
            | ANY_METHOD_FLAT_MAP
            | ANY_METHOD_FIND_LAST
            | ANY_METHOD_FIND_LAST_INDEX
            | ANY_METHOD_TO_REVERSED
            | ANY_METHOD_TO_SORTED
            | ANY_METHOD_TO_SPLICED
            | ANY_METHOD_WITH
    )
}

/// `method_call_mapset` Map arm ids.
pub(crate) fn map_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_GET
            | ANY_METHOD_GET_OR_INSERT
            | torajs_rc::ANY_METHOD_GET_OR_INSERT_COMPUTED
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
            | ANY_METHOD_UNION
            | ANY_METHOD_INTERSECTION
            | ANY_METHOD_DIFFERENCE
            | ANY_METHOD_SYMMETRIC_DIFFERENCE
            | ANY_METHOD_IS_SUBSET_OF
            | ANY_METHOD_IS_SUPERSET_OF
            | ANY_METHOD_IS_DISJOINT_FROM
    )
}

/// `method_call_regexp` arm ids.
pub(crate) fn regexp_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_TEST | ANY_METHOD_EXEC | ANY_METHOD_TO_STRING | torajs_rc::ANY_METHOD_COMPILE
    )
}

/// `method_call_weak` WeakMap arm ids.
pub(crate) fn weakmap_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_GET
            | ANY_METHOD_GET_OR_INSERT
            | torajs_rc::ANY_METHOD_GET_OR_INSERT_COMPUTED
            | ANY_METHOD_SET
            | ANY_METHOD_HAS
            | ANY_METHOD_DELETE
    )
}

/// `method_call_weak` WeakSet arm ids.
pub(crate) fn weakset_supports(mid: i64) -> bool {
    matches!(mid, ANY_METHOD_ADD | ANY_METHOD_HAS | ANY_METHOD_DELETE)
}

/// `method_call_weak` WeakRef arm ids — §26.1.4.2 gives the family
/// exactly one method.
pub(crate) fn weakref_supports(mid: i64) -> bool {
    mid == torajs_rc::any_method_iter::ANY_METHOD_DEREF
}

/// `method_call_closure` arm ids — `Function.prototype`'s surface.
/// TO_STRING paired with the B4 source-text arm (RFC
/// 20260719-fn-tostring-source; supported table and dispatcher move
/// in the same commit — rotation-148 drift lesson).
pub(crate) fn closure_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_CALL
            | ANY_METHOD_APPLY
            | ANY_METHOD_BIND
            | ANY_METHOD_TO_STRING
            | ANY_METHOD_TO_LOCALE_STRING
    )
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
        ANY_METHOD_SET_SECONDS, ANY_METHOD_SET_TIME, ANY_METHOD_SET_UTC_DATE,
        ANY_METHOD_SET_UTC_FULL_YEAR, ANY_METHOD_SET_UTC_HOURS, ANY_METHOD_SET_UTC_MILLISECONDS,
        ANY_METHOD_SET_UTC_MINUTES, ANY_METHOD_SET_UTC_MONTH, ANY_METHOD_SET_UTC_SECONDS,
        ANY_METHOD_SET_YEAR, ANY_METHOD_TO_DATE_STRING, ANY_METHOD_TO_GMT_STRING,
        ANY_METHOD_TO_ISO_STRING, ANY_METHOD_TO_JSON, ANY_METHOD_TO_LOCALE_DATE_STRING,
        ANY_METHOD_TO_LOCALE_TIME_STRING, ANY_METHOD_TO_TIME_STRING, ANY_METHOD_TO_UTC_STRING,
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
            | ANY_METHOD_SET_UTC_FULL_YEAR
            | ANY_METHOD_SET_UTC_MONTH
            | ANY_METHOD_SET_UTC_DATE
            | ANY_METHOD_SET_UTC_HOURS
            | ANY_METHOD_SET_UTC_MINUTES
            | ANY_METHOD_SET_UTC_SECONDS
            | ANY_METHOD_SET_UTC_MILLISECONDS
            | ANY_METHOD_TO_UTC_STRING
            | ANY_METHOD_TO_GMT_STRING
            | ANY_METHOD_TO_DATE_STRING
            | ANY_METHOD_TO_TIME_STRING
            | ANY_METHOD_TO_LOCALE_DATE_STRING
            | ANY_METHOD_TO_LOCALE_TIME_STRING
    )
}

/// §25.1.6 `ArrayBuffer.prototype`'s own methods (RFC
/// 20260823-typedarray-substrate). The accessors (`byteLength`,
/// `maxByteLength`, `resizable`, `detached`) are getters, not
/// methods, and answer through the member-read face instead.
pub(crate) fn arraybuffer_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_SLICE
            | ANY_METHOD_RESIZE
            | ANY_METHOD_TRANSFER
            | ANY_METHOD_TRANSFER_TO_FIXED_LENGTH
    )
}

/// §23.2.3 `%TypedArray%.prototype`'s own methods — slab A and
/// slab B together, which is exactly the set
/// `method_call_buffer::typedarray_method` resolves.
///
/// `toString` is absent for the same reason every other cell's is:
/// the universal arm above already answers it.
pub(crate) fn typedarray_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_AT
            | ANY_METHOD_FILL
            | ANY_METHOD_COPY_WITHIN
            | ANY_METHOD_REVERSE
            | ANY_METHOD_INDEX_OF
            | ANY_METHOD_LAST_INDEX_OF
            | ANY_METHOD_INCLUDES
            | ANY_METHOD_SUBARRAY
            | ANY_METHOD_SLICE
            | ANY_METHOD_TO_REVERSED
            | ANY_METHOD_WITH
            | ANY_METHOD_SET
            | ANY_METHOD_SORT
            | ANY_METHOD_TO_SORTED
            | ANY_METHOD_JOIN
            | ANY_METHOD_FOR_EACH
            | ANY_METHOD_MAP
            | ANY_METHOD_FILTER
            | ANY_METHOD_EVERY
            | ANY_METHOD_SOME
            | ANY_METHOD_FIND
            | ANY_METHOD_FIND_INDEX
            | ANY_METHOD_FIND_LAST
            | ANY_METHOD_FIND_LAST_INDEX
            | ANY_METHOD_REDUCE
            | ANY_METHOD_REDUCE_RIGHT
            | ANY_METHOD_KEYS
            | ANY_METHOD_VALUES
            | ANY_METHOD_ENTRIES
    )
}
