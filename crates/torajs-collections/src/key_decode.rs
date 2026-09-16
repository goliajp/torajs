//! In-crate decode of a stored entry key + an owned-Str equality fast
//! path — the two per-hit steps of a Map/Set lookup that used to leave
//! the crate.
//!
//! A lookup hit paid two extern calls to turn the entry's NaN-box key
//! back into its `(tag, payload)` pair and one more (`__torajs_str_eq`,
//! then `memcmp`) to compare string content: 30 of the 134 ns a
//! `map-str-100k` iteration spent (decomposition 2026-09-17, S8 + S9).
//!
//! Both are narrow-ABI mirrors, the same pattern [`crate::layout`]
//! already uses for the heap header and `TAG_STR`:
//!
//! * [`stored_pair`] mirrors `__torajs_anyv_unbox_tag` /
//!   `__torajs_anyv_unbox_value` over the image of
//!   `__torajs_anyv_box_from_pair`, which is the only writer of
//!   `MapEntry::key_anyv` / `value_anyv` (delete writes 0). That image
//!   holds no ShortStr; one would still decode through the extern pair
//!   (it materializes a Str).
//! * [`owned_str_eq`] answers only for two plain owned Str cells of the
//!   same encoding; Substr views and the `undefined` sentinel cell
//!   (identity-compared, never by its text) go to `__torajs_str_eq`.

use crate::layout::{ANY_BOOL, ANY_F64, ANY_HEAP, ANY_I64, ANY_NULL, ANY_UNDEF, HeapHeader};

unsafe extern "C" {
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-str's `undefined` sentinel cell; compared by address.
    static __TORAJS_STR_UNDEF_CELL: u8;
}

// NaN-box encoding (mirror of torajs-anyvalue `nanbox`).
const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
const TAG_TYPE_NUMBER: u64 = 0xFFFE_0000_0000_0000;
const DOUBLE_ENCODE_OFFSET: u64 = 0x0007_0000_0000_0000;
const VALUE_NULL: u64 = 0x02;
const VALUE_UNDEFINED: u64 = 0x0A;
const VALUE_FALSE: u64 = 0x06;
const TAG_BIT_TYPE_OTHER: u64 = 0x02;

// Str layout (mirror of torajs-str `layout` / `substr`).
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;
const STR_FLAG_IS_LATIN1: u16 = 0x0002;
const FLAG_SUBSTR_INLINE: u16 = 1 << 0;
const FLAG_SUBSTR_VIEW: u16 = 1 << 10;

/// `(tag, payload)` of a stored entry key or value — the pair
/// `__torajs_anyv_unbox_tag` / `__torajs_anyv_unbox_value` answer.
#[inline]
pub(crate) fn stored_pair(v: u64) -> (u8, u64) {
    let top = v & TOP_16_MASK;
    if top == TAG_TYPE_NUMBER {
        (ANY_I64, v as i32 as i64 as u64)
    } else if v & TAG_TYPE_NUMBER != 0 {
        (ANY_F64, v.wrapping_sub(DOUBLE_ENCODE_OFFSET))
    } else if top == 0 && v & TAG_BIT_TYPE_OTHER == 0 && v != 0 {
        (ANY_HEAP, v)
    } else if v == VALUE_NULL || v == 0 {
        (ANY_NULL, 0)
    } else if v == VALUE_UNDEFINED {
        (ANY_UNDEF, 0)
    } else if v & !1 == VALUE_FALSE {
        (ANY_BOOL, v & 1)
    } else {
        // ShortStr (never written by box_from_pair) — the extern pair
        // owns its materialization.
        unsafe {
            (
                __torajs_anyv_unbox_tag(v) as u8,
                __torajs_anyv_unbox_value(v) as u64,
            )
        }
    }
}

/// Content equality of two distinct non-null Str cells, when both are
/// plain owned strings of the same encoding; `None` sends the pair to
/// `__torajs_str_eq`.
///
/// # Safety
/// `a` and `b` are live heap cells whose type_tag is `TAG_STR`.
#[inline]
pub(crate) unsafe fn owned_str_eq(a: *const u8, b: *const u8) -> Option<bool> {
    let undef = unsafe { &__TORAJS_STR_UNDEF_CELL as *const u8 };
    if a == undef || b == undef {
        return None;
    }
    let fa = unsafe { (*(a as *const HeapHeader)).flags };
    let fb = unsafe { (*(b as *const HeapHeader)).flags };
    if (fa | fb) & (FLAG_SUBSTR_VIEW | FLAG_SUBSTR_INLINE) != 0
        || (fa ^ fb) & STR_FLAG_IS_LATIN1 != 0
    {
        return None;
    }
    let len = unsafe { (a.add(STR_LEN_OFF) as *const u32).read() };
    if len != unsafe { (b.add(STR_LEN_OFF) as *const u32).read() } {
        return Some(false);
    }
    let stride = if fa & STR_FLAG_IS_LATIN1 != 0 { 1 } else { 2 };
    let n = len as usize * stride;
    let pa = unsafe { core::slice::from_raw_parts(a.add(STR_DATA_OFF), n) };
    let pb = unsafe { core::slice::from_raw_parts(b.add(STR_DATA_OFF), n) };
    Some(pa == pb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediates_decode_to_their_pairs() {
        assert_eq!(
            stored_pair(TAG_TYPE_NUMBER | 0xFFFF_FFFF),
            (ANY_I64, (-1i64) as u64)
        );
        assert_eq!(stored_pair(TAG_TYPE_NUMBER | 42), (ANY_I64, 42));
        let bits = 1.5f64.to_bits();
        assert_eq!(stored_pair(bits + DOUBLE_ENCODE_OFFSET), (ANY_F64, bits));
        assert_eq!(stored_pair(VALUE_NULL), (ANY_NULL, 0));
        assert_eq!(stored_pair(VALUE_UNDEFINED), (ANY_UNDEF, 0));
        assert_eq!(stored_pair(VALUE_FALSE), (ANY_BOOL, 0));
        assert_eq!(stored_pair(VALUE_FALSE | 1), (ANY_BOOL, 1));
        assert_eq!(stored_pair(0x1000_2000), (ANY_HEAP, 0x1000_2000));
    }
}
