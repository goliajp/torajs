//! The NaN-box `AnyValue` encoding, as the dict reads and writes its
//! entry values.
//!
//! The encoding is owned by torajs-anyvalue (`nanbox.rs` there). The
//! `*_inline` steps restate it with the same constants, so that the
//! entry read behind `get_own`, the overwrite of an existing entry
//! (`set_own` / `set`), and the global-cell and accessor checks are a
//! few inline bit tests instead of a call into another staticlib that
//! the link cannot inline. The one step that is not bit
//! arithmetic — a ShortStr value read as a heap `Str` pointer, which
//! materializes a fresh string — still calls torajs-anyvalue, and so
//! does every other site (`box_from_pair` / `unbox_tag` /
//! `unbox_value`), where an inline copy would only add `__text`. The
//! tests below hold every constant and every step to the
//! torajs-anyvalue original.

unsafe extern "C" {
    /// torajs-anyvalue — the owner's pair decode / encode. The value
    /// decode is also the inline path's step for a ShortStr, whose
    /// pointer form has to be materialized.
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

const TAG_TYPE_NUMBER: u64 = 0xFFFE_0000_0000_0000;
const DOUBLE_ENCODE_OFFSET: u64 = 0x0007_0000_0000_0000;
const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
const TAG_BIT_TYPE_OTHER: u64 = 0x2;
const SHORT_STR_TAG: u64 = 0x0001_0000_0000_0000;
const VALUE_NULL: u64 = 0x02;
const VALUE_FALSE: u64 = 0x06;
const VALUE_TRUE: u64 = 0x07;
const VALUE_UNDEFINED: u64 = 0x0A;

/// The `(tag, value)` pair tags (torajs-rc `AnySlotTag`).
const SLOT_NULL: i64 = 0;
const SLOT_BOOL: i64 = 1;
const SLOT_I64: i64 = 2;
const SLOT_F64: i64 = 3;
const SLOT_HEAP: i64 = 4;
const SLOT_UNDEF: i64 = 5;

#[inline]
const fn is_int32(v: u64) -> bool {
    v & TOP_16_MASK == TAG_TYPE_NUMBER
}

#[inline]
const fn is_double(v: u64) -> bool {
    v & TAG_TYPE_NUMBER != 0 && !is_int32(v)
}

#[inline]
const fn is_cell(v: u64) -> bool {
    v & TOP_16_MASK == 0 && v & TAG_BIT_TYPE_OTHER == 0 && v != 0
}

#[inline]
const fn is_short_str(v: u64) -> bool {
    v & TOP_16_MASK == SHORT_STR_TAG
}

#[inline]
const fn is_bool(v: u64) -> bool {
    v & !1 == VALUE_FALSE
}

#[inline]
fn box_double(x: f64) -> u64 {
    let encoded = x.to_bits().wrapping_add(DOUBLE_ENCODE_OFFSET);
    if is_double(encoded) {
        encoded
    } else {
        // a NaN whose payload lands outside the double range
        f64::NAN.to_bits().wrapping_add(DOUBLE_ENCODE_OFFSET)
    }
}

/// `__torajs_anyv_box_from_pair` — a `(tag, value)` pair as one box.
#[inline]
pub(crate) fn box_from_pair_inline(tag: i64, value: i64) -> u64 {
    match tag {
        SLOT_NULL => VALUE_NULL,
        SLOT_BOOL => {
            if value != 0 {
                VALUE_TRUE
            } else {
                VALUE_FALSE
            }
        }
        SLOT_I64 => match i32::try_from(value) {
            Ok(n) => TAG_TYPE_NUMBER | n as u32 as u64,
            Err(_) => box_double(value as f64),
        },
        SLOT_F64 => box_double(f64::from_bits(value as u64)),
        SLOT_HEAP if value != 0 => value as u64,
        SLOT_UNDEF => VALUE_UNDEFINED,
        _ => VALUE_NULL,
    }
}

/// `__torajs_anyv_unbox_tag` — the box's pair tag. A ShortStr answers
/// Heap (its pair form is a materialized `Str`), and the array-hole
/// word `0` answers undefined.
#[inline]
pub(crate) fn unbox_tag_inline(v: u64) -> i64 {
    if is_int32(v) {
        SLOT_I64
    } else if is_double(v) {
        SLOT_F64
    } else if v == VALUE_NULL {
        SLOT_NULL
    } else if v == VALUE_UNDEFINED {
        SLOT_UNDEF
    } else if is_bool(v) {
        SLOT_BOOL
    } else if is_short_str(v) || is_cell(v) {
        SLOT_HEAP
    } else {
        SLOT_UNDEF
    }
}

/// `__torajs_anyv_unbox_value` for every box but a ShortStr.
#[inline]
fn unbox_value_bits(v: u64) -> i64 {
    if is_int32(v) {
        v as i32 as i64
    } else if is_double(v) {
        v.wrapping_sub(DOUBLE_ENCODE_OFFSET) as i64
    } else if is_bool(v) {
        (v & 1) as i64
    } else if is_cell(v) {
        v as i64
    } else {
        0
    }
}

/// `__torajs_anyv_unbox_value` — the box's pair value. A ShortStr
/// materializes a fresh `Str` (refcount 1) that the caller owns.
///
/// # Safety
/// `v` is a live box.
#[inline]
pub(crate) unsafe fn unbox_value_inline(v: u64) -> i64 {
    if is_short_str(v) {
        return unsafe { __torajs_anyv_unbox_value(v) };
    }
    unbox_value_bits(v)
}

/// The owner's out-of-line encode, for every site off the probe and
/// write paths: a call there costs what it always did, and the inline
/// copy is paid only where the dict reads or writes an entry.
#[inline]
pub(crate) fn box_from_pair(tag: i64, value: i64) -> u64 {
    unsafe { __torajs_anyv_box_from_pair(tag, value) }
}

/// The owner's out-of-line tag decode (see [`box_from_pair`]).
#[inline]
pub(crate) fn unbox_tag(v: u64) -> i64 {
    unsafe { __torajs_anyv_unbox_tag(v) }
}

/// The owner's out-of-line value decode (see [`box_from_pair`]).
///
/// # Safety
/// `v` is a live box.
#[inline]
pub(crate) unsafe fn unbox_value(v: u64) -> i64 {
    unsafe { __torajs_anyv_unbox_value(v) }
}

/// `__torajs_anyv_cell_ptr` — the heap pointer of a cell box, 0 for
/// every immediate (ShortStr included); never materializes.
#[inline]
pub(crate) fn cell_ptr(v: u64) -> i64 {
    if is_cell(v) { v as i64 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_anyvalue::nanbox as av;

    #[test]
    fn constants_are_the_anyvalue_constants() {
        assert_eq!(TAG_TYPE_NUMBER, av::TAG_TYPE_NUMBER);
        assert_eq!(DOUBLE_ENCODE_OFFSET, av::DOUBLE_ENCODE_OFFSET);
        assert_eq!(TOP_16_MASK, av::TOP_16_MASK);
        assert_eq!(TAG_BIT_TYPE_OTHER, av::TAG_BIT_TYPE_OTHER);
        assert_eq!(SHORT_STR_TAG, av::SHORT_STR_TAG);
        assert_eq!(VALUE_NULL, av::VALUE_NULL);
        assert_eq!(VALUE_FALSE, av::VALUE_FALSE);
        assert_eq!(VALUE_TRUE, av::VALUE_TRUE);
        assert_eq!(VALUE_UNDEFINED, av::VALUE_UNDEFINED);
        use torajs_rc::AnySlotTag as S;
        assert_eq!(SLOT_NULL, S::Null as i64);
        assert_eq!(SLOT_BOOL, S::Bool as i64);
        assert_eq!(SLOT_I64, S::I64 as i64);
        assert_eq!(SLOT_F64, S::F64 as i64);
        assert_eq!(SLOT_HEAP, S::Heap as i64);
        assert_eq!(SLOT_UNDEF, S::Undef as i64);
    }

    /// One box of every kind, the encodings torajs-anyvalue mints.
    fn samples() -> Vec<u64> {
        let mut v = vec![
            0,
            av::VALUE_NULL,
            av::VALUE_UNDEFINED,
            av::VALUE_TRUE,
            av::VALUE_FALSE,
            0x0000_1234_5678_9AB0,
            av::try_box_short_str(b"ab").unwrap(),
            av::try_box_short_str(b"").unwrap(),
        ];
        for n in [0, 1, -1, i32::MAX, i32::MIN] {
            v.push(av::box_int32(n));
        }
        for x in [
            0.0,
            -0.0,
            1.5,
            -2.25,
            1e308,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ] {
            v.push(av::box_double(x));
        }
        v
    }

    #[test]
    fn decode_matches_anyvalue() {
        for b in samples() {
            assert_eq!(is_int32(b), av::is_int32(b), "{b:#x}");
            assert_eq!(is_double(b), av::is_double(b), "{b:#x}");
            assert_eq!(is_cell(b), av::is_cell(b), "{b:#x}");
            assert_eq!(is_short_str(b), av::is_short_str(b), "{b:#x}");
            assert_eq!(is_bool(b), av::is_bool(b), "{b:#x}");
            let want_cell = if av::is_cell(b) { b as i64 } else { 0 };
            assert_eq!(cell_ptr(b), want_cell, "{b:#x}");
            if !av::is_short_str(b) {
                let want = if av::is_int32(b) {
                    av::as_int32(b) as i64
                } else if av::is_double(b) {
                    av::as_double(b).to_bits() as i64
                } else if av::is_bool(b) {
                    i64::from(av::as_bool(b))
                } else if av::is_cell(b) {
                    b as i64
                } else {
                    0
                };
                assert_eq!(unbox_value_bits(b), want, "{b:#x}");
            }
        }
    }

    #[test]
    fn tag_and_box_round_trip_like_anyvalue() {
        for b in samples() {
            let tag = unbox_tag_inline(b);
            let want = if av::is_int32(b) {
                SLOT_I64
            } else if av::is_double(b) {
                SLOT_F64
            } else if av::is_null(b) {
                SLOT_NULL
            } else if av::is_undefined(b) {
                SLOT_UNDEF
            } else if av::is_bool(b) {
                SLOT_BOOL
            } else if av::is_short_str(b) || av::is_cell(b) {
                SLOT_HEAP
            } else {
                SLOT_UNDEF
            };
            assert_eq!(tag, want, "{b:#x}");
            if !av::is_short_str(b) && b != 0 {
                assert_eq!(box_from_pair_inline(tag, unbox_value_bits(b)), b, "{b:#x}");
            }
        }
        assert_eq!(
            box_from_pair(SLOT_I64, 1 << 40),
            av::box_double((1u64 << 40) as f64)
        );
        assert_eq!(box_from_pair_inline(SLOT_HEAP, 0), av::VALUE_NULL);
        assert_eq!(box_from_pair_inline(9, 1), av::VALUE_NULL);
        let odd_nan = f64::from_bits(0xFFFA_0000_0000_0000);
        assert_eq!(box_double(odd_nan), av::box_double(odd_nan));
    }
}
