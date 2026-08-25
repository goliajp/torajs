//! Method id → dispatch-family membership (RFC 20260824-s2-5
//! Phase B blade 2b) — the FAMILY→domain judgment table, stored in
//! its inverse form: for one mid, the set of families whose arm
//! kernel can answer it with something other than no-such.
//!
//! The compiler's stub judgment keeps family F alive iff any
//! observed mid names F (`keep |= any_method_families(mid)`), so
//! this table may only err LARGE: an extra bit keeps an arm alive
//! (costs bytes), a missing bit stubs an arm the program enters
//! (costs a loud stub TypeError — caught by gate/sweep, never
//! silent). Universal / skeleton-resident mids (toString, valueOf,
//! call/apply/bind, Object.prototype surface) therefore answer
//! [`FAM_ALL`].
//!
//! Bit order is LOCKSTEP with the arm-seam roster
//! (`torajs-anyvalue/src/dispatch_seam.rs`, torajs-dispatch's
//! `default_arm!` list, and the cli's `FAMILY_ARMS` stub table):
//! str, arr, dynobj, struct, mapset, iter, buffer, date, promise,
//! regexp, bigint, symbol, closure, weak, num. The dynobj / struct
//! / closure expando worlds probe USER properties under any mid, so
//! the judgment side keeps those three whenever the observed set is
//! non-empty — their bits here only mark builtin-owned answers.
//! Append-only in lockstep with the id constants.

use crate::any_method::*;
use crate::any_method_date as d;
use crate::any_method_iter as it;

pub const FAM_STR: u16 = 1 << 0;
pub const FAM_ARR: u16 = 1 << 1;
pub const FAM_DYNOBJ: u16 = 1 << 2;
pub const FAM_STRUCT: u16 = 1 << 3;
pub const FAM_MAPSET: u16 = 1 << 4;
pub const FAM_ITER: u16 = 1 << 5;
pub const FAM_BUFFER: u16 = 1 << 6;
pub const FAM_DATE: u16 = 1 << 7;
pub const FAM_PROMISE: u16 = 1 << 8;
pub const FAM_REGEXP: u16 = 1 << 9;
pub const FAM_BIGINT: u16 = 1 << 10;
pub const FAM_SYMBOL: u16 = 1 << 11;
pub const FAM_CLOSURE: u16 = 1 << 12;
pub const FAM_WEAK: u16 = 1 << 13;
pub const FAM_NUM: u16 = 1 << 14;
/// All fifteen families — the conservative answer for universal /
/// skeleton mids and anything whose owner set is not worth pinning.
pub const FAM_ALL: u16 = (1 << 15) - 1;
/// The obj-world four: the families an OrdinaryToPrimitive run (a
/// user object's own valueOf / toString, Array.prototype.toString
/// = join) or a user-property probe can enter. Shared truth for the
/// compiler judgment's coercion keep and the per-static table in
/// [`crate::ns_static_judge`] — the per-family exotic coercions
/// (Date / RegExp / TypedArray / Symbol / BigInt faces) are NOT
/// here; a value of those families cannot exist without its
/// construction symbols (or a minting static's own family bits).
pub const FAM_OBJ_WORLD: u16 = FAM_DYNOBJ | FAM_STRUCT | FAM_CLOSURE | FAM_ARR;
/// Number of families — must stay equal to the arm-seam roster len.
pub const FAM_COUNT: usize = 15;

/// The families whose arm can answer `mid`. `0` never occurs for a
/// real id — unknown/out-of-table ids answer [`FAM_ALL`] (the
/// judgment layer treats an UNKNOWN observation as a conservative
/// fallback anyway).
pub fn any_method_families(mid: i64) -> u16 {
    const SAB: u16 = FAM_STR | FAM_ARR | FAM_BUFFER;
    const AB: u16 = FAM_ARR | FAM_BUFFER;
    const ABI: u16 = FAM_ARR | FAM_BUFFER | FAM_ITER;
    const MW: u16 = FAM_MAPSET | FAM_WEAK;
    match mid {
        // ---- str-only surface ----
        ANY_METHOD_CHAR_AT
        | ANY_METHOD_TO_UPPER_CASE
        | ANY_METHOD_TO_LOWER_CASE
        | ANY_METHOD_SPLIT
        | ANY_METHOD_TRIM
        | ANY_METHOD_TRIM_START
        | ANY_METHOD_TRIM_END
        | ANY_METHOD_MATCH
        | ANY_METHOD_MATCH_ALL
        | ANY_METHOD_REPLACE
        | ANY_METHOD_REPLACE_ALL
        | ANY_METHOD_SEARCH
        | ANY_METHOD_STARTS_WITH
        | ANY_METHOD_ENDS_WITH
        | ANY_METHOD_SUBSTRING
        | ANY_METHOD_SUBSTR
        | ANY_METHOD_CHAR_CODE_AT
        | ANY_METHOD_CODE_POINT_AT
        | ANY_METHOD_PAD_START
        | ANY_METHOD_PAD_END
        | ANY_METHOD_REPEAT
        | ANY_METHOD_LOCALE_COMPARE
        | ANY_METHOD_NORMALIZE
        | ANY_METHOD_TO_LOCALE_LOWER_CASE
        | ANY_METHOD_TO_LOCALE_UPPER_CASE
        | ANY_METHOD_IS_WELL_FORMED
        | ANY_METHOD_TO_WELL_FORMED
        | ANY_METHOD_ANCHOR
        | ANY_METHOD_FONTCOLOR
        | ANY_METHOD_FONTSIZE
        | ANY_METHOD_LINK
        | ANY_METHOD_BIG
        | ANY_METHOD_BLINK
        | ANY_METHOD_BOLD
        | ANY_METHOD_FIXED
        | ANY_METHOD_ITALICS
        | ANY_METHOD_SMALL
        | ANY_METHOD_STRIKE
        | ANY_METHOD_SUB
        | ANY_METHOD_SUP => FAM_STR,
        // ---- arr-only mutators / shapes ----
        ANY_METHOD_PUSH
        | ANY_METHOD_POP
        | ANY_METHOD_SHIFT
        | ANY_METHOD_UNSHIFT
        | ANY_METHOD_SPLICE
        | ANY_METHOD_FLAT
        | ANY_METHOD_ARR_TO_STRING => FAM_ARR,
        // ---- shared str/arr(/typedarray) index surface ----
        ANY_METHOD_INDEX_OF
        | ANY_METHOD_INCLUDES
        | ANY_METHOD_SLICE
        | ANY_METHOD_LAST_INDEX_OF
        | ANY_METHOD_AT
        | ANY_METHOD_CONCAT => SAB,
        // ---- arr + typedarray ----
        ANY_METHOD_JOIN
        | ANY_METHOD_REVERSE
        | ANY_METHOD_FILL
        | ANY_METHOD_COPY_WITHIN
        | ANY_METHOD_SORT
        | ANY_METHOD_FIND_LAST
        | ANY_METHOD_FIND_LAST_INDEX
        | ANY_METHOD_TO_REVERSED
        | ANY_METHOD_TO_SORTED
        | ANY_METHOD_TO_SPLICED
        | ANY_METHOD_WITH => AB,
        // ---- arr + typedarray + iterator helpers ----
        ANY_METHOD_MAP
        | ANY_METHOD_FILTER
        | ANY_METHOD_EVERY
        | ANY_METHOD_SOME
        | ANY_METHOD_FIND
        | ANY_METHOD_FIND_INDEX
        | ANY_METHOD_REDUCE
        | ANY_METHOD_REDUCE_RIGHT
        | ANY_METHOD_FLAT_MAP => ABI,
        ANY_METHOD_FOR_EACH => ABI | FAM_MAPSET,
        // ---- collection views (arr + map/set + typedarray) ----
        ANY_METHOD_KEYS | ANY_METHOD_VALUES | ANY_METHOD_ENTRIES => AB | FAM_MAPSET,
        // ---- map/set (+ the weak family's shared spellings) ----
        ANY_METHOD_GET | ANY_METHOD_SET | ANY_METHOD_HAS | ANY_METHOD_DELETE | ANY_METHOD_ADD => MW,
        ANY_METHOD_CLEAR
        | ANY_METHOD_UNION
        | ANY_METHOD_INTERSECTION
        | ANY_METHOD_DIFFERENCE
        | ANY_METHOD_SYMMETRIC_DIFFERENCE
        | ANY_METHOD_IS_SUBSET_OF
        | ANY_METHOD_IS_SUPERSET_OF
        | ANY_METHOD_IS_DISJOINT_FROM
        | ANY_METHOD_GET_SIZE
        | ANY_METHOD_GET_OR_INSERT
        | ANY_METHOD_GET_OR_INSERT_COMPUTED => FAM_MAPSET,
        // ---- number formatting ----
        ANY_METHOD_TO_FIXED | ANY_METHOD_TO_EXPONENTIAL | ANY_METHOD_TO_PRECISION => FAM_NUM,
        // ---- regexp ----
        ANY_METHOD_TEST | ANY_METHOD_EXEC | ANY_METHOD_COMPILE => FAM_REGEXP,
        // ---- promise ----
        ANY_METHOD_THEN | ANY_METHOD_CATCH | ANY_METHOD_FINALLY => FAM_PROMISE,
        // ---- symbol ----
        ANY_METHOD_SYMBOL_TO_STRING | ANY_METHOD_SYMBOL_VALUE_OF | ANY_METHOD_GET_DESCRIPTION => {
            FAM_SYMBOL
        }
        // ---- iterator protocol / helpers ----
        ANY_METHOD_NEXT => FAM_ITER,
        m if matches!(
            m,
            it::ANY_METHOD_TAKE
                | it::ANY_METHOD_DROP
                | it::ANY_METHOD_TO_ARRAY
                | it::ANY_METHOD_ITER_SELF
                | it::ANY_METHOD_ITER_DISPOSE
        ) =>
        {
            FAM_ITER
        }
        // gen return rides the closure family's step cells too
        ANY_METHOD_ITER_RETURN => FAM_ITER | FAM_CLOSURE,
        ANY_METHOD_STR_ITERATOR => FAM_STR | FAM_ITER,
        // ---- weak ----
        m if m == it::ANY_METHOD_DEREF => FAM_WEAK,
        // ---- date (toJSON + every id the date sibling owns) ----
        ANY_METHOD_TO_JSON => FAM_DATE,
        m if d::date_owned_mid(m) => FAM_DATE,
        // ---- arraybuffer / typedarray / dataview extras ----
        ANY_METHOD_RESIZE
        | ANY_METHOD_SUBARRAY
        | ANY_METHOD_TRANSFER
        | ANY_METHOD_TRANSFER_TO_FIXED_LENGTH
        | ANY_METHOD_GET_RESIZABLE => FAM_BUFFER,
        m if (ANY_METHOD_DV_GET_INT8..=ANY_METHOD_DV_SET_BIGUINT64).contains(&m) => FAM_BUFFER,
        // ---- universal / skeleton / Object.prototype / fn surface,
        // plus anything whose owner set is not worth pinning ----
        _ => FAM_ALL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every mid the reflection table names must belong somewhere,
    /// and the pinned single-family rows must not have drifted onto
    /// a wrong family (spot checks on both halves of the r492
    /// take/getOrInsertComputed split).
    #[test]
    fn families_cover_and_pin() {
        for mid in 0..256 {
            if crate::any_method_meta(mid).is_some() {
                assert_ne!(any_method_families(mid), 0, "mid {mid} has no family");
            }
        }
        assert_eq!(any_method_families(it::ANY_METHOD_TAKE), FAM_ITER);
        assert_eq!(
            any_method_families(ANY_METHOD_GET_OR_INSERT_COMPUTED),
            FAM_MAPSET
        );
        assert_eq!(any_method_families(ANY_METHOD_CHAR_AT), FAM_STR);
        assert_eq!(any_method_families(d::ANY_METHOD_GET_TIME), FAM_DATE);
        assert_eq!(any_method_families(ANY_METHOD_TO_STRING), FAM_ALL);
        assert_eq!(any_method_families(ANY_METHOD_CALL), FAM_ALL);
        assert_eq!(any_method_families(ANY_METHOD_UNKNOWN), FAM_ALL);
    }
}
