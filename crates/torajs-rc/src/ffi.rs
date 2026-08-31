//! FFI wrappers — thin shims for ssa_lower-emitted IR calls.
//!
//! These keep the exact C ABI (`extern "C"`, `*mut c_void` param,
//! `i32` return on dec for legacy 0/1 verdicts) so emitted call
//! sites never needed changes. Each wrapper is the
//! null-check + reborrow + delegate; all real logic is in the
//! methods above on [`HeapHeader`].
//!
//! Extracted from the root `lib.rs` to keep that file under the
//! 500-prod-LOC file-size hard limit (`rules/common/file-size.md`).
//! Crate root re-exports both FFI symbols so external callers can
//! continue to write `torajs_rc::__torajs_rc_inc` / `_dec`.

use core::ffi::c_void;
use core::ptr::NonNull;

use crate::{DropPolicy, HeapHeader};

/// FFI bridge to [`HeapHeader::inc_ref`]. Null-safe.
///
/// # Safety
///
/// `p` is null OR a valid `*mut HeapHeader` pointing to a live
/// header. Single-threaded contract — no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_inc(p: *mut c_void) {
    // Step 7d-A — `p` may carry a NaN-box `AnyValue` bit-pattern
    // (the boxed heap ptrs share canonical 48-bit user-VA + low
    // bits zero, which `nan_box_is_cell_like` accepts; everything
    // else — Int32 / f64 / Null / Undef / Bool — has a top tag bit
    // set and skips here). This keeps the legacy "real heap ptr"
    // contract intact (real ptrs always pass `is_cell_like`)
    // while making the helper safe for ssa_lower call sites that
    // can't statically distinguish cells from immediates after the
    // Type::Any switch.
    if !nan_box_is_cell_like(p) {
        return;
    }
    if let Some(mut header) = NonNull::new(p as *mut HeapHeader) {
        // SAFETY: `p` was non-null per the NonNull match arm;
        // caller invariant says it points to a live header.
        unsafe { header.as_mut() }.inc_ref();
    }
}

/// True when `p`'s bit pattern looks like a real heap pointer
/// (top 16 bits zero, low 1 bit zero — every torajs heap object
/// is 8-aligned, so the bottom two bits are always zero, and the
/// aarch64 user-VA is 48 bits wide). NaN-box non-cell encodings
/// always set either the top tag bits (TAG_TYPE_NUMBER for Int32
/// / f64) or the low TAG_BIT_TYPE_OTHER bit (0x02 — Null / Undef
/// / Bool sentinels), so this check cleanly distinguishes the
/// two without depending on torajs-anyvalue. Mirrors the
/// `is_cell` predicate in `torajs-anyvalue::nanbox`.
///
/// `pub` since rotation 546: any is-this-cell-an-X probe must run
/// this test BEFORE `__torajs_anyv_unbox_value` — a ShortStr
/// reports tag Heap and unbox_value materializes it into an owned
/// Str a probe then abandons (one leaked Str per probe; the
/// any-concat spread test leaked exactly this way).
#[inline]
pub fn nan_box_is_cell_like(p: *mut c_void) -> bool {
    // Step 8b-B: tighten weak `(v & TAG_TYPE_NUMBER) == 0` to strict
    // `(v & TOP_16_MASK) == 0` so ShortStr (top16 = 0x0001) values
    // are correctly classified as non-cell. Mirrors the same tighten
    // in `torajs-anyvalue::nanbox::is_cell` (8b-A). No-op refactor
    // pre-8b-C — no ShortStr values exist yet.
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    let v = p as u64;
    v != 0 && (v & TOP_16_MASK) == 0 && (v & TAG_BIT_TYPE_OTHER) == 0
}

/// FFI bridge to [`HeapHeader::dec_ref`]. Null-safe. Returns
/// `1` if the caller must free the object (matches the legacy C
/// `int __torajs_rc_dec` contract that other `runtime_*.c` files
/// already consume), `0` for keep.
///
/// # Safety
///
/// Same as [`__torajs_rc_inc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(p: *mut c_void) -> i32 {
    // Step 7d-A — same NaN-box gate as `__torajs_rc_inc`: non-cell
    // bit-patterns are immediate primitives with no heap header,
    // skip without dereferencing.
    if !nan_box_is_cell_like(p) {
        return 0;
    }
    let Some(mut header) = NonNull::new(p as *mut HeapHeader) else {
        return 0;
    };
    // SAFETY: as above.
    match unsafe { header.as_mut() }.dec_ref() {
        DropPolicy::Free => 1,
        DropPolicy::Keep => 0,
    }
}

#[cfg(test)]
mod tests {
    // Note: `__torajs_weakref_target_dying` stub is provided by the
    // lib.rs tests module — both `#[cfg(test)]` blocks link into the
    // same unit-test binary, so `__torajs_rc_dec` resolves the hook.

    use super::*;
    use crate::Tag;

    #[test]
    fn ffi_rc_inc_null_is_noop() {
        unsafe { __torajs_rc_inc(core::ptr::null_mut()) };
    }

    #[test]
    fn ffi_rc_dec_null_returns_zero() {
        let r = unsafe { __torajs_rc_dec(core::ptr::null_mut()) };
        assert_eq!(r, 0);
    }

    #[test]
    fn ffi_rc_inc_increments() {
        let mut h = HeapHeader::new(Tag::Str);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        unsafe { __torajs_rc_inc(p) };
        unsafe { __torajs_rc_inc(p) };
        assert_eq!(h.refcount, 3);
    }

    #[test]
    fn ffi_rc_dec_returns_one_on_hit_zero() {
        let mut h = HeapHeader::new(Tag::Obj);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        assert_eq!(unsafe { __torajs_rc_dec(p) }, 1);
        assert_eq!(h.refcount, 0);
    }

    #[test]
    fn ffi_rc_dec_returns_zero_above_zero() {
        let mut h = HeapHeader {
            refcount: 5,
            type_tag: Tag::Obj as u16,
            flags: 0,
        };
        let p = &mut h as *mut HeapHeader as *mut c_void;
        assert_eq!(unsafe { __torajs_rc_dec(p) }, 0);
        assert_eq!(h.refcount, 4);
    }
}
