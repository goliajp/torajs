//! §23.2.3.16 `%TypedArray%.prototype.join`
//! (RFC 20260823-typedarray-substrate 刀 5, slab A's last name).
//!
//! §23.2.3.16 and §23.1.3.15 are the same six steps over a different
//! length source: separator once, then each element, `undefined`
//! rendering as the empty string. So this delegates to
//! `__torajs_arr_join` rather than growing a second copy of the
//! walk — and the copy it would have grown is not small. A Str
//! payload is Latin-1 or UTF-16 (P11.1-S2), the output has to pick
//! the widest of its inputs, and the separator is the one piece here
//! that can be either. That machine already exists in torajs-arr and
//! has been fixed at least once for the encoding question
//! (rotation 473); a second one would be a second thing to fix.
//!
//! The elements themselves are always ASCII — a typed array holds
//! Numbers or BigInts — so the temporary array carries the only
//! interesting case across, which is the separator.
//!
//! An index past the live extent reads `undefined`, which the array
//! join renders as the empty string. That is not a special case
//! bolted on here: it is step 4 of both algorithms, and it is how a
//! view whose buffer shrank under it answers.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_undefined};

unsafe extern "C" {
    fn __torajs_typedarray_validate(av: AnyValue) -> i64;
    fn __torajs_typedarray_index_get(av: AnyValue, index: f64) -> AnyValue;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    /// The Any-element join. NOT `__torajs_arr_join`, which is the
    /// `Array<Str>` one and reads every slot as a Str pointer — the
    /// two names differ by four characters and by whether a
    /// NaN-boxed double is dereferenced as an address.
    fn __torajs_arr_any_join(arr: *const u8, sep: *const u8) -> AnyValue;
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// §7.1.17 ToString — a freshly owned Str the caller drops.
    fn __torajs_anyv_to_str(v: AnyValue) -> *mut c_void;
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_unbox_tag(v: AnyValue) -> i64;
    fn __torajs_anyv_unbox_value(v: AnyValue) -> i64;
}

/// §23.2.3.16.
///
/// # Safety
/// `recv` is a live TypedArray AnyValue; `argv` holds `argc` live
/// AnyValues.
pub(crate) unsafe fn typedarray_join(recv: AnyValue, argv: *const u64, argc: i64) -> AnyValue {
    unsafe {
        let len = __torajs_typedarray_validate(recv);
        if len < 0 {
            return VALUE_UNDEFINED;
        }
        // Step 3 — an absent or undefined separator is ",", and it
        // is coerced BEFORE any element is read.
        let sep_arg = if argc > 0 { *argv } else { VALUE_UNDEFINED };
        let sep = if is_undefined(sep_arg) {
            __torajs_str_alloc(b",".as_ptr(), 1) as *mut c_void
        } else {
            let s = __torajs_anyv_to_str(sep_arg);
            if __torajs_throw_check() != 0 {
                if !s.is_null() {
                    __torajs_str_drop(s);
                }
                return VALUE_UNDEFINED;
            }
            s
        };
        // Capacity is a hint only — push grows on demand — so a
        // huge length must not size the malloc.
        let mut arr = __torajs_arr_alloc_any(len.clamp(0, 4096) as u64);
        for k in 0..len {
            let v = __torajs_typedarray_index_get(recv, k as f64);
            // `push_any` TRANSFERS the stake into the slot (the
            // flat-map append next door relies on the same thing), so
            // the read's reference is not released here — releasing
            // it would be a double free that only a BigInt-element
            // join could show.
            arr = __torajs_arr_push_any(
                arr as *mut c_void,
                __torajs_anyv_unbox_tag(v) as u64,
                __torajs_anyv_unbox_value(v) as u64,
            );
        }
        let out = __torajs_arr_any_join(arr as *const u8, sep as *const u8);
        __torajs_str_drop(sep);
        __torajs_value_drop_heap(arr as *mut c_void);
        out
    }
}
