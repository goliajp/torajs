//! The longest string a Str holds, and the one test every string
//! builder puts its result length through before it allocates.
//!
//! A builder that knows its result length up front asks [`str_len_of`]
//! and, on `None`, answers [`__torajs_str_too_long`]: a pending
//! RangeError plus an empty placeholder the call site's throw check
//! discards. Runtime crates that link against this one only over the C
//! ABI raise through the same symbol and mirror [`STR_MAX_LEN`] by
//! name, the way they mirror the layout offsets.

use crate::block::StrBlock;
use crate::layout::byte_capacity;

unsafe extern "C" {
    fn __torajs_throw_range_error(msg: *const u8);
}

/// The most code units a Str holds: 2^31 - 1.
///
/// The length field is a u32, and a UTF-16 payload's byte count,
/// twice its length, has to fit one too. JSC's `String::MaxLength` is
/// the same bound, so every string bun builds can be built here (V8
/// stops lower, at 2^29 - 24). ECMA-262 leaves the bound to the
/// implementation (6.1.4); building a longer string raises a
/// RangeError.
pub const STR_MAX_LEN: usize = (1 << 31) - 1;

/// `units` as a Str length, or `None` when no Str holds that many.
#[inline(always)]
pub const fn str_len_of(units: usize) -> Option<u32> {
    if units > STR_MAX_LEN {
        None
    } else {
        Some(units as u32)
    }
}

/// The most payload bytes a Str in the given encoding holds; a grown
/// cell's slack never goes past it, so an append that fits the slack
/// also fits [`STR_MAX_LEN`].
#[inline(always)]
pub const fn str_max_bytes(is_latin1: bool) -> u32 {
    byte_capacity(STR_MAX_LEN as u32, is_latin1)
}

/// Raise the RangeError for a string longer than [`STR_MAX_LEN`] and
/// answer an empty Str, which the caller's throw check discards.
///
/// # Safety
///
/// Records a pending throw: the caller returns to code that checks
/// for one. The answer is an owned refcount-1 Str.
#[cold]
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_too_long() -> *mut u8 {
    unsafe { __torajs_throw_range_error(c"Invalid string length".as_ptr().cast()) };
    StrBlock::alloc_with_encoding(0, true).into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bound_is_the_largest_length_a_utf16_payload_fits() {
        assert_eq!(str_len_of(STR_MAX_LEN), Some(u32::MAX / 2));
        assert_eq!(str_len_of(STR_MAX_LEN + 1), None);
        assert_eq!(str_len_of(usize::MAX), None);
        assert_eq!(str_max_bytes(false), u32::MAX - 1);
        assert_eq!(str_max_bytes(true), u32::MAX / 2);
    }
}
