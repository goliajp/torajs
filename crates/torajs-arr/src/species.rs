//! §9.4.2.3 ArraySpeciesCreate constructor-face guard (RFC
//! 20260713-array-proto-residual blade 3).
//!
//! The species family (concat / filter / flat / flatMap / map /
//! slice / splice) reads `Get(O, "constructor")` before creating
//! its product. tr always creates plain Arrays, which is
//! spec-equivalent for the overwhelmingly common shapes — absent /
//! `undefined` constructor (step 6 default), or an object whose
//! `@@species` is undefined (steps 5-6: tr values never carry
//! `@@species`). The one OBSERVABLE divergence is step 7: a
//! present non-object non-undefined constructor (`a.constructor =
//! null / 1 / "s" / false`) must throw TypeError instead of
//! silently defaulting.
//!
//! Recorded boundary: a same-realm CONSTRUCTOR function (step 4)
//! would be used to build the product per spec — tr defaults to
//! Array (matching the no-`@@species` fn shape, diverging for real
//! subclass ctors). An accessor `constructor` entry's getter RUNS
//! (spec Get, RFC 20260721 刀 10 G5a) — its throw propagates and
//! its answer takes the same step 5-7 classification as a data
//! entry.
//!
//! Fast path: an array that never grew expando props is one
//! NULL-check (`ARR_PROPS_OFF` load).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::layout::ARR_PROPS_OFF;

unsafe extern "C" {
    /// torajs-str — key alloc for the props probe.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — catchable TypeError (records via TLS; the
    /// compiled caller's throw check propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — pending-throw probe (the accessor getter leg).
    fn __torajs_throw_check() -> i64;
    /// torajs-dynobj — run an accessor entry's getter; owned answer.
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    /// torajs-anyvalue — NaN-box pack/unpack (receiver box + the
    /// getter's answer).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// Universal NaN-box-safe heap dropper (getter answer release).
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Guard the species-family constructor face. Returns 1 when a
/// TypeError was recorded (the any-tier dispatcher answers
/// undefined early; compiled callers rely on the throw check), 0
/// when the derive may proceed with the default Array product.
///
/// The props-NULL fast path answers here; a receiver with a props
/// bag goes through the `__torajs_arr_species_guard_slow` link seam
/// (`crate::exotic_seam`) to [`__torajs_arr_species_guard_props`],
/// so a program in which no array can grow a bag links none of the
/// accessor / dispatch machinery the slow path reaches.
///
/// # Safety
/// `arr` is a live array heap block pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_species_guard(arr: *const u8) -> i64 {
    unsafe {
        let props = *(arr.add(ARR_PROPS_OFF) as *const *const c_void);
        if props.is_null() {
            return 0;
        }
        crate::exotic_seam::__torajs_arr_species_guard_slow(arr)
    }
}

/// The slow half of [`__torajs_arr_species_guard`]: the receiver has
/// a props bag, so `constructor` may be present — read it and
/// classify per §9.4.2.3 steps 4-7.
///
/// # Safety
/// `arr` is a live array heap block pointer whose props slot is
/// non-NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_species_guard_props(arr: *const u8) -> i64 {
    unsafe {
        let key = __torajs_str_alloc(b"constructor".as_ptr(), 11);
        let dtag =
            crate::props::__torajs_arrprops_get_tag(arr as *mut c_void, key as *const c_void);
        let dval =
            crate::props::__torajs_arrprops_get_value(arr as *mut c_void, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        classify_ctor_pair_entry(arr, dtag, dval)
    }
}

/// The per-entry dispatch (probe pair or accessor-getter answer).
unsafe fn classify_ctor_pair_entry(arr: *const u8, dtag: u64, dval: u64) -> i64 {
    unsafe {
        match dtag {
            // Absent or explicit undefined — step 6 default Array.
            5 => 0,
            // Accessor entry — §7.3.2 Get runs the getter (刀 10
            // G5a: a poisoned `constructor` getter's throw is
            // observable); the answer classifies like a data entry.
            6 => {
                let got = __torajs_accessor_invoke_getter(
                    dval as *const c_void,
                    __torajs_anyv_box_from_pair(4, arr as i64),
                );
                if __torajs_throw_check() != 0 {
                    __torajs_value_drop_heap(got as *mut c_void);
                    return 1;
                }
                let gtag = __torajs_anyv_unbox_tag(got);
                let gval = __torajs_anyv_unbox_value(got);
                let verdict = classify_ctor_pair_entry(arr, gtag as u64, gval as u64);
                __torajs_value_drop_heap(got as *mut c_void);
                verdict
            }
            // Heap cell: objects (dynobj / struct / closure / any
            // container) reach steps 5-6 → @@species undefined →
            // default; heap-shaped PRIMITIVES (Str / Symbol /
            // BigInt) are step 7 non-constructors.
            4 => {
                let tag = ((dval as *const u8).add(4) as *const u16).read();
                if tag == Tag::Str as u16 || tag == Tag::Symbol as u16 || tag == Tag::BigInt as u16
                {
                    __torajs_throw_type_error(
                        c"array species constructor is not a constructor".as_ptr(),
                    );
                    1
                } else {
                    0
                }
            }
            // null / bool / number primitives — step 7 TypeError.
            _ => {
                __torajs_throw_type_error(
                    c"array species constructor is not a constructor".as_ptr(),
                );
                1
            }
        }
    }
}
