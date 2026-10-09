//! `Array<Any>` allocation: malloc-only, outside the cap-matched pool
//! (8-byte NaN-box stride vs the pool's typed-slot ledger).

use core::ffi::c_void;

use torajs_rc::{AnySlotTag, FLAG_ARR_ANY};

use crate::alloc::ARR_CAP_LOW32_OFF;
use crate::any::{ANY_SLOT_BYTES, ANY_UNDEF, slot_anyvalue_ptr};
use crate::layout::{ARR_CELL_SIZE, ARR_DATA_PTR_OFF, ARR_LEN_OFF, ARR_PROPS_OFF, TAG_ARR};

/// Head-offset slot (matches `any.rs`).
const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_calloc"]
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
}

unsafe extern "C" {
    /// Cross-tier — torajs-anyvalue NaN-box pack (undefined fill).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// Cross-tier — torajs-anyvalue NaN-box tag probe (I64/F64
    /// dispatch for the `new Array(any)` runtime type test).
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    /// Cross-tier — legacy raw-value decode. Only called on the
    /// number tags here (ShortStr materializes under it — the
    /// element path stores the AnyValue bits untouched instead).
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// Cross-tier — torajs-rc. NaN-box-safe: no-op for immediates,
    /// bumps the wrapped heap pointer's refcount for cells.
    fn __torajs_rc_inc(p: *mut c_void);
    /// Cross-tier — torajs-throw catchable RangeError.
    fn __torajs_throw_range_error(msg: *const u8);
}

#[inline]
pub(crate) unsafe fn write_header_any(p: *mut u8, len: u64, cap: u32) {
    unsafe {
        *(p as *mut u32) = 1; // refcount
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = FLAG_ARR_ANY;
        *(p.add(ARR_LEN_OFF) as *mut u64) = len;
        *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = cap;
        *(p.add(ARR_HEAD_OFF) as *mut u32) = 0; // Any-arrays never deque-shift
        // Round 4 chunk 5a — inline props_dynobj slot initialized to NULL.
        *(p.add(ARR_PROPS_OFF) as *mut u64) = 0;
        // B1 — data pointer starts self-referential (inline slots).
        *(p.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = p.add(ARR_CELL_SIZE);
    }
}

/// `__torajs_arr_alloc_any(cap)` — fresh empty Array<Any>.
/// Bypasses the regular Array<T> pool (different slot stride).
///
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any(cap: u64) -> *mut u8 {
    unsafe {
        let total = ARR_CELL_SIZE + (cap as usize) * ANY_SLOT_BYTES;
        let p = malloc(total) as *mut u8;
        write_header_any(p, 0, cap as u32);
        p
    }
}

/// `__torajs_arr_alloc_any_filled(n)` — `new Array(n)` per ES spec
/// §23.1.2.1. len=cap=n, all slots boxed `ANY_UNDEF` so `arr[i]`
/// decodes as `undefined` per ES §10.4.2.1 (sparse missing-index
/// semantics densely emulated; true sparse hole would need an
/// elem-kind-tag substrate, L3b). Mirrors the hole-fill pattern in
/// `__torajs_arr_set_at_any`.
///
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8 {
    if !length_in_range(n) {
        return core::ptr::null_mut();
    }
    unsafe {
        let total = ARR_CELL_SIZE + (n as usize) * ANY_SLOT_BYTES;
        let p = malloc(total) as *mut u8;
        write_header_any(p, n, n as u32);
        if n > 0 {
            let undef = __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0);
            for k in 0..n {
                *slot_anyvalue_ptr(p, k) = undef;
            }
        }
        p
    }
}

/// §23.1.2.1 step 4.b — a length outside [0, 2^32-1] is a RangeError
/// (test262-bug-corpus RC-4 F5: `new Array(-1)` arrives as 0xFFFF..FF
/// u64 and SIGSEGVd through the overflowed malloc). Message matches
/// bun/JSC for uncaught-print byte parity. The throw helper RETURNS;
/// ssa_lower's emit_throw_check right after the call diverts before
/// the NULL is touched.
fn length_in_range(n: u64) -> bool {
    const MAX_ARRAY_LEN: u64 = u32::MAX as u64;
    if n > MAX_ARRAY_LEN {
        unsafe {
            __torajs_throw_range_error(
                c"Array length must be a positive integer of safe magnitude."
                    .as_ptr()
                    .cast(),
            );
        }
        return false;
    }
    true
}

/// `__torajs_arr_alloc_any_holes(n)` — `new Array(n)` per §23.1.2.1:
/// ArrayCreate sets `length` and creates NO element — every index is a
/// hole. The slots are the reserved encoding `0`
/// (`FLAG_ARR_ANY_HOLEY`), which `calloc` hands over already (lazily,
/// page by page: `new Array(1e6)` touches nothing until written), and a
/// later store simply overwrites one. The Array-constructor lanes all
/// land here; `__torajs_arr_alloc_any_filled` stays for the runtime
/// callers that fill every slot themselves.
///
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any_holes(n: u64) -> *mut u8 {
    if !length_in_range(n) {
        return core::ptr::null_mut();
    }
    unsafe {
        let p = calloc(1, ARR_CELL_SIZE + (n as usize) * ANY_SLOT_BYTES) as *mut u8;
        write_header_any(p, n, n as u32);
        if n > 0 {
            crate::exotic_seam::__torajs_arr_flag_any_holey(p as *mut c_void);
        }
        p
    }
}

/// `__torajs_arr_alloc_any_filled_f64(len)` — `new Array(len)` for a
/// Number operand that is not provably an integer at compile time.
/// §23.1.2.1 step 4.b: a len with ToUint32(len) != len (NaN /
/// ±Infinity / fractional / negative / > 2^32-1) is a RangeError.
/// The f64 entry exists because the SSA i64 coercion (FpToSi / const
/// fold) erases exactly the bits this check needs; integer-provable
/// operands stay on the u64 entry above. The round-trip cast check
/// avoids libm (`trunc`) — `as u64` saturates NaN/negative to 0 and
/// overflow to u64::MAX, all of which fail the equality or the range
/// guards. `-0.0` passes (`-0.0 >= 0.0`, round-trips to `0.0`) per
/// the spec's numeric (not SameValue) comparison.
///
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any_filled_f64(len: f64) -> *mut u8 {
    const MAX_ARRAY_LEN: f64 = u32::MAX as f64;
    if !((0.0..=MAX_ARRAY_LEN).contains(&len) && (len as u64) as f64 == len) {
        unsafe {
            __torajs_throw_range_error(
                c"Array length must be a positive integer of safe magnitude."
                    .as_ptr()
                    .cast(),
            );
        }
        return core::ptr::null_mut();
    }
    unsafe { __torajs_arr_alloc_any_holes(len as u64) }
}

/// `__torajs_arr_new_from_any(v)` — `new Array(len)` for an Any
/// operand. §23.1.1.1 step 4 branches on whether the argument IS a
/// Number, not on coercing it to one: `new Array('3')` is `['3']`
/// (length 1) while `new Array(3)` is length 3, so the Any lane is
/// a runtime type test. Number tags reuse the validated length
/// entries above (a negative i64 arrives as an out-of-range u64;
/// the f64 entry keeps the NaN / ±Infinity / fractional bits the
/// step-4.b RangeError needs). Every other tag allocates a
/// length-1 Array<Any> holding the value. The box is borrowed:
/// the slot takes its own +1 via the NaN-box-safe rc_inc (no-op
/// for immediates), and `unbox_value` is never called on this
/// path — ShortStr stays inline instead of materializing.
///
/// # Safety
/// `v` is a live boxed value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_new_from_any(v: u64) -> *mut u8 {
    unsafe {
        let tag = __torajs_anyv_unbox_tag(v);
        if tag == AnySlotTag::I64 as i64 {
            return __torajs_arr_alloc_any_holes(__torajs_anyv_unbox_value(v) as u64);
        }
        if tag == AnySlotTag::F64 as i64 {
            return __torajs_arr_alloc_any_filled_f64(f64::from_bits(
                __torajs_anyv_unbox_value(v) as u64
            ));
        }
        let p = __torajs_arr_alloc_any_filled(1);
        *slot_anyvalue_ptr(p, 0) = v;
        __torajs_rc_inc(v as *mut c_void);
        p
    }
}
