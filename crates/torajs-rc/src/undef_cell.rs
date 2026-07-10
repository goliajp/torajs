//! RFC 20260710-optional-undefined-repr C2b — the generic immortal
//! JS `undefined` oddball cell for pointer-shaped struct slots.
//!
//! An Obj / Arr / Closure struct slot is a single pointer: NULL means
//! JS `null` (591 convention), so `{ inner: undefined }` used to
//! collapse both values onto the same bits — `=== null` answered
//! true and print rendered null. The sentinel splits them, mirroring
//! the Str-family cell in `torajs-str::undef_sentinel` (JSC/V8
//! oddball shape):
//!
//! - `undefined` = the address of [`__TORAJS_UNDEF_CELL`] — a bare
//!   static header block with its own [`Tag::Undefined`], so every
//!   runtime tag-dispatch walker (value_drop_heap catch-all, cycle
//!   collector) routes it to an rc-gated no-op path without knowing
//!   about it; `FLAG_STATIC_LITERAL` short-circuits inc/dec/drop.
//! - NULL ptr keeps meaning JS `null`.
//!
//! Unlike the Str cell there is no ToString payload: every consumer
//! of these slot types is a branch site (strict-eq compares the
//! ADDRESS, ToBoolean answers falsy, print / JSON probe identity),
//! so one generic cell serves all pointer-shaped slot types.

use crate::{FLAG_STATIC_LITERAL, HeapHeader, Tag};

/// The one immortal generic `undefined` cell. Read-only after link;
/// the refcount field is never touched (`FLAG_STATIC_LITERAL`
/// short-circuits every rc/drop path), so sharing it across threads
/// is sound. Exported unmangled so ssa_lower can materialize the
/// address with a `GlobalRef` (ADRP+ADD, zero calls) — the Mach-O
/// symbol is `___TORAJS_UNDEF_CELL`.
#[unsafe(no_mangle)]
pub static __TORAJS_UNDEF_CELL: HeapHeader = HeapHeader {
    refcount: 1,
    type_tag: Tag::Undefined as u16,
    flags: FLAG_STATIC_LITERAL,
};

/// Address of the generic sentinel cell (crate-internal shorthand).
#[inline]
pub fn undef_cell_ptr() -> *mut u8 {
    &__TORAJS_UNDEF_CELL as *const HeapHeader as *mut u8
}

/// True iff `p` IS the generic sentinel (identity, never content).
#[inline]
pub fn is_undef_cell(p: *const u8) -> bool {
    core::ptr::eq(p, &__TORAJS_UNDEF_CELL as *const HeapHeader as *const u8)
}

/// FFI identity probe — struct print / descriptor reflection and the
/// JSON object key-skip lane call this on a raw pointer slot to tell
/// "JS undefined" from a live heap cell.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_is_undef_cell(p: *const u8) -> i64 {
    is_undef_cell(p) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_carries_the_undefined_tag_and_static_flag() {
        assert_eq!(__TORAJS_UNDEF_CELL.type_tag, Tag::Undefined as u16);
        assert!(__TORAJS_UNDEF_CELL.is_static_literal());
    }

    #[test]
    fn identity_not_content() {
        assert!(is_undef_cell(undef_cell_ptr()));
        let other = HeapHeader {
            refcount: 1,
            type_tag: Tag::Undefined as u16,
            flags: FLAG_STATIC_LITERAL,
        };
        assert!(!is_undef_cell(&other as *const HeapHeader as *const u8));
    }

    #[test]
    fn rc_ops_are_noops_on_the_cell() {
        // FLAG_STATIC_LITERAL short-circuits the FFI inc/dec — the
        // static header is never written (would corrupt/fault
        // otherwise).
        unsafe {
            crate::ffi::__torajs_rc_inc(undef_cell_ptr() as *mut core::ffi::c_void);
            crate::ffi::__torajs_rc_dec(undef_cell_ptr() as *mut core::ffi::c_void);
        }
        assert_eq!(__TORAJS_UNDEF_CELL.refcount, 1);
    }

    #[test]
    fn ffi_probe_answers_identity() {
        assert_eq!(__torajs_is_undef_cell(undef_cell_ptr()), 1);
        let x: u64 = 0;
        assert_eq!(__torajs_is_undef_cell(&x as *const u64 as *const u8), 0);
    }
}
