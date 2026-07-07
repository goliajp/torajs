//! RFC 20260707-undefined-sentinel-repr chunk 2 — the immortal JS
//! `undefined` sentinel Str cell.
//!
//! One runtime value used to carry two JS values: a NULL ptr in a
//! Str slot meant `undefined` (591 convention) while a real `null`
//! in a `Nullable<Str>` slot shared the same bits, so print / eq /
//! typeof could not tell them apart. The sentinel splits them:
//!
//! - `undefined` = the address of [`__TORAJS_STR_UNDEF_CELL`] — a
//!   static immortal Str block (JSC/V8 oddball shape; the mmalloc
//!   `zero_sentinel` is the in-repo precedent for a distinguished
//!   non-NULL identity-compared pointer). Its payload is the text
//!   "undefined", so every ToString-semantics consumer (print,
//!   concat, template literals) reads the right answer with zero
//!   branches; `FLAG_STATIC_LITERAL` makes rc_inc / rc_dec /
//!   str_drop / str_free no-ops through the existing machinery.
//! - NULL ptr goes back to meaning JS `null`.
//!
//! Identity-sensitive consumers (str_eq, JSON, the null guard,
//! typeof) branch on the ADDRESS — the cell must therefore never be
//! shared with an interned `"undefined"` literal.

use crate::layout::STR_FLAG_IS_LATIN1;
use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader, Tag};

/// Str-block-shaped static: `[header:8][length:4][_pad:4][bytes:9]`
/// per [`crate::layout`].
#[repr(C)]
pub struct UndefSentinelCell {
    header: HeapHeader,
    length: u32,
    _pad: u32,
    data: [u8; 9],
}

/// The one immortal `undefined` cell. Read-only after link; the
/// refcount field is never touched (`FLAG_STATIC_LITERAL` short-
/// circuits every rc/drop path), so sharing it across threads is
/// sound.
pub static __TORAJS_STR_UNDEF_CELL: UndefSentinelCell = UndefSentinelCell {
    header: HeapHeader {
        refcount: 1,
        type_tag: Tag::Str as u16,
        flags: FLAG_STATIC_LITERAL | STR_FLAG_IS_LATIN1,
    },
    length: 9,
    _pad: 0,
    data: *b"undefined",
};

/// Address of the sentinel cell (crate-internal shorthand).
#[inline]
pub fn undef_ptr() -> *mut u8 {
    &__TORAJS_STR_UNDEF_CELL as *const UndefSentinelCell as *mut u8
}

/// True iff `p` IS the sentinel (identity, never content).
#[inline]
pub fn is_undef(p: *const u8) -> bool {
    core::ptr::eq(
        p,
        &__TORAJS_STR_UNDEF_CELL as *const UndefSentinelCell as *const u8,
    )
}

/// FFI accessor — producers in other runtime tiers (torajs-regex
/// miss-capture slots) and ssa_lower emit sites materialize the
/// sentinel through this call.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_str_undef() -> *mut u8 {
    undef_ptr()
}

/// `p == null || p == undefined` for a Str-typed value — the
/// loose-eq `s == null` / `s == undefined` runtime probe
/// (§7.2.13 steps 2-3: nullish loose-equals nullish only).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_str_is_nullish(p: *const u8) -> i64 {
    (p.is_null() || is_undef(p)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_is_a_valid_latin1_str_block() {
        let p = undef_ptr();
        let len = unsafe { (p.add(crate::layout::STR_LEN_OFF) as *const u32).read() };
        assert_eq!(len, 9);
        let bytes = unsafe { core::slice::from_raw_parts(p.add(crate::layout::STR_DATA_OFF), 9) };
        assert_eq!(bytes, b"undefined");
    }

    #[test]
    fn identity_not_content() {
        assert!(is_undef(undef_ptr()));
        let other = *b"undefined";
        assert!(!is_undef(other.as_ptr()));
    }

    #[test]
    fn nullish_probe_covers_both() {
        assert_eq!(__torajs_str_is_nullish(core::ptr::null()), 1);
        assert_eq!(__torajs_str_is_nullish(undef_ptr()), 1);
        let x: u64 = 0;
        assert_eq!(__torajs_str_is_nullish(&x as *const u64 as *const u8), 0);
    }

    #[test]
    fn rc_and_drop_are_noops_on_the_sentinel() {
        // FLAG_STATIC_LITERAL short-circuits str_drop; the cell is
        // untouched (would segfault/corrupt otherwise).
        unsafe { crate::__torajs_str_drop(undef_ptr()) };
        assert!(is_undef(undef_ptr()));
        assert_eq!(__TORAJS_STR_UNDEF_CELL.length, 9);
    }
}
