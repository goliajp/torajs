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
use crate::substr::FLAG_SUBSTR_VIEW;
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

/// Substr-block-shaped static (RFC 20260707 residual chunk — string
/// INDEX reads): `[header:8][len:8][parent:16][offset:24]` per
/// [`crate::substr`]. Its parent IS the Str sentinel cell, so every
/// view-reading consumer (print / concat / substr method probes)
/// walks `parent → "undefined"` with zero branches — the Substr
/// mirror of the Str cell's free ToString property. Identity-
/// sensitive consumers compare the ADDRESS.
#[repr(C)]
pub struct SubstrUndefSentinelCell {
    header: HeapHeader,
    length: u64,
    parent: *const UndefSentinelCell,
    offset: u64,
}

// SAFETY: read-only after link — the parent field points at another
// immortal static and FLAG_STATIC_LITERAL short-circuits every
// rc/drop write path.
unsafe impl Sync for SubstrUndefSentinelCell {}

/// The one immortal `undefined` Substr view. `s[oob]` answers this
/// (ES §10.4.3 [[Get]] — unlike charAt §22.1.3.2 which answers "").
pub static __TORAJS_SUBSTR_UNDEF_CELL: SubstrUndefSentinelCell = SubstrUndefSentinelCell {
    header: HeapHeader {
        refcount: 1,
        type_tag: Tag::Str as u16,
        flags: FLAG_STATIC_LITERAL | FLAG_SUBSTR_VIEW,
    },
    length: 9,
    parent: &__TORAJS_STR_UNDEF_CELL,
    offset: 0,
};

/// Address of the Substr sentinel cell (crate-internal shorthand).
#[inline]
pub fn substr_undef_ptr() -> *mut u8 {
    &__TORAJS_SUBSTR_UNDEF_CELL as *const SubstrUndefSentinelCell as *mut u8
}

/// True iff `p` IS the Substr sentinel (identity, never content).
#[inline]
pub fn is_substr_undef(p: *const u8) -> bool {
    core::ptr::eq(
        p,
        &__TORAJS_SUBSTR_UNDEF_CELL as *const SubstrUndefSentinelCell as *const u8,
    )
}

/// FFI accessor — ssa_lower emit sites (strict-eq / typeof on a
/// Substr slot) materialize the Substr sentinel through this call.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_substr_undef() -> *mut u8 {
    substr_undef_ptr()
}

/// `p == undefined` ONLY (either sentinel repr) — the JSON object
/// slow lane's key-skip probe (§25.5.2.4 step 8.b: undefined fields
/// skip their key, while NULL is JS `null` and keeps it — so the
/// nullish probe below is the wrong tool there).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_str_is_undef(p: *const u8) -> i64 {
    (is_undef(p) || is_substr_undef(p)) as i64
}

/// `p == null || p == undefined` for a Str- or Substr-typed value —
/// the loose-eq `s == null` / `s == undefined` runtime probe
/// (§7.2.13 steps 2-3: nullish loose-equals nullish only) and the
/// ToBoolean falsy pre-probe. One probe covers both reprs: a Str
/// slot holds NULL or the Str cell, a Substr slot holds the Substr
/// cell.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_str_is_nullish(p: *const u8) -> i64 {
    (p.is_null() || is_undef(p) || is_substr_undef(p)) as i64
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

    #[test]
    fn substr_sentinel_is_a_valid_view_of_the_str_sentinel() {
        use crate::substr::{SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF};
        let p = substr_undef_ptr();
        let len = unsafe { (p.add(SUBSTR_LEN_OFF) as *const u64).read() };
        let parent = unsafe { (p.add(SUBSTR_PARENT_OFF) as *const *const u8).read() };
        let off = unsafe { (p.add(SUBSTR_OFFSET_OFF) as *const u64).read() };
        assert_eq!(len, 9);
        assert_eq!(off, 0);
        assert!(is_undef(parent));
    }

    #[test]
    fn substr_sentinel_identity_not_content() {
        assert!(is_substr_undef(substr_undef_ptr()));
        assert!(!is_substr_undef(undef_ptr()));
        assert!(!is_undef(substr_undef_ptr()));
    }

    #[test]
    fn substr_sentinel_view_reads_undefined_text() {
        // The view walk (parent + offset/len) must answer the
        // sentinel text so ToString consumers are branch-free.
        let p = substr_undef_ptr();
        let parent =
            unsafe { (p.add(crate::substr::SUBSTR_PARENT_OFF) as *const *const u8).read() };
        let bytes =
            unsafe { core::slice::from_raw_parts(parent.add(crate::layout::STR_DATA_OFF), 9) };
        assert_eq!(bytes, b"undefined");
    }

    #[test]
    fn substr_drop_is_noop_on_the_sentinel() {
        unsafe {
            crate::substr::__torajs_substr_drop(substr_undef_ptr() as *mut core::ffi::c_void)
        };
        assert!(is_substr_undef(substr_undef_ptr()));
        assert_eq!(__TORAJS_SUBSTR_UNDEF_CELL.length, 9);
    }

    #[test]
    fn nullish_probe_covers_substr_sentinel() {
        assert_eq!(__torajs_str_is_nullish(substr_undef_ptr()), 1);
    }

    #[test]
    fn to_owned_materializes_sentinel_as_str_sentinel() {
        // Identity must propagate across the repr change (not a
        // fresh "undefined" text copy).
        let owned = unsafe { crate::substr_methods::__torajs_substr_to_owned(substr_undef_ptr()) };
        assert!(is_undef(owned as *const u8));
    }

    #[test]
    fn substr_eq_str_undefined_across_reprs() {
        use crate::substr_methods::__torajs_substr_eq_str;
        // undefined === undefined across reprs (Substr-slot sentinel
        // vs materialized Str sentinel).
        assert_eq!(
            unsafe { __torajs_substr_eq_str(substr_undef_ptr(), undef_ptr()) },
            1
        );
        // Sentinel never content-equals a real "undefined" string.
        let s = {
            let mut b = crate::block::StrBlock::alloc_with_encoding(9, true);
            unsafe { b.as_bytes_mut(9) }.copy_from_slice(b"undefined");
            b.into_raw()
        };
        assert_eq!(unsafe { __torajs_substr_eq_str(substr_undef_ptr(), s) }, 0);
        unsafe { crate::block::__torajs_str_free(s) };
    }

    #[test]
    fn substr_index_view_oob_and_propagation() {
        use crate::substr_methods::__torajs_substr_index_view;
        // Sentinel receiver propagates itself; OOB on a real view
        // answers the sentinel.
        assert!(is_substr_undef(unsafe {
            __torajs_substr_index_view(substr_undef_ptr(), 0)
        }));
    }
}
