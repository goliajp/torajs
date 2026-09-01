//! `Array<Any>` → typed `Array<T>` assign-boundary conversion
//! (chunk 698). `const a: number[] = Array.from(set)` binds a
//! NaN-box block to a typed binding; pre-fix the raw-slot readers
//! misdecoded box bits as element values (silent-wrong: `[10, 20]`
//! printed as `-562949953421302, …`). The helper decode-copies
//! into a fresh typed block at the assign boundary so every
//! downstream read stays on the typed fast path (no per-read kind
//! branch on the hot path).
//!
//! Two passes over an ANY source: validate every slot coerces to
//! the target kind first (catchable TypeError before anything
//! allocates), then construct. A non-ANY source (a typed block
//! behind the static `Arr<Any>` view, chunk 621) shallow-copies
//! via `arr_slice` when its stamped kind matches; UNSET or
//! mismatched kinds throw — a raw block whose repr can't be
//! verified must not flow into a typed binding silently.

use core::ffi::{c_char, c_void};

use torajs_rc::{ARR_ELEM_KIND_SHIFT, ARR_KIND_HEAP, FLAG_ARR_ANY, HeapHeader};

use crate::any::slot_anyvalue_ptr;
use crate::any_typed_bridge::coerce_raw_scalar;
use crate::layout::{ARR_LEN_OFF, arr_data};

unsafe extern "C" {
    /// Cross-tier — torajs-anyvalue NaN-box decoders. Tag scheme:
    /// 0=Null, 1=Bool, 2=I64, 3=F64 (bits), 4=Heap, 5=Undef.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// Cross-tier — torajs-rc refcount bump (NaN-box-safe).
    fn __torajs_rc_inc(p: *mut c_void);
    /// Cross-tier — torajs-throw catchable error (record + return).
    fn __torajs_throw_type_error(msg: *const c_char);
}

/// NaN-box pair tag for heap cells (`box_to_any`'s scheme).
const TAG_HEAP: i64 = 4;

/// `__torajs_arr_any_to_typed(arr, kind)` — build a fresh typed
/// `Array<T>` (raw 8-byte slots, elem kind stamped) from an
/// `Array<Any>` value. Always copies — an aliased source keeps its
/// own layout untouched. Heap elements gain a share (+1 rc) owned
/// by the new block. Throws a catchable TypeError (returning NULL
/// before any allocation) when a slot can't carry the target repr.
///
/// # Safety
///
/// `arr` is a live Arr cell (ANY-flavor or typed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_to_typed(arr: *mut u8, kind: i64) -> *mut u8 {
    // RFC 20260810 刀 D — the rebox walk reads raw slots; loud
    // reject.
    if unsafe {
        crate::sparse_gate::sparse_tail_rejects(
            arr as *const core::ffi::c_void,
            b"sparse array tail is not yet supported in a typed-array conversion\0".as_ptr(),
        )
    } {
        return unsafe { crate::alloc::__torajs_arr_alloc_any(0) };
    }
    let want = kind as u16;
    let hdr = arr as *const HeapHeader;
    let len = unsafe { *(arr.add(ARR_LEN_OFF) as *const u64) };
    if unsafe { (*hdr).flags } & FLAG_ARR_ANY == 0 {
        // Typed block behind the static Arr<Any> view (chunk 621) —
        // the stamped kind must match the target; raw reprs then
        // copy verbatim via the slice walk.
        let have = unsafe { (*hdr).arr_elem_kind() };
        if have != want {
            return unsafe { throw_mismatch() };
        }
        let cloned = unsafe { crate::slice::__torajs_arr_slice(arr as *const u8, 0, len as i64) };
        if want == ARR_KIND_HEAP {
            for k in 0..len {
                let p = unsafe { *(arr_data(cloned).add((k as usize) * 8) as *const u64) };
                if p != 0 {
                    unsafe { __torajs_rc_inc(p as *mut c_void) };
                }
            }
        }
        return cloned;
    }
    // Pass 1 — validate every slot coerces BEFORE any allocation,
    // so the throw path leaves nothing half-built. No `unbox_value`
    // here (rotation 546 form): a ShortStr reports tag Heap and
    // unbox_value would leak one materialized Str per validated slot.
    // Tag alone decides — a HEAP tag is either a real cell (whose box
    // is a non-null pointer) or a ShortStr (materializable in pass 2),
    // and `coerce_raw_scalar` answers None for every HEAP tag anyway.
    for k in 0..len {
        let av = unsafe { *slot_anyvalue_ptr(arr, k) };
        let tag = unsafe { __torajs_anyv_unbox_tag(av) };
        let ok = if want == ARR_KIND_HEAP {
            tag == TAG_HEAP && av != 0
        } else if tag == TAG_HEAP {
            false
        } else {
            let val = unsafe { __torajs_anyv_unbox_value(av) };
            coerce_raw_scalar(want, tag as u64, val as u64).is_some()
        };
        if !ok {
            return unsafe { throw_mismatch() };
        }
    }
    // Pass 2 — construct. No throw path from here on.
    let dst = unsafe { crate::alloc::__torajs_arr_alloc(len) };
    unsafe {
        *(dst.add(ARR_LEN_OFF) as *mut u64) = len;
        // Stamp the elem kind so the block self-describes if it
        // later flows back behind an Arr<Any> view.
        let dh = dst as *mut HeapHeader;
        (*dh).flags |= want << ARR_ELEM_KIND_SHIFT;
    }
    for k in 0..len {
        let av = unsafe { *slot_anyvalue_ptr(arr, k) };
        let raw = if want == ARR_KIND_HEAP {
            if torajs_rc::ffi::nan_box_is_cell_like(av as *mut c_void) {
                // A cell's box IS the pointer; the new block owns its
                // own share.
                unsafe { __torajs_rc_inc(av as *mut c_void) };
                av
            } else {
                // ShortStr — the materialization carries a fresh rc=1
                // stake the slot adopts (an inc here double-stakes).
                unsafe { __torajs_anyv_unbox_value(av) as u64 }
            }
        } else {
            // Pass 1 proved the coercion holds (and ruled out HEAP
            // tags, so this unbox never materializes).
            let tag = unsafe { __torajs_anyv_unbox_tag(av) };
            let val = unsafe { __torajs_anyv_unbox_value(av) };
            coerce_raw_scalar(want, tag as u64, val as u64).unwrap()
        };
        unsafe { *(arr_data(dst).add((k as usize) * 8) as *mut u64) = raw };
    }
    dst
}

/// Arm the catchable TypeError and answer NULL (the SSA caller's
/// `emit_throw_check` diverts before the NULL is touched).
unsafe fn throw_mismatch() -> *mut u8 {
    unsafe {
        __torajs_throw_type_error(
            c"array element does not match the annotated element type".as_ptr(),
        );
    }
    core::ptr::null_mut()
}
